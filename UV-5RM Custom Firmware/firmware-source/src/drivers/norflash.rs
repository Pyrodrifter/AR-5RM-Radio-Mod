use crate::board;
use at32f421_pac as pac;

const CMD_WRITE_ENABLE: u8 = 0x06;
const CMD_READ_STATUS1: u8 = 0x05;
const CMD_READ_DATA: u8 = 0x03;
const CMD_PAGE_PROGRAM: u8 = 0x02;
const CMD_ERASE_SECTOR_4K: u8 = 0x20;
#[allow(dead_code)]
const CMD_ERASE_CHIP: u8 = 0xC7;

const STATUS_BUSY_BIT: u8 = 0x01;

pub const PAGE_SIZE: usize = 256;
pub const SECTOR_SIZE: u32 = 4096;

const BUSY_POLL_LIMIT: u32 = 1_000_000;

pub const CAL_BLOCK_ADDR: u32 = 0xF210;
pub const CAL_BLOCK_LEN: usize = 11;

#[cfg(feature = "ar5rm-nrf")]
pub(crate) const USER_BASE: u32 = 0x1A0000;
#[cfg(feature = "ar5rm-nrf")]
pub(crate) const USER_END: u32 = USER_BASE + 0x1A000;
/// End of firmware-private NOR that apps may not write: the user region,
/// the logo and the home-screen background after it.
#[cfg(feature = "ar5rm-nrf")]
pub(crate) const PROTECTED_END: u32 =
    USER_BASE + 0x10000 + 2 * crate::flash_map::BOOT_LOGO_SIZE as u32 + SECTOR_SIZE;
#[cfg(feature = "ar5rm-nrf")]
const USER_MARKER: u32 = USER_BASE + crate::flash_map::addr::FIRST_BOOT_MARKER_ADDR;

pub struct NorFlash<'a> {
    gpioa: &'a pac::Gpioa,
    spi1: &'a pac::Spi1,
    #[cfg(feature = "ar5rm-nrf")]
    overlay_writable: bool,
    #[cfg(feature = "ar5rm-nrf")]
    user_ready: bool,
}

impl<'a> NorFlash<'a> {
    pub fn new(gpioa: &'a pac::Gpioa, spi1: &'a pac::Spi1) -> Self {
        let flash = NorFlash {
            gpioa,
            spi1,
            #[cfg(feature = "ar5rm-nrf")]
            overlay_writable: false,
            #[cfg(feature = "ar5rm-nrf")]
            user_ready: false,
        };
        #[cfg(feature = "ar5rm-nrf")]
        {
            let mut flash = flash;
            // A smaller chip can wrap high addresses into stock data.
            board::set_norflash_cs(gpioa, false);
            flash.transfer_byte(0x9F); // JEDEC ID: manufacturer, type, density
            let vendor = flash.transfer_byte(0);
            flash.transfer_byte(0);
            let density = flash.transfer_byte(0);
            board::set_norflash_cs(gpioa, true);
            flash.overlay_writable = vendor != 0 && vendor != 0xFF
                && (0x15..=0x18).contains(&density);
            if flash.overlay_writable {
                flash.user_ready = flash.prepare_user_data();
            }
            flash
        }
        #[cfg(not(feature = "ar5rm-nrf"))]
        flash
    }

    fn transfer_byte(&mut self, byte: u8) -> u8 {
        while self.spi1.sts().read().tdbe().bit_is_clear() {}
        self.spi1
            .dt()
            .write(|w| unsafe { w.dt().bits(byte as u16) });
        while self.spi1.sts().read().rdbf().bit_is_clear() {}
        self.spi1.dt().read().dt().bits() as u8
    }

    fn send_addr24(&mut self, addr: u32) {
        self.transfer_byte((addr >> 16) as u8);
        self.transfer_byte((addr >> 8) as u8);
        self.transfer_byte(addr as u8);
    }

    #[cfg_attr(feature = "ar5rm-nrf", inline(never))]
    pub fn read_bytes(&mut self, addr: u32, buf: &mut [u8]) {
        #[cfg(feature = "ar5rm-nrf")]
        let addr = self.map_user_addr(addr);
        self.read_physical(addr, buf);
    }

    #[cfg_attr(feature = "ar5rm-nrf", inline(never))]
    pub(crate) fn read_physical(&mut self, addr: u32, buf: &mut [u8]) {
        board::set_norflash_cs(self.gpioa, false);
        self.transfer_byte(CMD_READ_DATA);
        self.send_addr24(addr);
        for b in buf.iter_mut() {
            *b = self.transfer_byte(0x00);
        }
        board::set_norflash_cs(self.gpioa, true);
    }

    pub fn read_status(&mut self) -> u8 {
        board::set_norflash_cs(self.gpioa, false);
        self.transfer_byte(CMD_READ_STATUS1);
        let status = self.transfer_byte(0x00);
        board::set_norflash_cs(self.gpioa, true);
        status
    }

    fn wait_until_ready(&mut self) {
        for _ in 0..BUSY_POLL_LIMIT {
            if self.read_status() & STATUS_BUSY_BIT == 0 {
                return;
            }
        }
    }

    fn write_enable(&mut self) {
        board::set_norflash_cs(self.gpioa, false);
        self.transfer_byte(CMD_WRITE_ENABLE);
        board::set_norflash_cs(self.gpioa, true);
    }

    fn write_page(&mut self, addr: u32, data: &[u8]) {
        self.write_enable();
        board::set_norflash_cs(self.gpioa, false);
        self.transfer_byte(CMD_PAGE_PROGRAM);
        self.send_addr24(addr);
        for &b in data {
            self.transfer_byte(b);
        }
        board::set_norflash_cs(self.gpioa, true);
        self.wait_until_ready();
    }

    #[cfg_attr(feature = "ar5rm-nrf", inline(never))]
    pub fn write_bytes(&mut self, addr: u32, data: &[u8]) {
        #[cfg(feature = "ar5rm-nrf")]
        let addr = self.map_user_addr(addr);
        #[cfg(feature = "ar5rm-nrf")]
        if !self.overlay_writable || addr < crate::flash_map::addr::OVERLAY_APP_ADDR
            || addr.saturating_add(data.len() as u32) > 0x200000
        {
            return;
        }
        let mut addr = addr;
        let mut remaining = data;
        while !remaining.is_empty() {
            let offset_in_page = (addr % PAGE_SIZE as u32) as usize;
            let chunk_len = (PAGE_SIZE - offset_in_page).min(remaining.len());
            let (chunk, rest) = remaining.split_at(chunk_len);
            self.write_page(addr, chunk);
            addr += chunk_len as u32;
            remaining = rest;
        }
    }

    #[cfg_attr(feature = "ar5rm-nrf", inline(never))]
    pub fn erase_sector(&mut self, addr: u32) {
        #[cfg(feature = "ar5rm-nrf")]
        let addr = self.map_user_addr(addr);
        #[cfg(feature = "ar5rm-nrf")]
        if !self.overlay_writable
            || !(crate::flash_map::addr::OVERLAY_APP_ADDR..0x200000).contains(&addr)
        {
            return;
        }
        self.write_enable();
        board::set_norflash_cs(self.gpioa, false);
        self.transfer_byte(CMD_ERASE_SECTOR_4K);
        self.send_addr24(addr);
        board::set_norflash_cs(self.gpioa, true);
        self.wait_until_ready();
    }

    #[allow(dead_code)]
    #[cfg(not(feature = "ar5rm-nrf"))]
    pub fn erase_chip(&mut self) {
        self.write_enable();
        board::set_norflash_cs(self.gpioa, false);
        self.transfer_byte(CMD_ERASE_CHIP);
        board::set_norflash_cs(self.gpioa, true);
        self.wait_until_ready();
    }

    #[cfg(feature = "ar5rm-nrf")]
    pub(crate) fn user_data_ready(&self) -> bool {
        self.user_ready
    }

    #[cfg(feature = "ar5rm-nrf")]
    #[inline(never)]
    fn map_user_addr(&self, addr: u32) -> u32 {
        use crate::flash_map::{addr::{BACKGROUND_ADDR, BACKGROUND_MARKER_ADDR, BOOT_LOGO_ADDR}, BOOT_LOGO_SIZE};
        if !self.user_ready {
            return addr;
        }
        if addr < 0xF000 {
            return USER_BASE + addr;
        }
        if (BOOT_LOGO_ADDR..BOOT_LOGO_ADDR + BOOT_LOGO_SIZE as u32).contains(&addr) {
            return USER_BASE + 0x10000 + addr - BOOT_LOGO_ADDR;
        }
        // The home-screen background sits right after the logo: physical
        // 0x1BA000..0x1C4000.
        if (BACKGROUND_ADDR..BACKGROUND_ADDR + BOOT_LOGO_SIZE as u32).contains(&addr) {
            return USER_BASE + 0x10000 + BOOT_LOGO_SIZE as u32 + addr - BACKGROUND_ADDR;
        }
        if (BACKGROUND_MARKER_ADDR..BACKGROUND_MARKER_ADDR + SECTOR_SIZE).contains(&addr) {
            return USER_BASE + 0x10000 + 2 * BOOT_LOGO_SIZE as u32 + addr - BACKGROUND_MARKER_ADDR;
        }
        addr
    }

    #[cfg(feature = "ar5rm-nrf")]
    #[inline(never)]
    fn prepare_user_data(&mut self) -> bool {
        let mut marker = [0u8; 8];
        self.read_physical(USER_MARKER, &mut marker);
        if &marker == b"NRF1OKAY" {
            return true;
        }
        let mut page = [0u8; PAGE_SIZE];
        if marker == [0xFF; 8] {
            for addr in (USER_BASE..USER_END).step_by(PAGE_SIZE) {
                self.read_physical(addr, &mut page);
                if page.iter().any(|&byte| byte != 0xFF) {
                    return false;
                }
            }
            self.write_bytes(USER_MARKER, b"NRF1");
            self.read_physical(USER_MARKER, &mut marker);
        }
        if &marker[..4] != b"NRF1" {
            return false;
        }

        // Copy the factory channels and contacts. The other sectors use
        // this firmware's own format and start blank. A power loss before
        // OKAY lets us safely retry this copy from the untouched originals.
        for sector in (0..0x8000).step_by(SECTOR_SIZE as usize).chain([0xA000]) {
            self.erase_sector(USER_BASE + sector);
            for addr in (sector..sector + SECTOR_SIZE).step_by(PAGE_SIZE) {
                self.read_physical(addr, &mut page);
                if page.iter().any(|&byte| byte != 0xFF) {
                    self.write_bytes(USER_BASE + addr, &page);
                }
                let mut check = [0u8; PAGE_SIZE];
                self.read_physical(USER_BASE + addr, &mut check);
                if page != check {
                    return false;
                }
            }
        }
        self.write_bytes(USER_MARKER + 4, b"OKAY");
        self.read_physical(USER_MARKER, &mut marker);
        &marker == b"NRF1OKAY"
    }
}
