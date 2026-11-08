//! Repeater Finder: while someone is on a repeater's output, look for the same
//! traffic on the usual input frequencies and read the tone it carries.
//! Receive-only: `set_tx_enabled` is never called, so PTT can't key from here.

use bf5rh_abi::{Api, AppEvent, AppResult};
use app_theme::pal;

const KIND_SINGLE: u8 = 2;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;

const F5: u8 = 0;
const F6: u8 = 1;
const F9: u8 = 2;

const SCREEN_W: i16 = 160;
const SCREEN_H: u16 = 128;

const MAX_CANDS: usize = 2;
const VOTES: usize = 3;

/// `(low Hz, high Hz, offsets in kHz)`: where a standard repeater's input is.
const PLANS: [(u32, u32, &[i32]); 3] = [
    (144_000_000, 148_000_000, &[-600, 600]),
    (222_000_000, 225_000_000, &[-1600]),
    (420_000_000, 450_000_000, &[-5000, 5000]),
];
const GMRS_OUT: (u32, u32) = (462_550_000, 462_725_000);
const GMRS_OFFSET_KHZ: i32 = 5000;

const SETTLE_MS: u32 = 4;
const SAMPLES: u32 = 3;
const NEIGHBOR_HZ: u32 = 100_000;
/// dB a candidate must stand above the quieter of its two neighbors.
const HIT_MARGIN: u16 = 12;
/// dB above that neighbor floor that still counts as the talker being there.
const STAY_MARGIN: u16 = 6;

const OPEN_MIN_100US: u32 = 3_000;
const CLOSE_MIN_100US: u32 = 10_000;
const PROBE_GAP_100US: u32 = 10_000;
const IDLE_GAP_100US: u32 = 50_000;
const TONE_WINDOW_100US: u32 = 15_000;

const CTCSS_COUNT: i32 = 50;
const CTCSS_HITS: u8 = 2;
const VOTES_ENOUGH: u8 = 3;
const TONE_TRIES: u8 = 5;

const CHAN_BYTES: usize = 32;
const FLAG_SCAN: u8 = 0x04;
const FLAG_NARROW: u8 = 0x40;
const POWER_LOW: u8 = 1;
const CHAN_DIGITS: usize = 4;
const CHAN_MAX: u32 = 1000;

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    NoPlan,
    Listen,
    Tone,
    Save,
}

#[derive(Clone, Copy, PartialEq)]
struct Cand {
    hz: u32,
    offset_khz: i32,
    seen: u8,
    /// Times it was busy while the output was quiet: probably not this input.
    idle: u8,
    last_burst: u16,
    votes: [(i32, u8); VOTES],
    no_tone: u8,
    tries: u8,
}

const EMPTY: Cand = Cand {
    hz: 0,
    offset_khz: 0,
    seen: 0,
    idle: 0,
    last_burst: 0,
    votes: [(-1, 0); VOTES],
    no_tone: 0,
    tries: 0,
};

impl Cand {
    fn best_vote(&self) -> (i32, u8) {
        let mut best = (-1, 0);
        for &(idx, n) in &self.votes {
            if n > best.1 {
                best = (idx, n);
            }
        }
        best
    }

    fn tone_mixed(&self) -> bool {
        self.votes.iter().filter(|v| v.1 > 0).count() > 1
    }

    fn add_vote(&mut self, idx: i32) {
        for v in self.votes.iter_mut() {
            if v.1 > 0 && v.0 == idx {
                v.1 = v.1.saturating_add(1);
                return;
            }
        }
        for v in self.votes.iter_mut() {
            if v.1 == 0 {
                *v = (idx, 1);
                return;
            }
        }
    }

    fn wants_tone(&self) -> bool {
        self.tries < TONE_TRIES && self.best_vote().1 < VOTES_ENOUGH
    }
}

struct State {
    phase: Phase,
    out_hz: u32,
    wide: bool,
    cands: [Cand; MAX_CANDS],
    n_cands: usize,
    selected: usize,

    open_100us: u32,
    closed_100us: u32,
    since_probe_100us: u32,
    since_idle_100us: u32,
    burst: u16,

    tone_cand: usize,
    tone_left_100us: u32,
    tone_floor: u16,
    last_idx: i32,
    idx_hits: u8,

    input: [u8; CHAN_DIGITS],
    input_len: usize,
    /// 0 none, 1 saved, 2 in use, 3 bad number
    notice: u8,
    notice_num: u16,

    dirty: bool,
    full: bool,
}

static mut STATE: State = State {
    phase: Phase::NoPlan,
    out_hz: 0,
    wide: true,
    cands: [EMPTY; MAX_CANDS],
    n_cands: 0,
    selected: 0,
    open_100us: 0,
    closed_100us: 0,
    since_probe_100us: 0,
    since_idle_100us: 0,
    burst: 0,
    tone_cand: 0,
    tone_left_100us: 0,
    tone_floor: 0,
    last_idx: -1,
    idx_hits: 0,
    input: [0; CHAN_DIGITS],
    input_len: 0,
    notice: 0,
    notice_num: 0,
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

fn plan_for(st: &mut State) {
    st.n_cands = 0;
    let out = st.out_hz;
    if (GMRS_OUT.0..=GMRS_OUT.1).contains(&out) {
        st.cands[0] = Cand {
            hz: out + GMRS_OFFSET_KHZ as u32 * 1000,
            offset_khz: GMRS_OFFSET_KHZ,
            ..EMPTY
        };
        st.n_cands = 1;
        return;
    }
    for &(lo, hi, offsets) in PLANS.iter() {
        if !(lo..=hi).contains(&out) {
            continue;
        }
        for &khz in offsets {
            let hz = (out as i64 + khz as i64 * 1000) as u32;
            if (lo..=hi).contains(&hz) && st.n_cands < MAX_CANDS {
                st.cands[st.n_cands] = Cand {
                    hz,
                    offset_khz: khz,
                    ..EMPTY
                };
                st.n_cands += 1;
            }
        }
    }
}

fn enter(api: &Api) -> AppResult {
    let st = state();
    st.out_hz = (api.get_master_freq)();
    st.wide = (api.get_master_wide)();
    st.selected = 0;
    st.open_100us = 0;
    st.closed_100us = 0;
    st.since_probe_100us = PROBE_GAP_100US;
    st.since_idle_100us = 0;
    st.burst = 1;
    st.input_len = 0;
    st.notice = 0;
    st.dirty = true;
    st.full = true;
    plan_for(st);
    st.phase = if st.n_cands == 0 {
        Phase::NoPlan
    } else {
        Phase::Listen
    };
    back_to_output(api, st);
    AppResult::Continue
}

fn leave(api: &Api) -> AppResult {
    (api.set_subaudio_scan_filter)(false);
    (api.set_af_out)(false);
    AppResult::Continue
}

/// `enter_rx` also re-applies the subaudio config the tone filter overwrote.
fn back_to_output(api: &Api, st: &State) {
    (api.set_subaudio_scan_filter)(false);
    (api.set_modulation)(0);
    (api.set_rx_freq)(st.out_hz);
    (api.set_bandwidth)(st.wide);
    (api.set_subaudio_rx)(0);
    (api.set_sql_level)((api.settings_get)(0) as u8);
    (api.enter_rx)();
}

fn measure(api: &Api, hz: u32) -> u16 {
    (api.retune_rx)(hz, true);
    (api.delay_ms)(SETTLE_MS);
    let mut sum: u32 = 0;
    for _ in 0..SAMPLES {
        sum += (api.read_rssi)() as u32;
        (api.delay_ms)(1);
    }
    (sum / SAMPLES) as u16
}

/// `(level, floor)`: floor is the quieter neighbor, so one busy neighbor doesn't
/// hide a real signal while broad desense from a strong output raises both.
fn measure_cand(api: &Api, hz: u32) -> (u16, u16) {
    let level = measure(api, hz);
    let below = measure(api, hz - NEIGHBOR_HZ);
    let above = measure(api, hz + NEIGHBOR_HZ);
    (level, below.min(above))
}

fn probe(api: &Api, st: &mut State, output_active: bool) {
    (api.set_holds_rx)(false);
    (api.set_af_out)(false);

    let mut best: Option<(usize, u16, u16)> = None;
    for i in 0..st.n_cands {
        let (level, floor) = measure_cand(api, st.cands[i].hz);
        if level < floor + HIT_MARGIN {
            continue;
        }
        let c = &mut st.cands[i];
        if output_active {
            if c.last_burst != st.burst {
                c.last_burst = st.burst;
                c.seen = c.seen.saturating_add(1);
                st.dirty = true;
            }
            if best.map_or(true, |b| level > b.1) {
                best = Some((i, level, floor));
            }
        } else {
            c.idle = c.idle.saturating_add(1);
            st.dirty = true;
        }
    }

    if let Some((i, _, floor)) = best {
        if st.cands[i].wants_tone() {
            (api.retune_rx)(st.cands[i].hz, true);
            (api.set_subaudio_scan_filter)(true);
            st.tone_cand = i;
            st.tone_left_100us = TONE_WINDOW_100US;
            st.tone_floor = floor;
            st.last_idx = -1;
            st.idx_hits = 0;
            st.phase = Phase::Tone;
            st.dirty = true;
            return;
        }
    }
    back_to_output(api, st);
}

fn finish_tone(api: &Api, st: &mut State, result: Option<i32>) {
    let c = &mut st.cands[st.tone_cand];
    c.tries = c.tries.saturating_add(1);
    match result {
        Some(idx) => c.add_vote(idx),
        None => c.no_tone = c.no_tone.saturating_add(1),
    }
    st.phase = Phase::Listen;
    st.since_probe_100us = 0;
    st.dirty = true;
    back_to_output(api, st);
}

fn tick_tone(api: &Api, st: &mut State, dt: u32) {
    st.tone_left_100us = st.tone_left_100us.saturating_sub(dt);
    // The talker stopped: nothing to read, and that says nothing about a tone.
    if (api.read_rssi)() < st.tone_floor + STAY_MARGIN {
        st.phase = Phase::Listen;
        st.since_probe_100us = 0;
        st.dirty = true;
        back_to_output(api, st);
        return;
    }
    let idx = (api.detect_subaudio)();
    if idx >= CTCSS_COUNT {
        finish_tone(api, st, Some(idx));
    } else if idx >= 0 {
        // A single CTCSS decode is prone to false reads; DCS isn't.
        if idx == st.last_idx {
            st.idx_hits += 1;
        } else {
            st.last_idx = idx;
            st.idx_hits = 1;
        }
        if st.idx_hits >= CTCSS_HITS {
            finish_tone(api, st, Some(idx));
        }
    } else if idx <= -2 || st.tone_left_100us == 0 {
        finish_tone(api, st, None);
    }
}

fn tick_listen(api: &Api, st: &mut State, dt: u32) {
    st.since_probe_100us = st.since_probe_100us.saturating_add(dt);
    st.since_idle_100us = st.since_idle_100us.saturating_add(dt);

    if (api.squelch_open)() {
        if st.open_100us == 0 {
            st.dirty = true;
        }
        st.open_100us = st.open_100us.saturating_add(dt);
        st.closed_100us = 0;
        if st.open_100us >= OPEN_MIN_100US
            && st.since_probe_100us >= PROBE_GAP_100US
        {
            st.since_probe_100us = 0;
            probe(api, st, true);
        }
        return;
    }

    st.closed_100us = st.closed_100us.saturating_add(dt);
    if st.open_100us > 0 && st.closed_100us >= CLOSE_MIN_100US {
        st.open_100us = 0;
        st.burst = st.burst.wrapping_add(1);
        st.dirty = true;
    }
    if st.open_100us == 0 && st.since_idle_100us >= IDLE_GAP_100US {
        st.since_idle_100us = 0;
        probe(api, st, false);
    }
}

fn tick(api: &Api, dt: u32) -> AppResult {
    let st = state();
    match st.phase {
        Phase::Listen => tick_listen(api, st, dt),
        Phase::Tone => tick_tone(api, st, dt),
        Phase::NoPlan | Phase::Save => {}
    }
    AppResult::Continue
}

fn bcd_byte(v: u32) -> u8 {
    (((v / 10) << 4) | (v % 10)) as u8
}

fn hz_to_bcd(hz: u32) -> [u8; 4] {
    let d = hz / 10;
    [
        bcd_byte(d % 100),
        bcd_byte(d / 100 % 100),
        bcd_byte(d / 10_000 % 100),
        bcd_byte(d / 1_000_000),
    ]
}

fn build_record(api: &Api, st: &State, c: &Cand) -> [u8; CHAN_BYTES] {
    let mut r = [0u8; CHAN_BYTES];
    r[0..4].copy_from_slice(&hz_to_bcd(st.out_hz));
    r[4..8].copy_from_slice(&hz_to_bcd(c.hz));
    let (idx, votes) = c.best_vote();
    let code = if votes > 0 {
        (api.subaudio_code_of_index)(idx)
    } else {
        0
    };
    r[10..12].copy_from_slice(&code.to_le_bytes());
    r[14] = POWER_LOW;
    r[15] = FLAG_SCAN | if st.wide { 0 } else { FLAG_NARROW };
    r[16..20].copy_from_slice(&[0xFF; 4]);

    let mut name = Buf::new();
    name.s(b"RPT ");
    name.u(api, st.out_hz / 1_000_000);
    name.ch(b'.');
    name.u3(st.out_hz / 1000 % 1000);
    let n = name.as_slice().len().min(12);
    r[20..32].copy_from_slice(&[0xFF; 12]);
    r[20..20 + n].copy_from_slice(&name.as_slice()[..n]);
    r
}

fn commit_save(api: &Api, st: &mut State) {
    let mut typed: u32 = 0;
    for &d in &st.input[..st.input_len] {
        typed = typed * 10 + d as u32;
    }
    st.input_len = 0;
    st.dirty = true;
    if !(1..=CHAN_MAX).contains(&typed) {
        st.notice = 3;
        return;
    }
    let slot = (typed - 1) as u16;
    st.notice_num = typed as u16;

    // Never overwrite: this list is the user's programming.
    let mut existing = [0u8; CHAN_BYTES];
    if (api.chan_read)(slot, existing.as_mut_ptr(), CHAN_BYTES as u16) {
        st.notice = 2;
        return;
    }
    let record = build_record(api, st, &st.cands[st.selected]);
    if (api.chan_write)(slot, record.as_ptr(), CHAN_BYTES as u16) {
        st.notice = 1;
        st.phase = Phase::Listen;
    } else {
        st.notice = 3;
    }
}

fn key(api: &Api, id: u8, kind: u8) -> AppResult {
    if kind != KIND_SINGLE {
        return AppResult::Continue;
    }
    let st = state();
    st.dirty = true;
    if st.phase == Phase::Save {
        match id {
            0..=9 => {
                st.notice = 0;
                if st.input_len < CHAN_DIGITS {
                    st.input[st.input_len] = id;
                    st.input_len += 1;
                }
            }
            KEY_MENU => commit_save(api, st),
            KEY_EXIT => {
                st.input_len = 0;
                st.notice = 0;
                st.phase = Phase::Listen;
            }
            _ => {}
        }
        return AppResult::Continue;
    }
    match id {
        KEY_EXIT => return AppResult::Exit,
        KEY_UP | KEY_DOWN if st.n_cands > 1 => {
            st.selected = (st.selected + 1) % st.n_cands;
        }
        KEY_MENU if st.phase == Phase::Listen => {
            if st.cands[st.selected].seen > 0 {
                st.input_len = 0;
                st.notice = 0;
                st.phase = Phase::Save;
            }
        }
        _ => {}
    }
    AppResult::Continue
}

// ui

struct Buf {
    b: [u8; 32],
    n: usize,
}

impl Buf {
    fn new() -> Buf {
        Buf { b: [0; 32], n: 0 }
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

    fn u3(&mut self, v: u32) {
        self.ch(b'0' + (v / 100 % 10) as u8);
        self.ch(b'0' + (v / 10 % 10) as u8);
        self.ch(b'0' + (v % 10) as u8);
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

fn line(api: &Api, y: i16, h: u16, s: &[u8], fg: u16, bg: u16, font: u8) {
    (api.fill_rect)(0, y, SCREEN_W as u16, h, bg);
    text(api, 4, y + 1, s, fg, bg, font);
}

fn draw_cand(api: &Api, st: &State, i: usize, y: i16) {
    let c = &st.cands[i];
    let bg = if i == st.selected && st.n_cands > 1 {
        pal().card_hi
    } else {
        pal().bg
    };

    let mut a = Buf::new();
    a.freq(api, c.hz);
    a.ch(b' ');
    a.ch(if c.offset_khz < 0 { b'-' } else { b'+' });
    a.u(api, c.offset_khz.unsigned_abs());
    a.ch(b'K');
    line(api, y, 12, a.as_slice(), pal().text, bg, F6);

    let mut b = Buf::new();
    b.s(b"SEEN ");
    b.u(api, c.seen as u32);
    b.ch(b' ');
    let (idx, votes) = c.best_vote();
    if votes > 0 {
        let mut tmp = [0u8; 16];
        let n = (api.subaudio_format)(idx, tmp.as_mut_ptr(), 16) as usize;
        b.s(&tmp[..n.min(tmp.len())]);
        if c.tone_mixed() {
            b.ch(b'?');
        }
    } else if c.no_tone > 0 {
        b.s(b"NO TONE");
    } else {
        b.s(b"TONE --");
    }
    if c.idle > 0 {
        b.s(b" BUSY");
    }
    let fg = if c.idle > 0 { pal().warn } else { pal().text };
    line(api, y + 12, 12, b.as_slice(), fg, bg, F6);
}

fn draw(api: &Api) -> AppResult {
    let st = state();
    if !st.dirty && !st.full {
        return AppResult::Continue;
    }
    if st.full {
        (api.fill_rect)(0, 0, SCREEN_W as u16, SCREEN_H, pal().bg);
        line(api, 0, 13, b"REPEATER FINDER", pal().text, pal().card, F6);
        let mut out = Buf::new();
        out.freq(api, st.out_hz);
        text(api, 4, 15, b"OUT", pal().dim, pal().bg, F5);
        text(api, 24, 15, out.as_slice(), pal().text, pal().bg, F9);
        st.full = false;
    }

    let mut status = Buf::new();
    match st.phase {
        Phase::NoPlan => status.s(b"NO STANDARD OFFSET"),
        Phase::Listen if st.n_cands > 0 && st.open_100us > 0 => {
            status.s(b"CHECKING INPUTS")
        }
        Phase::Listen => status.s(b"WAITING FOR TRAFFIC"),
        Phase::Tone => status.s(b"READING TONE"),
        Phase::Save => {
            status.s(b"SAVE TO CH ");
            for i in 0..CHAN_DIGITS {
                status.ch(if i < st.input_len {
                    b'0' + st.input[i]
                } else {
                    b'_'
                });
            }
        }
    }
    line(api, 36, 12, status.as_slice(), pal().warn, pal().bg, F6);

    for i in 0..MAX_CANDS {
        let y = 50 + i as i16 * 26;
        if i < st.n_cands {
            draw_cand(api, st, i, y);
        } else {
            (api.fill_rect)(0, y, SCREEN_W as u16, 24, pal().bg);
        }
    }

    let mut hint = Buf::new();
    match st.notice {
        1 => {
            hint.s(b"SAVED TO CH ");
            hint.u(api, st.notice_num as u32);
        }
        2 => {
            hint.s(b"CH ");
            hint.u(api, st.notice_num as u32);
            hint.s(b" IN USE");
        }
        3 => hint.s(b"BAD CHANNEL"),
        _ if st.phase == Phase::Save => hint.s(b"MENU OK  EXIT BACK"),
        _ if st.phase == Phase::NoPlan => hint.s(b"TUNE A REPEATER  EXIT"),
        _ => hint.s(b"MENU SAVE  EXIT"),
    }
    line(api, 110, 12, hint.as_slice(), pal().dim, pal().bg, F6);
    st.dirty = false;
    AppResult::Continue
}


// Read-only views for the PC simulator; not part of the radio build.
#[cfg(not(target_os = "none"))]
pub fn debug_dump() -> std::string::String {
    use std::fmt::Write;
    let st = state();
    let mut out = std::format!("n={} ", st.n_cands);
    for i in 0..st.n_cands {
        let c = &st.cands[i];
        if c.seen >= 3 {
            let _ = write!(out, "{}:{} seen>=3 ({}) ", i, c.hz, c.seen);
        } else {
            let _ = write!(out, "{}:{} seen={} ", i, c.hz, c.seen);
        }
        let (idx, votes) = c.best_vote();
        if votes > 0 {
            let _ = write!(out, "tone={} votes={} ", idx, votes);
        }
        if c.idle > 0 {
            let _ = write!(out, "idle>0 ({}) ", c.idle);
        }
        if c.no_tone > 0 {
            let _ = write!(out, "no_tone>0 ({}) ", c.no_tone);
        }
    }
    out
}

#[cfg(not(target_os = "none"))]
pub fn debug_in_save() -> bool {
    state().phase == Phase::Save
}
