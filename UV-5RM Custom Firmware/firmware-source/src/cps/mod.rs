mod frame;

use cortex_m::peripheral::syst::SystClkSource;
use cortex_m::peripheral::{SCB, SYST};

use crate::app::App;
use crate::device::display::Display;
use crate::drivers::norflash::SECTOR_SIZE;
use crate::flash_map::{self, addr};
use crate::hal;
use crate::ui;

const HANDSHAKE: &[u8] = b"PROGRAMBF-5RH";
const ACK: u8 = 0x06;
const IDLE_TIMEOUT_MS: u32 = 2000;
const MAX_CHANNEL_NUM: u16 = 999;

const SAT_TABLE_BASE: u32 = addr::OVERLAY_DATA_ADDR;
/// 20 records * 256 bytes, spanning two 4KB sectors.
const SAT_TABLE_SIZE: u32 = 5120;

pub struct HandshakeDetector {
    matched: usize,
}

impl HandshakeDetector {
    pub const fn new() -> Self {
        HandshakeDetector { matched: 0 }
    }

    pub fn poll(&mut self) -> bool {
        while let Some(byte) = hal::uart::take_byte() {
            if byte == HANDSHAKE[self.matched] {
                self.matched += 1;
                if self.matched == HANDSHAKE.len() {
                    self.matched = 0;
                    return true;
                }
            } else {
                self.matched = usize::from(byte == HANDSHAKE[0]);
            }
        }
        false
    }
}

pub fn run_session(
    app: &mut App<'_>,
    syst: &mut SYST,
    display: &mut Display<'_>,
) -> ! {
    app.radio_mut().rf_off(syst);
    app.storage_mut().reset_channel_write_session();

    ui::cps::draw_programming(display.as_draw_target());

    hal::uart::write_byte(ACK);

    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(crate::hal::clock::SCLK_HZ / 1000 - 1);
    syst.clear_current();
    syst.enable_counter();

    let mut reader = frame::FrameReader::new();
    let mut idle_ms: u32 = 0;
    let mut app_slot: Option<u8> = None;

    loop {
        if syst.has_wrapped() {
            idle_ms += 1;
            if idle_ms >= IDLE_TIMEOUT_MS {
                SCB::sys_reset();
            }
        }

        let Some(byte) = hal::uart::take_byte() else {
            continue;
        };
        idle_ms = 0;

        let frame = match reader.feed(byte) {
            frame::FeedResult::Pending => continue,
            frame::FeedResult::CrcError { addr } => {
                send_error(addr, frame::ERR_BAD_CRC);
                continue;
            }
            frame::FeedResult::Frame(frame) => frame,
        };

        match frame.cmd {
            frame::CMD_READ => handle_read(app, frame.addr),
            #[cfg(feature = "ar5rm-nrf")]
            frame::CMD_READ_BG => handle_read_logo(app, frame.addr, frame.cmd),
            frame::CMD_WRITE | frame::CMD_WRITE_LOGO | frame::CMD_WRITE_BG => {
                #[cfg(not(feature = "ar5rm-nrf"))]
                if frame.cmd == frame::CMD_WRITE_BG {
                    send_error(frame.addr, frame::ERR_BAD_LEN);
                    continue;
                }
                #[cfg(feature = "ar5rm-nrf")]
                if !app.storage_mut().norflash.user_data_ready() {
                    send_error(frame.addr, frame::ERR_BAD_ADDR);
                    continue;
                }
                if frame.cmd == frame::CMD_WRITE {
                    handle_write(app, frame.addr, frame.data)
                } else {
                    handle_write_logo(app, frame.addr, frame.data, frame.cmd)
                }
            }
            frame::CMD_READ_LOGO => handle_read_logo(app, frame.addr, frame.cmd),
            frame::CMD_APP_ERASE => {
                app_slot = handle_app_erase(app, frame.addr);
            }
            frame::CMD_APP_WRITE => handle_app_write(
                app,
                app_slot,
                frame.addr,
                frame.data,
            ),
            #[cfg(not(feature = "ar5rm-nrf"))]
            frame::CMD_SAT_WRITE => {
                handle_sat_write(app, frame.addr, frame.data)
            }
            frame::CMD_READ_FLASH_RAW => handle_read_flash_raw(app, frame.addr),
            frame::CMD_END => {
                send_response(frame::CMD_END, frame.addr, &[]);
                hal::uart::flush(); // let the ACK bytes drain out before reset
                SCB::sys_reset();
            }
            _ => send_error(frame.addr, frame::ERR_BAD_LEN),
        }
    }
}

fn handle_read(app: &mut App<'_>, wire_addr: u16) {
    let logical = wire_addr as u32;
    let mut buf = [0u8; frame::MAX_DATA];

    let len = if logical < addr::VFO_INFO_ADDR {
        read_channel(app, logical, &mut buf)
    } else if logical == addr::VFO_INFO_ADDR {
        let raw = app.storage_mut().load_vfo_raw().unwrap_or([0xFF; 64]);
        buf[..64].copy_from_slice(&raw);
        Some(64)
    } else if logical == addr::RADIO_IMFOS_ADDR {
        #[cfg(feature = "ar5rm-nrf")]
        let settings = app
            .storage_mut()
            .load_settings_raw()
            .unwrap_or_else(|| flash_map::Settings::DEFAULT.to_bytes());
        #[cfg(not(feature = "ar5rm-nrf"))]
        let settings = app
            .storage_mut()
            .load_settings()
            .unwrap_or(flash_map::Settings::DEFAULT)
            .to_bytes();
        buf[..flash_map::SETTINGS_BYTES].copy_from_slice(&settings);
        Some(flash_map::SETTINGS_BYTES)
    } else if is_contact_addr(logical) {
        let n = addr::CONTACT_SIZE as usize;
        app.storage_mut().read_raw(logical, &mut buf[..n]);
        Some(n)
    } else if logical == addr::FM_ADDR {
        let raw = app
            .storage_mut()
            .load_fm_raw()
            .unwrap_or([0xFF; flash_map::FM_CHANNEL_COUNT * 2]);
        let n = raw.len();
        buf[..n].copy_from_slice(&raw);
        Some(n)
    } else {
        None
    };

    match len {
        Some(len) => send_response(frame::CMD_READ, wire_addr, &buf[..len]),
        None => send_error(wire_addr, frame::ERR_BAD_ADDR),
    }
}

fn read_channel(
    app: &mut App<'_>,
    logical: u32,
    buf: &mut [u8; frame::MAX_DATA],
) -> Option<usize> {
    if !logical.is_multiple_of(addr::CHAN_SIZE) {
        return None;
    }
    let num = (logical / addr::CHAN_SIZE) as u16;
    if num > MAX_CHANNEL_NUM {
        return None;
    }
    let len = addr::CHAN_SIZE as usize;
    app.storage_mut().read_raw(logical, &mut buf[..len]);
    Some(len)
}

#[cfg(not(feature = "ar5rm-nrf"))]
fn handle_write(app: &mut App<'_>, wire_addr: u16, data: &[u8]) {
    let logical = wire_addr as u32;

    let ok = if logical < addr::VFO_INFO_ADDR {
        write_channel(app, logical, data)
    } else if logical == addr::VFO_INFO_ADDR && data.len() == 64 {
        let raw: [u8; 64] = data.try_into().unwrap();
        app.storage_mut().save_vfo_raw(&raw);
        true
    } else if logical == addr::RADIO_IMFOS_ADDR
        && data.len() == flash_map::SETTINGS_BYTES
    {
        let raw: [u8; flash_map::SETTINGS_BYTES] = data.try_into().unwrap();
        app.storage_mut()
            .save_settings(&flash_map::Settings::from_bytes(&raw));
        true
    } else if is_contact_addr(logical)
        && data.len() == addr::CONTACT_SIZE as usize
    {
        let idx = contact_index(logical);
        let raw: [u8; addr::CONTACT_SIZE as usize] = data.try_into().unwrap();
        app.storage_mut()
            .write_contact(idx, &flash_map::Contact::from_bytes(&raw));
        true
    } else if logical == addr::FM_ADDR
        && data.len() == flash_map::FM_CHANNEL_COUNT * 2
    {
        let mut channels =
            [flash_map::FM_CHANNEL_EMPTY; flash_map::FM_CHANNEL_COUNT];
        for (v, pair) in channels.iter_mut().zip(data.chunks_exact(2)) {
            *v = u16::from_le_bytes([pair[0], pair[1]]);
        }
        app.storage_mut().save_fm_channels(&channels);
        true
    } else {
        false
    };

    if ok {
        send_response(frame::CMD_WRITE, wire_addr, &[]);
    } else {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
    }
}

#[cfg(feature = "ar5rm-nrf")]
fn handle_write(app: &mut App<'_>, wire_addr: u16, data: &[u8]) {
    let logical = u32::from(wire_addr);
    if logical < addr::VFO_INFO_ADDR && write_channel(app, logical, data) {
        send_response(frame::CMD_WRITE, wire_addr, &[]);
    } else if logical == addr::RADIO_IMFOS_ADDR
        && data.len() == flash_map::SETTINGS_BYTES
    {
        let raw: &[u8; flash_map::SETTINGS_BYTES] = data.try_into().unwrap();
        app.storage_mut().save_settings(&flash_map::Settings::from_bytes(raw));
        send_response(frame::CMD_WRITE, wire_addr, &[]);
    } else {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
    }
}

fn write_channel(app: &mut App<'_>, logical: u32, data: &[u8]) -> bool {
    if !logical.is_multiple_of(addr::CHAN_SIZE)
        || data.len() != addr::CHAN_SIZE as usize
    {
        return false;
    }
    let num = (logical / addr::CHAN_SIZE) as u16;
    if num > MAX_CHANNEL_NUM {
        return false;
    }
    app.storage_mut().write_channel_bytes(num, data);
    true
}

fn is_contact_addr(logical: u32) -> bool {
    let base = addr::DTMF_CODE_ADDR;
    let end = base + addr::CONTACT_COUNT as u32 * addr::CONTACT_SIZE;
    logical >= base
        && logical < end
        && (logical - base).is_multiple_of(addr::CONTACT_SIZE)
}

fn contact_index(logical: u32) -> u8 {
    ((logical - addr::DTMF_CODE_ADDR) / addr::CONTACT_SIZE) as u8
}

/// Where an image command reads or writes: the boot logo or the background.
fn image_base(cmd: u8) -> u32 {
    if cmd == frame::CMD_READ_LOGO || cmd == frame::CMD_WRITE_LOGO {
        addr::BOOT_LOGO_ADDR
    } else {
        addr::BACKGROUND_ADDR
    }
}

fn handle_read_logo(app: &mut App<'_>, wire_addr: u16, cmd: u8) {
    #[cfg(feature = "ar5rm-nrf")]
    if cmd == frame::CMD_READ_BG && wire_addr == u16::MAX {
        let mut marker = [0u8; 4];
        app.storage_mut().read_raw(addr::BACKGROUND_MARKER_ADDR, &mut marker);
        send_response(cmd, wire_addr, &[u8::from(marker == *b"BG01")]);
        return;
    }
    let offset = wire_addr as u32;
    if !offset.is_multiple_of(frame::BOOT_LOGO_CHUNK)
        || offset + frame::BOOT_LOGO_CHUNK > flash_map::BOOT_LOGO_SIZE as u32
    {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
        return;
    }
    let mut buf = [0u8; frame::BOOT_LOGO_CHUNK as usize];
    app.storage_mut().read_raw(image_base(cmd) + offset, &mut buf);
    send_response(cmd, wire_addr, &buf);
}

fn handle_write_logo(app: &mut App<'_>, wire_addr: u16, data: &[u8], cmd: u8) {
    #[cfg(feature = "ar5rm-nrf")]
    if cmd == frame::CMD_WRITE_BG && wire_addr == u16::MAX && data.is_empty() {
        app.storage_mut().norflash.erase_sector(addr::BACKGROUND_MARKER_ADDR);
        send_response(cmd, wire_addr, &[]);
        return;
    }
    #[cfg(feature = "ar5rm-nrf")]
    if cmd == frame::CMD_WRITE_BG && wire_addr == u16::MAX - 1 && data.is_empty() {
        app.storage_mut().write_raw(addr::BACKGROUND_MARKER_ADDR, b"BG01");
        send_response(cmd, wire_addr, &[]);
        return;
    }
    let base = image_base(cmd);
    let offset = wire_addr as u32;
    let ok = offset.is_multiple_of(frame::BOOT_LOGO_CHUNK)
        && data.len() == frame::BOOT_LOGO_CHUNK as usize
        && offset + frame::BOOT_LOGO_CHUNK <= flash_map::BOOT_LOGO_SIZE as u32;
    if !ok {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
        return;
    }
    if offset == 0 {
        let storage = app.storage_mut();
        #[cfg(feature = "ar5rm-nrf")]
        if cmd == frame::CMD_WRITE_BG {
            storage.norflash.erase_sector(addr::BACKGROUND_MARKER_ADDR);
        }
        let mut sector = base;
        let end = base + flash_map::BOOT_LOGO_SIZE as u32;
        while sector < end {
            storage.norflash.erase_sector(sector);
            sector += SECTOR_SIZE;
        }
    }
    app.storage_mut().write_raw(base + offset, data);
    send_response(cmd, wire_addr, &[]);
}

fn handle_app_erase(app: &mut App<'_>, wire_addr: u16) -> Option<u8> {
    let slot = wire_addr as u8;
    if slot >= addr::OVERLAY_SLOT_COUNT {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
        return None;
    }
    let storage = app.storage_mut();
    let base = addr::overlay_slot_addr(slot);
    let mut sector = base;
    let end = base + addr::OVERLAY_SLOT_SIZE;
    while sector < end {
        storage.norflash.erase_sector(sector);
        sector += SECTOR_SIZE;
    }
    send_response(frame::CMD_APP_ERASE, wire_addr, &[]);
    Some(slot)
}

fn handle_app_write(
    app: &mut App<'_>,
    slot: Option<u8>,
    wire_addr: u16,
    data: &[u8],
) {
    let Some(slot) = slot else {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
        return;
    };
    let offset = wire_addr as u32;
    let ok = offset.is_multiple_of(frame::APP_CHUNK)
        && data.len() == frame::APP_CHUNK as usize
        && offset + frame::APP_CHUNK <= addr::OVERLAY_SLOT_SIZE;
    if !ok {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
        return;
    }
    app.storage_mut()
        .write_raw(addr::overlay_slot_addr(slot) + offset, data);
    send_response(frame::CMD_APP_WRITE, wire_addr, &[]);
}

fn handle_sat_write(app: &mut App<'_>, wire_addr: u16, data: &[u8]) {
    let offset = wire_addr as u32;
    let ok = offset.is_multiple_of(frame::SAT_CHUNK)
        && data.len() == frame::SAT_CHUNK as usize
        && offset + frame::SAT_CHUNK <= SAT_TABLE_SIZE;
    if !ok {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
        return;
    }
    if offset == 0 {
        let storage = app.storage_mut();
        let mut sector = SAT_TABLE_BASE;
        let end = SAT_TABLE_BASE + SAT_TABLE_SIZE;
        while sector < end {
            storage.norflash.erase_sector(sector);
            sector += SECTOR_SIZE;
        }
    }
    app.storage_mut().write_raw(SAT_TABLE_BASE + offset, data);
    send_response(frame::CMD_SAT_WRITE, wire_addr, &[]);
}

const FLASH_CHIP_SIZE: u32 = 0x20_0000;
const FLASH_RAW_CHUNK: u32 = 64;

fn handle_read_flash_raw(app: &mut App<'_>, wire_addr: u16) {
    let addr = wire_addr as u32 * FLASH_RAW_CHUNK;
    if addr + FLASH_RAW_CHUNK > FLASH_CHIP_SIZE {
        send_error(wire_addr, frame::ERR_BAD_ADDR);
        return;
    }
    let mut buf = [0u8; FLASH_RAW_CHUNK as usize];
    app.storage_mut().norflash.read_physical(addr, &mut buf);
    send_response(frame::CMD_READ_FLASH_RAW, wire_addr, &buf);
}

fn send_response(cmd: u8, addr: u16, data: &[u8]) {
    use hal::wear_leveled::{crc16_update, crc16_xmodem};

    let [addr_hi, addr_lo] = addr.to_be_bytes();
    let [len_hi, len_lo] = (data.len() as u16).to_be_bytes();
    let header = [cmd, addr_hi, addr_lo, len_hi, len_lo];
    let crc = crc16_update(crc16_xmodem(&header), data);
    hal::uart::write_byte(frame::HEADER);
    hal::uart::write_bytes(&header);
    hal::uart::write_bytes(data);
    hal::uart::write_bytes(&crc.to_be_bytes());
}

fn send_error(addr: u16, code: u8) {
    send_response(frame::CMD_ERROR, addr, &[code]);
}
