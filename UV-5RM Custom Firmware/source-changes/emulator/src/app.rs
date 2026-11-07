//! A stand-in for the firmware's `app::App`: just the read-only methods that
//! `ui/standby.rs` calls, with the same signatures, so the radio's real home
//! screen code draws in the emulator unchanged. The key handling here is a
//! simplified copy of `app/keys.rs` for the main screen; it is not the radio's.

use crate::flash_map::{self, addr, Channel, Settings};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Power {
    Low,
    Mid,
    High,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modulation {
    Fm,
    Am,
    Usb,
    Cw,
    Cwf,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubAudio {
    None,
    Ctcss(u16),
    Dcs { code: u16, inverted: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChannelDisplayMode {
    Frequency,
    Name,
    NameFreq,
}

#[derive(Clone, Copy)]
pub struct Side {
    channel_mode: bool,
    channel: u16,
    name: [u8; 12],
    ch_rx_hz: u32,
    ch_tx_hz: u32,
    ch_tx_blank: bool,
    vfo_hz: u32,
    offset_hz: u32,
    dir: u8,
    modulation: Modulation,
    power: Power,
    wide: bool,
    sub_rx: SubAudio,
    sub_tx: SubAudio,
    reversed: bool,
}

impl Side {
    fn vfo(hz: u32) -> Side {
        Side {
            channel_mode: false,
            channel: 0,
            name: [0; 12],
            ch_rx_hz: hz,
            ch_tx_hz: hz,
            ch_tx_blank: false,
            vfo_hz: hz,
            offset_hz: 600_000,
            dir: 0,
            modulation: Modulation::Fm,
            power: Power::Low,
            wide: true,
            sub_rx: SubAudio::None,
            sub_tx: SubAudio::None,
            reversed: false,
        }
    }
}

/// What a home-screen key asks the host to do.
pub enum HomeAction {
    None,
    OpenMenu,
    OpenApp(&'static str),
}

/// Reads the emulator's SPI flash, as the firmware's `Storage` does.
pub struct Storage;

impl Storage {
    pub fn read_raw(&mut self, addr: u32, buf: &mut [u8]) {
        crate::nor_read_into(addr, buf);
    }
}

pub struct App {
    storage: Storage,
    sides: [Side; 2],
    master: usize,
    input: [u8; 6],
    input_len: usize,
    key_lock: bool,
    transmitting: bool,
    tx_prohibited: bool,
    tx_since: Instant,
    monitor: bool,
    rssi_raw: u16,
    open: bool,
    mic: u8,
    settings: Settings,
    started: Instant,
    notice_until: Option<Instant>,
}

pub const KIND_SINGLE: u8 = 2;
pub const KIND_LONG: u8 = 3;
pub const KIND_REPEAT: u8 = 4;

fn sub_from_code(code: u16) -> SubAudio {
    match flash_map::SubaudioCode::decode(code) {
        flash_map::SubaudioCode::None => SubAudio::None,
        flash_map::SubaudioCode::Ctcss(t) => SubAudio::Ctcss(t),
        flash_map::SubaudioCode::DcsNormal(i) => dcs(i, false),
        flash_map::SubaudioCode::DcsInverted(i) => dcs(i - 105, true),
    }
}

fn dcs(idx: u16, inverted: bool) -> SubAudio {
    match crate::tones::DCS_TABLE.get((idx as usize).wrapping_sub(1)) {
        Some(&code) => SubAudio::Dcs { code, inverted },
        None => SubAudio::None,
    }
}

fn read_channel(nor: &[u8], num: u16) -> Option<Channel> {
    let a = (addr::CHAN_ADDR + num as u32 * addr::CHAN_SIZE) as usize;
    let rec: &[u8; 32] = nor.get(a..a + 32)?.try_into().ok()?;
    (!rec.iter().all(|&b| b == 0xFF)).then(|| Channel::from_bytes(rec))
}

impl App {
    pub fn new(settings: Settings, master_hz: u32) -> App {
        App {
            storage: Storage,
            sides: [Side::vfo(master_hz), Side::vfo(446_006_250)],
            master: 0,
            input: [0; 6],
            input_len: 0,
            key_lock: false,
            transmitting: false,
            tx_prohibited: false,
            tx_since: Instant::now(),
            monitor: false,
            rssi_raw: 45,
            open: false,
            mic: 0,
            settings,
            started: Instant::now(),
            notice_until: None,
        }
    }

    pub fn master_freq_hz(&self) -> u32 {
        self.side_freq_hz(self.master)
    }

    fn load_channel(&mut self, side: usize, num: u16, nor: &[u8]) -> bool {
        let Some(ch) = read_channel(nor, num) else { return false };
        let s = &mut self.sides[side];
        s.channel = num;
        s.ch_rx_hz = ch.rx_freq_deci_hz() * 10;
        s.ch_tx_blank = ch.tx_is_blank();
        s.ch_tx_hz = if s.ch_tx_blank { s.ch_rx_hz } else { ch.tx_freq_deci_hz() * 10 };
        s.name = ch.name;
        s.power = match ch.tx_power {
            1 => Power::Low,
            2 => Power::Mid,
            _ => Power::High,
        };
        s.wide = !ch.wide_narrow();
        s.sub_rx = sub_from_code(ch.rx_dcs_cts_num);
        s.sub_tx = sub_from_code(ch.tx_dcs_cts_num);
        s.modulation = Modulation::Fm;
        true
    }

    /// The next stored channel after (or before) `from`, wrapping.
    fn next_channel(nor: &[u8], from: u16, up: bool) -> Option<u16> {
        (1..=1000u16)
            .map(|d| if up { (from + d) % 1000 } else { (from + 1000 - d % 1000) % 1000 })
            .find(|&n| read_channel(nor, n).is_some())
    }

    fn notice(&mut self) {
        self.notice_until = Some(Instant::now() + std::time::Duration::from_millis(1500));
    }

    /// Per frame: settings, PTT and the fake receiver.
    pub fn tick(&mut self, settings: Settings, ptt: bool, signal: bool, noise: u32) {
        self.settings = settings;
        let tx_ok = !settings.tx_forbid;
        if ptt && !self.transmitting && !self.tx_prohibited {
            if tx_ok {
                self.transmitting = true;
                self.tx_since = Instant::now();
            } else {
                self.tx_prohibited = true;
            }
        }
        if !ptt {
            self.transmitting = false;
            self.tx_prohibited = false;
        }
        self.mic = if self.transmitting { 40 + (noise % 30) as u8 } else { 0 };
        self.open = signal || self.monitor;
        // raw - 170 = dBm: about S9+10 with a signal, the noise floor without.
        self.rssi_raw = if signal { 110 + (noise % 6) as u16 } else { 42 + (noise % 5) as u16 };
        if self.notice_until.is_some_and(|t| Instant::now() > t) {
            self.notice_until = None;
        }
    }

    pub fn key(&mut self, id: u8, kind: u8, nor: &[u8]) -> HomeAction {
        use crate::{K_AB, K_BAND, K_DOWN, K_EXIT, K_MENU, K_POUND, K_SIDE1, K_SIDE2, K_STAR, K_UP, K_VM};
        if kind == KIND_LONG && id == K_STAR {
            self.key_lock = !self.key_lock;
            return HomeAction::None;
        }
        if self.key_lock && !matches!(id, K_SIDE1 | K_SIDE2) {
            return HomeAction::None;
        }
        let m = self.master;
        if kind == KIND_REPEAT || kind == KIND_SINGLE {
            match id {
                K_UP | K_DOWN => {
                    let up = id == K_UP;
                    let s = &mut self.sides[m];
                    if s.channel_mode {
                        if let Some(n) = Self::next_channel(nor, s.channel, up) {
                            self.load_channel(m, n, nor);
                        }
                    } else {
                        let step = 12_500;
                        s.vfo_hz = if up { s.vfo_hz + step } else { s.vfo_hz.saturating_sub(step) };
                    }
                    return HomeAction::None;
                }
                _ => {}
            }
        }
        if kind == KIND_LONG {
            let f = match id {
                K_SIDE1 => self.settings.side1_long,
                K_SIDE2 => self.settings.side2_long,
                K_BAND => self.settings.band_long,
                _ => 0,
            };
            return self.key_function(f);
        }
        if kind != KIND_SINGLE {
            return HomeAction::None;
        }
        match id {
            0..=9 => {
                let s = self.sides[m];
                let cap = if s.channel_mode { 3 } else { 6 };
                if self.input_len < cap {
                    self.input[self.input_len] = id;
                    self.input_len += 1;
                }
                if self.input_len == cap {
                    self.commit_input(nor);
                }
            }
            K_MENU => {
                if self.input_len > 0 && self.sides[m].channel_mode {
                    self.commit_input(nor);
                } else {
                    return HomeAction::OpenMenu;
                }
            }
            K_EXIT => self.input_len = 0,
            K_VM => {
                self.input_len = 0;
                let s = self.sides[m];
                if s.channel_mode {
                    self.sides[m].channel_mode = false;
                } else if self.load_channel(m, s.channel, nor)
                    || Self::next_channel(nor, s.channel, true).is_some_and(|n| self.load_channel(m, n, nor))
                {
                    self.sides[m].channel_mode = true;
                } else {
                    self.notice();
                }
            }
            K_AB => {
                self.input_len = 0;
                self.master ^= 1;
            }
            K_SIDE1 => return self.key_function(self.settings.side1_short),
            K_SIDE2 => return self.key_function(self.settings.side2_short),
            K_BAND | K_POUND | K_STAR => {}
            _ => {}
        }
        HomeAction::None
    }

    fn commit_input(&mut self, nor: &[u8]) {
        let m = self.master;
        let digits = &self.input[..self.input_len];
        let n = digits.iter().fold(0u32, |a, &d| a * 10 + d as u32);
        if self.sides[m].channel_mode {
            if n >= 1 && !self.load_channel(m, (n - 1) as u16, nor) {
                self.notice();
            }
        } else if digits.len() == 6 {
            self.sides[m].vfo_hz = n * 1000;
        }
        self.input_len = 0;
    }

    /// The side-key functions in app/keyfn.rs that show on this screen.
    fn key_function(&mut self, f: u8) -> HomeAction {
        let s = &mut self.sides[self.master];
        match f {
            1 => s.wide = !s.wide,
            2 => self.monitor = !self.monitor,
            3 => {
                s.modulation = match s.modulation {
                    Modulation::Fm => Modulation::Am,
                    Modulation::Am => Modulation::Usb,
                    _ => Modulation::Fm,
                }
            }
            5 => return HomeAction::OpenApp("FM"),
            7 => {
                s.power = match s.power {
                    Power::High => Power::Mid,
                    Power::Mid => Power::Low,
                    Power::Low => Power::High,
                }
            }
            10 => s.reversed = !s.reversed,
            _ => {}
        }
        HomeAction::None
    }

    // ---- what ui/standby.rs reads, mirroring app/mod.rs ----

    pub fn storage_mut(&mut self) -> &mut Storage {
        &mut self.storage
    }

    pub fn audio_open(&self) -> bool {
        self.open
    }
    pub fn battery_bars(&self) -> u8 {
        4
    }
    pub fn battery_voltage_cv(&self) -> u16 {
        790
    }
    pub fn chan_mode_short(&self) -> &'static str {
        "ALL"
    }
    pub fn channel_display_mode(&self) -> ChannelDisplayMode {
        match self.settings.channel_display_mode {
            1 => ChannelDisplayMode::Name,
            2 => ChannelDisplayMode::NameFreq,
            _ => ChannelDisplayMode::Frequency,
        }
    }
    pub fn channel_input_len(&self) -> usize {
        if self.sides[self.master].channel_mode { self.input_len } else { 0 }
    }
    pub fn dtmf_dial_active(&self) -> bool {
        false
    }
    pub fn dtmf_dial_capacity(&self) -> usize {
        0
    }
    pub fn dtmf_dial_digit(&self, _idx: usize) -> u8 {
        0
    }
    pub fn dtmf_dial_len(&self) -> usize {
        0
    }
    pub fn dual_standby_enabled(&self) -> bool {
        self.settings.dual_standby
    }
    pub fn freq_input_digit(&self, idx: usize) -> u8 {
        self.input.get(idx).copied().unwrap_or(0)
    }
    pub fn freq_input_len(&self) -> usize {
        if self.sides[self.master].channel_mode { 0 } else { self.input_len }
    }
    pub fn is_key_locked(&self) -> bool {
        self.key_lock
    }
    pub fn is_monitor(&self) -> bool {
        self.monitor
    }
    pub fn is_transmitting(&self) -> bool {
        self.transmitting
    }
    pub fn last_signal_peak(&self) -> Option<(usize, i32)> {
        None
    }
    pub fn last_signal_side(&self) -> Option<usize> {
        self.open.then_some(self.master)
    }
    pub fn master_index(&self) -> usize {
        self.master
    }
    pub fn mic_level(&self) -> u8 {
        self.mic
    }
    pub fn no_channels_notice(&self) -> bool {
        self.notice_until.is_some()
    }
    pub fn power_save_active(&self) -> bool {
        false
    }
    pub fn rssi_dbm(&self) -> i32 {
        self.rssi_raw as i32 - 170
    }
    pub fn rssi_open(&self) -> bool {
        self.open
    }
    pub fn rx_blink_on(&self) -> bool {
        (self.started.elapsed().as_millis() / 400) % 2 == 0
    }
    pub fn s_meter_level(&self) -> u8 {
        let pos = (-self.rssi_dbm()).clamp(53, 141);
        if pos >= 93 {
            map(pos, 141, 93, 1, 9).clamp(1, 9) as u8
        } else {
            (9 + map(pos, 93, 53, 0, 4).clamp(0, 4)) as u8
        }
    }
    pub fn s_meter_s_number(&self) -> u8 {
        map((-self.rssi_dbm()).clamp(53, 141), 141, 93, 1, 9).clamp(1, 9) as u8
    }
    pub fn s_meter_over_s9_dbm(&self) -> i32 {
        (93 - (-self.rssi_dbm()).clamp(53, 141)).max(0)
    }
    pub fn side_channel_num(&self, index: usize) -> u16 {
        self.sides[index].channel
    }
    pub fn side_freq_dir(&self, index: usize) -> u8 {
        let s = &self.sides[index];
        if s.channel_mode {
            match s.ch_tx_hz.cmp(&s.ch_rx_hz) {
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => 2,
                _ => 0,
            }
        } else {
            s.dir
        }
    }
    pub fn side_freq_hz(&self, index: usize) -> u32 {
        let s = &self.sides[index];
        if s.channel_mode { s.ch_rx_hz } else { s.vfo_hz }
    }
    pub fn side_is_channel_mode(&self, index: usize) -> bool {
        self.sides[index].channel_mode
    }
    pub fn side_modulation(&self, index: usize) -> Modulation {
        self.sides[index].modulation
    }
    pub fn side_name_str(&self, index: usize) -> &str {
        let raw = &self.sides[index].name;
        let end = raw.iter().position(|&b| b == 0 || b == 0xFF).unwrap_or(raw.len());
        std::str::from_utf8(&raw[..end]).unwrap_or("")
    }
    pub fn side_offset_hz(&self, index: usize) -> u32 {
        let s = &self.sides[index];
        if s.channel_mode { s.ch_tx_hz.abs_diff(s.ch_rx_hz) } else { s.offset_hz }
    }
    pub fn side_power(&self, index: usize) -> Power {
        self.sides[index].power
    }
    pub fn side_reversed(&self, index: usize) -> bool {
        self.sides[index].reversed
    }
    pub fn side_subaudio_rx(&self, index: usize) -> SubAudio {
        self.sides[index].sub_rx
    }
    pub fn side_subaudio_tx(&self, index: usize) -> SubAudio {
        self.sides[index].sub_tx
    }
    pub fn side_tx_freq_hz(&self, index: usize) -> u32 {
        let s = &self.sides[index];
        if s.channel_mode {
            return s.ch_tx_hz;
        }
        match s.dir {
            1 => s.vfo_hz + s.offset_hz,
            2 => s.vfo_hz.saturating_sub(s.offset_hz),
            _ => s.vfo_hz,
        }
    }
    pub fn side_wide_band(&self, index: usize) -> bool {
        self.sides[index].wide
    }
    pub fn tx_elapsed_seconds(&self) -> u32 {
        if self.transmitting { self.tx_since.elapsed().as_secs() as u32 } else { 0 }
    }
    pub fn tx_prohibited(&self) -> bool {
        self.tx_prohibited
    }
    pub fn vox_enabled(&self) -> bool {
        self.settings.vox_switch
    }
    pub fn watching_index(&self) -> usize {
        self.master
    }
}

/// Same as app/mod.rs.
pub fn s_meter_level_of(dbm: i32) -> u8 {
    let pos = (-dbm).clamp(53, 141);
    if pos >= 93 {
        map(pos, 141, 93, 1, 9).clamp(1, 9) as u8
    } else {
        (9 + map(pos, 93, 53, 0, 4).clamp(0, 4)) as u8
    }
}

fn map(x: i32, in_min: i32, in_max: i32, out_min: i32, out_max: i32) -> i32 {
    (x - in_min) * (out_max - out_min) / (in_max - in_min) + out_min
}
