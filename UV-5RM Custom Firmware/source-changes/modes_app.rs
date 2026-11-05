use super::side_ops;
use super::{
    digit_value, scan, App, ChVfoMode, Mode, MAX_CHANNEL_NUM,
    NO_CHANNELS_HOLD_TICKS, STEP_LIST_DECI_HZ,
};
use crate::device::keypad::{KeyEvent, KeyEventKind, KeyId};
use crate::device::radio::{FreqRanges, SubAudio};
use crate::flash_map::Settings;
use cortex_m::peripheral::SYST;

pub(crate) struct ChanMode {
    pub name: &'static str,
    pub short: &'static str,
    /// Inclusive channel numbers as printed on the reference sheet (from 1).
    ranges: &'static [(u16, u16)],
}

const ALL: &[(u16, u16)] = &[(1, MAX_CHANNEL_NUM + 1)];

pub(crate) const MODES: [ChanMode; 13] = [
    ChanMode { name: "ALL", short: "ALL", ranges: ALL },
    ChanMode { name: "WEATHER", short: "WX", ranges: &[(1, 7)] },
    ChanMode { name: "POLICE FIRE EMS", short: "PD/FD", ranges: &[(14, 105)] },
    ChanMode { name: "FED & GOV", short: "FED", ranges: &[(106, 146)] },
    ChanMode { name: "UTILITIES", short: "UTIL", ranges: &[(147, 157)] },
    ChanMode { name: "RAILROAD", short: "RAIL", ranges: &[(158, 167)] },
    ChanMode { name: "AIR TRAFFIC", short: "AIR", ranges: &[(168, 233)] },
    ChanMode { name: "REPEATERS", short: "RPT", ranges: &[(234, 304), (327, 344)] },
    ChanMode { name: "FRS", short: "FRS", ranges: &[(305, 326)] },
    ChanMode { name: "GMRS", short: "GMRS", ranges: &[(305, 344)] },
    ChanMode { name: "MURS", short: "MURS", ranges: &[(345, 349)] },
    ChanMode { name: "FRS GMRS MURS", short: "F/G/M", ranges: &[(305, 349)] },
    // Channel list stays open here; the scan itself sweeps the typed range.
    ChanMode { name: "CUSTOM RANGE", short: "RANGE", ranges: ALL },
];

const CUSTOM: u8 = MODES.len() as u8 - 1;

const DEFAULT_RANGE_HZ: (u32, u32) = (144_000_000, 148_000_000);

/// `boot_text_line2` isn't shown in logo boot mode, so it carries the mode
/// and range across power cycles without changing the settings record size
/// that the settings app and CPS tools depend on.
const STORE_MAGIC: u8 = b'M';

pub(super) fn restore(settings: &Settings) -> (u8, u32, u32) {
    let b = &settings.boot_text_line2;
    if b[0] != STORE_MAGIC || b[1] > CUSTOM {
        return (0, DEFAULT_RANGE_HZ.0, DEFAULT_RANGE_HZ.1);
    }
    let lo = u32::from_le_bytes([b[2], b[3], b[4], b[5]]);
    let hi = u32::from_le_bytes([b[6], b[7], b[8], b[9]]);
    if range_valid(lo, hi) {
        (b[1], lo, hi)
    } else {
        (b[1], DEFAULT_RANGE_HZ.0, DEFAULT_RANGE_HZ.1)
    }
}

fn persist(app: &mut App) {
    let b = &mut app.settings.boot_text_line2;
    b[0] = STORE_MAGIC;
    b[1] = app.chan_mode;
    b[2..6].copy_from_slice(&app.range_lo_hz.to_le_bytes());
    b[6..10].copy_from_slice(&app.range_hi_hz.to_le_bytes());
    app.save_settings();
}

fn range_valid(lo: u32, hi: u32) -> bool {
    let hw = FreqRanges::hardware();
    lo < hi && hw.allows(lo) && hw.allows(hi)
}

/// `num` is a flash slot, counting from 0.
pub(super) fn contains(mode: u8, num: u16) -> bool {
    let printed = num + 1;
    MODES[mode as usize]
        .ranges
        .iter()
        .any(|&(a, b)| (a..=b).contains(&printed))
}

pub(super) fn is_custom(mode: u8) -> bool {
    mode == CUSTOM
}

fn first_channel(app: &mut App, mode: u8) -> Option<u16> {
    for &(a, b) in MODES[mode as usize].ranges {
        for printed in a..=b {
            if !app.storage.is_channel_empty(printed - 1) {
                return Some(printed - 1);
            }
        }
    }
    None
}

pub(super) fn open_menu(app: &mut App) {
    app.mode_menu_index = app.chan_mode as usize;
    app.input.clear();
    app.mode = Mode::ModeMenu;
}

fn open_range(app: &mut App) {
    app.input.clear();
    app.range_stage = 0;
    app.range_err = false;
    app.mode = Mode::RangeInput;
}

/// Two rising notes; the second climbs with the mode's place in the list.
fn play_mode_cue(app: &mut App, syst: &mut SYST) {
    let tones = [(88, 70), (60 + 8 * app.chan_mode as u16, 90)];
    app.radio.play_cue(syst, &tones);
}

fn select(app: &mut App, syst: &mut SYST, index: usize, then_scan: bool) {
    app.chan_mode = index as u8;
    persist(app);
    app.mode = Mode::Standby;
    app.reset_key_idle();

    if is_custom(app.chan_mode) {
        play_mode_cue(app, syst);
        open_range(app);
        return;
    }

    let current = app.sides[app.master].channel_num;
    let keep = !app.storage.is_channel_empty(current)
        && contains(app.chan_mode, current);
    let target = if keep {
        Some(current)
    } else {
        first_channel(app, app.chan_mode)
    };
    match target {
        Some(num) => {
            side_ops::load_channel_num(app, num);
            app.save_channel_state();
        }
        None => app.no_channels_ticks = NO_CHANNELS_HOLD_TICKS,
    }
    app.sync_watching_to_master(syst);
    play_mode_cue(app, syst);
    if then_scan && target.is_some() {
        scan::enter(app, syst);
    }
}

pub(super) fn dispatch_menu(app: &mut App, syst: &mut SYST, ev: KeyEvent) {
    let len = MODES.len();
    match (ev.kind, ev.key) {
        (KeyEventKind::Single | KeyEventKind::Repeat, KeyId::Up) => {
            app.mode_menu_index = (app.mode_menu_index + len - 1) % len;
        }
        (KeyEventKind::Single | KeyEventKind::Repeat, KeyId::Down) => {
            app.mode_menu_index = (app.mode_menu_index + 1) % len;
        }
        (KeyEventKind::Single, KeyId::Menu) => {
            select(app, syst, app.mode_menu_index, false);
        }
        // Hold MENU: pick the mode and start scanning it right away.
        (KeyEventKind::Long, KeyId::Menu) => {
            select(app, syst, app.mode_menu_index, true);
        }
        (KeyEventKind::Single, KeyId::Exit | KeyId::Band) => {
            app.mode = Mode::Standby;
            app.reset_key_idle();
        }
        _ => {}
    }
}

fn step_frequency_index(current: u8, up: bool) -> u8 {
    let len = STEP_LIST_DECI_HZ.len() as u8;
    if up {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

fn commit_range_field(app: &mut App) {
    if !app.input.is_empty() {
        let hz = app.input.value() * 1000;
        if app.range_stage == 0 {
            app.range_lo_hz = hz;
        } else {
            app.range_hi_hz = hz;
        }
    }
    app.input.clear();
}

fn start_range_scan(app: &mut App, syst: &mut SYST) {
    let lo = app.range_lo_hz;
    persist(app);
    let s = &mut app.sides[app.master];
    s.vfo_chan = ChVfoMode::Vfo;
    s.rx_only = false;
    s.reversed = false;
    s.name = [0; 12];
    s.rx_freq_hz = lo;
    s.freq_dir = 0;
    s.offset_hz = 0;
    s.cfg.subaudio_rx = SubAudio::None;
    s.cfg.subaudio_tx = SubAudio::None;
    s.auto_modulation();
    s.refresh_cfg_freqs();
    app.save_vfo();
    app.mode = Mode::Standby;
    app.reset_key_idle();
    app.sync_watching_to_master(syst);
    scan::enter(app, syst);
}

pub(super) fn dispatch_range(app: &mut App, syst: &mut SYST, ev: KeyEvent) {
    if !matches!(ev.kind, KeyEventKind::Single | KeyEventKind::Repeat) {
        return;
    }
    if ev.kind == KeyEventKind::Single {
        if let Some(d) = digit_value(ev.key) {
            app.range_err = false;
            app.input.push(d);
            return;
        }
    }
    match ev.key {
        KeyId::Up | KeyId::Down => {
            let s = &mut app.sides[app.master];
            s.freq_step = step_frequency_index(s.freq_step, ev.key == KeyId::Up);
        }
        KeyId::Menu if ev.kind == KeyEventKind::Single => {
            commit_range_field(app);
            if app.range_stage == 0 {
                app.range_stage = 1;
            } else if range_valid(app.range_lo_hz, app.range_hi_hz) {
                start_range_scan(app, syst);
            } else {
                app.range_err = true;
                app.range_stage = 0;
                app.radio.play_cue(syst, &[(40, 200)]);
            }
        }
        KeyId::Exit if ev.kind == KeyEventKind::Single => {
            if app.input.is_empty() {
                app.mode = Mode::Standby;
                app.reset_key_idle();
            } else {
                app.input.clear();
            }
        }
        _ => {}
    }
}

impl<'a> App<'a> {
    pub fn chan_mode_index(&self) -> usize {
        self.chan_mode as usize
    }
    pub fn chan_mode_name(&self) -> &'static str {
        MODES[self.chan_mode as usize].name
    }
    pub fn chan_mode_short(&self) -> &'static str {
        MODES[self.chan_mode as usize].short
    }
    pub fn mode_menu_index(&self) -> usize {
        self.mode_menu_index
    }
    pub fn range_hz(&self) -> (u32, u32) {
        (self.range_lo_hz, self.range_hi_hz)
    }
    pub fn range_stage(&self) -> u8 {
        self.range_stage
    }
    pub fn range_err(&self) -> bool {
        self.range_err
    }
    pub fn range_step_deci_hz(&self) -> u32 {
        self.sides[self.master].step_deci_hz()
    }
    pub fn range_input_len(&self) -> usize {
        self.input.len
    }
    pub fn range_input_write(&self, w: &mut dyn core::fmt::Write) {
        self.input.write_display(3, w);
    }
}
