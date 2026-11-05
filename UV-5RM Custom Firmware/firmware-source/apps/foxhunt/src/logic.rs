//! Fox Hunt: a large signal meter that says whether a transmitter is getting
//! stronger, plus a rough distance range once calibrated at a known distance.
//!
//! Receive-only. Never call `api.tone_on`: it maps to `Radio::rtone_on`, which
//! keys the transmitter, so it can't be used for a local signal-strength tone.

use bf5rh_abi::{Api, AppEvent, AppResult};

const KIND_SINGLE: u8 = 2;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const GRID: u16 = 0x2124;
const TITLE_BG: u16 = 0x5800;
const BAR: u16 = 0xD000;
const PEAK: u16 = 0xFFFF;
const YELLOW: u16 = 0xFFE0;
const GREEN: u16 = 0x07E0;
const RED: u16 = 0xF800;

const F5: u8 = 0;
const F6: u8 = 1;
const F9: u8 = 2;

const SCREEN_W: u16 = 160;
const SCREEN_H: u16 = 128;

const RSSI_DBM_BASE: i32 = 170;
const WINDOW_100US: u32 = 2_000;
/// A long segment is cut here, so a steady signal still shows a live trend.
const SEGMENT_100US: u32 = 50_000;

/// Above this the noise floor stops following, so a transmitter that never
/// unkeys (a beacon, NOAA Weather) doesn't end up counted as noise.
const FLOOR_CAP: u16 = 75; // -95 dBm
const FLOOR_RISE_100US: u32 = 100_000;
const SIGNAL_ABOVE_FLOOR: u16 = 8;
const LOST_ABOVE_FLOOR: u16 = 4;
const TREND_DB: i32 = 3;
const STRONG_RAW: u16 = 140; // -30 dBm: expect the reading to stop rising

const BAR_RAW_MIN: u32 = 40; // -130 dBm
const BAR_RAW_SPAN: u32 = 100; // up to -30 dBm

const AM_LOW_HZ: u32 = 108_000_000;
const AM_HIGH_HZ: u32 = 136_000_000;
const RX_MIN_HZ: u32 = 16_000_000;
const RX_MAX_HZ: u32 = 560_000_000;

const FREQ_DIGITS: usize = 6;
const DIST_DIGITS: usize = 4;
const DIST_MAX_TENTHS: u64 = 99_999;

/// `10^(x/20)` times 1000, for x in 0..20 dB.
const DB_TO_RATIO: [u16; 20] = [
    1000, 1122, 1259, 1413, 1585, 1778, 1995, 2239, 2512, 2818, 3162, 3548,
    3981, 4467, 5012, 5623, 6310, 7079, 7943, 8913,
];

#[derive(Clone, Copy, PartialEq)]
enum View {
    Meter,
    InputFreq,
    InputDist,
}

struct State {
    view: View,
    hz: u32,
    wide: bool,
    audio: bool,

    window_100us: u32,
    window_sum: u32,
    window_n: u32,
    floor: u16,
    floor_known: bool,
    floor_timer_100us: u32,

    present: bool,
    has_reading: bool,
    level: u16,
    peak: u16,

    seg_sum: u32,
    seg_n: u32,
    seg_100us: u32,
    skip_window: bool,
    reference: u16,
    has_reference: bool,

    cal_level: u16,
    cal_tenths: u32,
    has_cal: bool,

    input: [u8; FREQ_DIGITS],
    input_len: usize,
    /// 0 none, 1 needs a signal, 2 bad value
    notice: u8,

    dirty: bool,
    full: bool,
}

static mut STATE: State = State {
    view: View::Meter,
    hz: 0,
    wide: true,
    audio: true,
    window_100us: 0,
    window_sum: 0,
    window_n: 0,
    floor: FLOOR_CAP,
    floor_known: false,
    floor_timer_100us: 0,
    present: false,
    has_reading: false,
    level: 0,
    peak: 0,
    seg_sum: 0,
    seg_n: 0,
    seg_100us: 0,
    skip_window: false,
    reference: 0,
    has_reference: false,
    cal_level: 0,
    cal_tenths: 0,
    has_cal: false,
    input: [0; FREQ_DIGITS],
    input_len: 0,
    notice: 0,
    dirty: true,
    full: true,
};

fn state() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    match ev {
        AppEvent::Enter => enter(api),
        AppEvent::Key { id, kind } => key(api, id, kind),
        AppEvent::Tick { dt_100us } => tick(api, dt_100us),
        AppEvent::Draw => draw(api),
        AppEvent::Leave => leave(api),
    }
}

/// Readings and calibration only mean something for one frequency.
fn reset_readings(st: &mut State) {
    st.window_100us = 0;
    st.window_sum = 0;
    st.window_n = 0;
    st.floor = FLOOR_CAP;
    st.floor_known = false;
    st.floor_timer_100us = 0;
    st.present = false;
    st.has_reading = false;
    st.level = 0;
    st.peak = 0;
    st.seg_sum = 0;
    st.seg_n = 0;
    st.seg_100us = 0;
    st.has_reference = false;
    st.has_cal = false;
}

fn tune(api: &Api, st: &State) {
    let am = (AM_LOW_HZ..=AM_HIGH_HZ).contains(&st.hz);
    (api.set_modulation)(if am { 1 } else { 0 });
    (api.set_rx_freq)(st.hz);
    (api.set_bandwidth)(st.wide);
    (api.set_subaudio_rx)(0);
    (api.set_sql_level)((api.settings_get)(0) as u8);
    (api.enter_rx)();
    if !st.audio {
        mute(api);
    }
}

/// `enter_rx` hands squelch and audio to the firmware; taking that back mutes.
fn mute(api: &Api) {
    (api.set_holds_rx)(false);
    (api.set_af_out)(false);
}

fn enter(api: &Api) -> AppResult {
    let st = state();
    st.view = View::Meter;
    st.hz = (api.get_master_freq)();
    st.wide = (api.get_master_wide)();
    st.audio = true;
    st.input_len = 0;
    st.notice = 0;
    st.dirty = true;
    st.full = true;
    reset_readings(st);
    (api.set_agc_fix)(-1);
    (api.set_backlight_hold)(true);
    tune(api, st);
    AppResult::Continue
}

fn leave(api: &Api) -> AppResult {
    (api.set_af_out)(false);
    (api.set_backlight_hold)(false);
    AppResult::Continue
}

fn close_segment(st: &mut State) {
    if st.seg_n > 0 {
        st.reference = (st.seg_sum / st.seg_n) as u16;
        st.has_reference = true;
    }
    st.seg_sum = 0;
    st.seg_n = 0;
    st.seg_100us = 0;
}

fn update_floor(st: &mut State, avg: u16) {
    if !st.floor_known || avg < st.floor {
        st.floor = avg.min(FLOOR_CAP);
        st.floor_known = true;
        st.floor_timer_100us = 0;
    } else if st.floor < FLOOR_CAP
        && st.floor_timer_100us >= FLOOR_RISE_100US
    {
        st.floor += 1;
        st.floor_timer_100us = 0;
    }
}

fn finish_window(st: &mut State) {
    let avg = (st.window_sum / st.window_n.max(1)) as u16;
    let elapsed = st.window_100us;
    st.window_100us = 0;
    st.window_sum = 0;
    st.window_n = 0;
    st.floor_timer_100us = st.floor_timer_100us.saturating_add(elapsed);
    update_floor(st, avg);

    let on = if st.present {
        avg >= st.floor + LOST_ABOVE_FLOOR
    } else {
        avg >= st.floor + SIGNAL_ABOVE_FLOOR
    };
    if on != st.present {
        st.present = on;
        st.dirty = true;
        if on {
            st.skip_window = true;
        } else {
            close_segment(st);
        }
    }
    if !on {
        return;
    }

    // The trend and distance move with the running average, not just the level.
    st.dirty = true;
    st.level = avg;
    st.has_reading = true;
    st.peak = st.peak.max(avg);
    if st.skip_window {
        st.skip_window = false;
        return;
    }
    st.seg_sum += avg as u32;
    st.seg_n += 1;
    st.seg_100us = st.seg_100us.saturating_add(elapsed);
    if st.seg_100us >= SEGMENT_100US {
        close_segment(st);
    }
}

fn tick(api: &Api, dt: u32) -> AppResult {
    let st = state();
    if st.view == View::Meter {
        st.window_sum += (api.read_rssi)() as u32;
        st.window_n += 1;
        st.window_100us = st.window_100us.saturating_add(dt);
        if st.window_100us >= WINDOW_100US {
            finish_window(st);
        }
    }
    AppResult::Continue
}

/// The segment in progress, or the last finished one between transmissions.
fn current_level(st: &State) -> u16 {
    if st.seg_n > 0 {
        (st.seg_sum / st.seg_n) as u16
    } else if st.has_reference {
        st.reference
    } else {
        st.level
    }
}

fn commit_freq(api: &Api, st: &mut State) {
    let mut khz: u32 = 0;
    for i in 0..FREQ_DIGITS {
        let d = if i < st.input_len { st.input[i] } else { 0 };
        khz = khz * 10 + d as u32;
    }
    let typed = st.input_len > 0;
    st.input_len = 0;
    st.view = View::Meter;
    if !typed {
        return;
    }
    let hz = khz * 1000;
    if (RX_MIN_HZ..=RX_MAX_HZ).contains(&hz) {
        st.hz = hz;
        st.notice = 0;
        reset_readings(st);
        st.full = true;
        tune(api, st);
    } else {
        st.notice = 2;
    }
}

fn commit_dist(st: &mut State) {
    let mut tenths: u32 = 0;
    for &d in &st.input[..st.input_len] {
        tenths = tenths * 10 + d as u32;
    }
    st.input_len = 0;
    st.view = View::Meter;
    if tenths == 0 {
        st.notice = 2;
        return;
    }
    st.notice = 0;
    st.cal_tenths = tenths;
    st.cal_level = current_level(st);
    st.has_cal = true;
}

fn key(api: &Api, id: u8, kind: u8) -> AppResult {
    if kind != KIND_SINGLE {
        return AppResult::Continue;
    }
    let st = state();
    st.dirty = true;

    if st.view != View::Meter {
        let cap = if st.view == View::InputFreq {
            FREQ_DIGITS
        } else {
            DIST_DIGITS
        };
        match id {
            0..=9 => {
                if st.input_len < cap {
                    st.input[st.input_len] = id;
                    st.input_len += 1;
                }
                if st.view == View::InputFreq && st.input_len == cap {
                    commit_freq(api, st);
                }
            }
            KEY_MENU if st.view == View::InputFreq => commit_freq(api, st),
            KEY_MENU => commit_dist(st),
            KEY_EXIT => {
                st.input_len = 0;
                st.view = View::Meter;
            }
            _ => {}
        }
        return AppResult::Continue;
    }

    match id {
        KEY_EXIT => return AppResult::Exit,
        KEY_MENU => {
            if st.has_reading {
                st.notice = 0;
                st.input_len = 0;
                st.view = View::InputDist;
            } else {
                st.notice = 1;
            }
        }
        1 => {
            st.audio = !st.audio;
            if st.audio {
                tune(api, st);
            } else {
                mute(api);
            }
        }
        5 => {
            st.notice = 0;
            st.input_len = 0;
            st.view = View::InputFreq;
        }
        0 => {
            st.has_cal = false;
            st.peak = st.level;
            st.has_reference = false;
            st.notice = 0;
        }
        _ => {}
    }
    AppResult::Continue
}

// distance

/// `10^(db/divisor)`, in thousandths; `divisor` is 20 or 40.
fn ratio_milli(db: u32, divisor: u32) -> u64 {
    // In steps of 20 dB after scaling, so one table covers both slopes.
    let scaled = db * 20 / divisor;
    let mut out = DB_TO_RATIO[(scaled % 20) as usize] as u64;
    for _ in 0..scaled / 20 {
        out = out.saturating_mul(10);
    }
    out
}

/// Tenths of a mile at `db_weaker` from the calibration point.
fn scale_tenths(tenths: u32, db_weaker: i32, divisor: u32) -> u64 {
    let r = ratio_milli(db_weaker.unsigned_abs(), divisor);
    let t = tenths as u64;
    if db_weaker >= 0 {
        t.saturating_mul(r) / 1000
    } else {
        t.saturating_mul(1000) / r
    }
}

/// `(low, high)` in tenths of a mile. Open ground loses 20 dB per tenfold
/// distance and cluttered ground about 40, so those bound the estimate.
fn distance_range(st: &State) -> (u64, u64) {
    let db_weaker = st.cal_level as i32 - current_level(st) as i32;
    let open = scale_tenths(st.cal_tenths, db_weaker, 20);
    let cluttered = scale_tenths(st.cal_tenths, db_weaker, 40);
    (open.min(cluttered), open.max(cluttered))
}

// ui

struct Buf {
    b: [u8; 34],
    n: usize,
}

impl Buf {
    fn new() -> Buf {
        Buf { b: [0; 34], n: 0 }
    }

    fn ch(&mut self, c: u8) {
        if self.n < self.b.len() {
            self.b[self.n] = c;
            self.n += 1;
        }
    }

    fn s(&mut self, text: &[u8]) {
        for &c in text {
            self.ch(c);
        }
    }

    fn u(&mut self, api: &Api, v: u32) {
        let mut tmp = [0u8; 12];
        let n = (api.fmt_u32)(v, tmp.as_mut_ptr()) as usize;
        self.s(&tmp[..n.min(tmp.len())]);
    }

    fn i(&mut self, api: &Api, v: i32) {
        if v < 0 {
            self.ch(b'-');
            self.u(api, (-v) as u32);
        } else {
            self.u(api, v as u32);
        }
    }

    fn tenths(&mut self, api: &Api, t: u64) {
        let t = t.min(DIST_MAX_TENTHS) as u32;
        self.u(api, t / 10);
        self.ch(b'.');
        self.u(api, t % 10);
    }

    fn freq(&mut self, api: &Api, hz: u32) {
        let mut tmp = [0u8; 16];
        let n = (api.fmt_freq)(hz, tmp.as_mut_ptr()) as usize;
        self.s(&tmp[..n.min(tmp.len())]);
    }

    fn as_slice(&self) -> &[u8] {
        &self.b[..self.n]
    }
}

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16, font: u8) {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, font);
}

fn line(api: &Api, y: i16, h: u16, s: &[u8], fg: u16, font: u8) {
    (api.fill_rect)(0, y, 4, h, BLACK);
    let end = (api.draw_text)(4, y, s.as_ptr(), s.len() as u16, fg, BLACK, font);
    (api.fill_rect)(end as i16, y, SCREEN_W - end, h, BLACK);
}

fn raw_to_x(raw: u16) -> u16 {
    let v = (raw as u32).saturating_sub(BAR_RAW_MIN).min(BAR_RAW_SPAN);
    (v * (SCREEN_W as u32 - 2) / BAR_RAW_SPAN) as u16
}

fn draw_bar(api: &Api, st: &State) {
    let (y, h) = (38i16, 12u16);
    (api.fill_rect)(0, y, SCREEN_W, 1, GRID);
    (api.fill_rect)(0, y + h as i16 - 1, SCREEN_W, 1, GRID);
    (api.fill_rect)(0, y, 1, h, GRID);
    (api.fill_rect)(SCREEN_W as i16 - 1, y, 1, h, GRID);
    let inner = SCREEN_W - 2;
    let w = if st.has_reading { raw_to_x(st.level).min(inner) } else { 0 };
    let color = if st.present { BAR } else { GRAY };
    (api.fill_rect)(1, y + 1, w, h - 2, color);
    (api.fill_rect)(1 + w as i16, y + 1, inner - w, h - 2, BLACK);
    if st.has_reading {
        let p = raw_to_x(st.peak).min(inner - 1);
        (api.fill_rect)(1 + p as i16, y + 1, 1, h - 2, PEAK);
    }
}

fn draw_level(api: &Api, st: &State) {
    let mut b = Buf::new();
    if st.present {
        b.i(api, st.level as i32 - RSSI_DBM_BASE);
        b.s(b" DBM");
        line(api, 16, 20, b.as_slice(), WHITE, F9);
    } else {
        line(api, 16, 20, b"NO SIGNAL", GRAY, F9);
    }
}

fn draw_trend(api: &Api, st: &State) {
    let mut b = Buf::new();
    let mut color = WHITE;
    if st.level >= STRONG_RAW && st.present {
        b.s(b"TOO STRONG");
        color = YELLOW;
    } else if st.present && st.has_reference {
        let diff = current_level(st) as i32 - st.reference as i32;
        if diff >= TREND_DB {
            b.s(b"CLOSER +");
            b.u(api, diff as u32);
            color = GREEN;
        } else if diff <= -TREND_DB {
            b.s(b"FARTHER -");
            b.u(api, (-diff) as u32);
            color = RED;
        } else {
            b.s(b"SAME");
        }
    } else if st.present {
        b.s(b"MEASURING");
    } else if st.has_reading {
        b.s(b"LAST ");
        b.i(api, st.level as i32 - RSSI_DBM_BASE);
        b.s(b" DBM");
        color = GRAY;
    }
    line(api, 52, 20, b.as_slice(), color, F9);
}

fn draw_distance(api: &Api, st: &State) {
    let mut b = Buf::new();
    if st.has_cal && st.has_reading {
        let (low, high) = distance_range(st);
        if high == 0 {
            b.s(b"UNDER 0.1 MI");
        } else {
            b.s(b"ABOUT ");
            b.tenths(api, low);
            b.ch(b'-');
            b.tenths(api, high);
            b.s(b" MI");
        }
        line(api, 74, 12, b.as_slice(), YELLOW, F6);
    } else if st.has_reading {
        line(api, 74, 12, b"MENU: SET DISTANCE", GRAY, F6);
    } else {
        line(api, 74, 12, b"WAITING FOR SIGNAL", GRAY, F6);
    }

    let mut p = Buf::new();
    if st.has_reading {
        p.s(b"PEAK ");
        p.i(api, st.peak as i32 - RSSI_DBM_BASE);
    }
    if !st.audio {
        if st.has_reading {
            p.s(b"  ");
        }
        p.s(b"MUTED");
    }
    line(api, 87, 12, p.as_slice(), GRAY, F6);
}

fn draw_input(api: &Api, st: &State) {
    let mut b = Buf::new();
    if st.view == View::InputFreq {
        for i in 0..FREQ_DIGITS {
            if i == 3 {
                b.ch(b'.');
            }
            b.ch(if i < st.input_len {
                b'0' + st.input[i]
            } else {
                b'_'
            });
        }
        line(api, 52, 20, b.as_slice(), YELLOW, F9);
        line(api, 74, 12, b"TYPE FREQUENCY, MHZ", GRAY, F6);
    } else {
        // Typed digits fill from the right, with one decimal: 1 0 reads 1.0.
        let mut t: u32 = 0;
        for &d in &st.input[..st.input_len] {
            t = t * 10 + d as u32;
        }
        b.tenths(api, t as u64);
        b.s(b" MI");
        line(api, 52, 20, b.as_slice(), YELLOW, F9);
        line(api, 74, 12, b"TYPE 1 0 FOR 1.0 MILE", GRAY, F6);
    }
    (api.fill_rect)(0, 87, SCREEN_W, 12, BLACK);
    line(api, 118, 9, b"MENU OK  EXIT BACK", GRAY, F5);
}

fn draw(api: &Api) -> AppResult {
    let st = state();
    if !st.dirty && !st.full {
        return AppResult::Continue;
    }
    if st.full {
        (api.fill_rect)(0, 0, SCREEN_W, SCREEN_H, BLACK);
        (api.fill_rect)(0, 0, SCREEN_W, 14, TITLE_BG);
        text(api, 4, 2, b"FOX HUNT", WHITE, TITLE_BG, F6);
        let mut f = Buf::new();
        f.freq(api, st.hz);
        let x = SCREEN_W as i16 - f.as_slice().len() as i16 * 6 - 2;
        text(api, x, 2, f.as_slice(), WHITE, TITLE_BG, F6);
        st.full = false;
    }

    draw_level(api, st);
    draw_bar(api, st);
    if st.view != View::Meter {
        draw_input(api, st);
    } else {
        draw_trend(api, st);
        draw_distance(api, st);
        match st.notice {
            1 => line(api, 102, 9, b"NEEDS A SIGNAL FIRST", YELLOW, F5),
            2 => line(api, 102, 9, b"NOT A VALID VALUE", YELLOW, F5),
            _ => (api.fill_rect)(0, 102, SCREEN_W, 9, BLACK),
        }
        line(api, 118, 9, b"1 AUDIO 5 FREQ 0 RESET MENU CAL", GRAY, F5);
    }
    st.dirty = false;
    AppResult::Continue
}

// Read-only views for the PC simulator; not part of the radio build.
#[cfg(not(target_os = "none"))]
pub fn debug_force_redraw() {
    state().full = true;
}

#[cfg(not(target_os = "none"))]
pub fn debug_range() -> (u64, u64) {
    distance_range(state())
}

#[cfg(not(target_os = "none"))]
pub fn debug_trend() -> i32 {
    let st = state();
    current_level(st) as i32 - st.reference as i32
}

#[cfg(not(target_os = "none"))]
pub fn debug_present() -> bool {
    state().present
}

#[cfg(not(target_os = "none"))]
pub fn debug_level() -> u16 {
    state().level
}

#[cfg(not(target_os = "none"))]
pub fn debug_has_cal() -> bool {
    state().has_cal
}

#[cfg(not(target_os = "none"))]
pub fn debug_hz() -> u32 {
    state().hz
}

#[cfg(not(target_os = "none"))]
pub fn debug_notice() -> u8 {
    state().notice
}

#[cfg(not(target_os = "none"))]
pub fn debug_has_reference() -> bool {
    state().has_reference
}
