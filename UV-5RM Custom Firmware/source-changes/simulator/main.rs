//! Runs the real Repeater Finder, Band Sweep and menu-list code against a
//! simulated radio on the PC. Writes each screen as a PPM and checks behavior.
#![allow(static_mut_refs, dead_code, unused_imports, unsafe_op_in_unsafe_fn)]

use bf5rh_abi::{Api, AppEvent, AppResult, ListRow};
use embedded_graphics::mono_font::ascii::{FONT_5X8, FONT_6X10, FONT_9X18};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use std::collections::HashMap;
use std::fmt::Write as _;

#[path = "../apps_new/rptfind/src/logic.rs"]
mod rptfind;
#[path = "../apps_new/bandsweep/src/logic.rs"]
mod bandsweep;
#[path = "../apps_new/foxhunt/src/logic.rs"]
mod foxhunt;
#[path = "../apps_new/qtscan2/src/logic.rs"]
mod qtscan2;
#[path = "../apps_new/search2/src/logic.rs"]
mod search2;
#[path = "../../firmware-source/apps/textapp/src/logic.rs"]
mod textapp;
#[path = "../../firmware-source/apps/radiowatch/src/logic.rs"]
mod radiowatch;
#[path = "../../firmware-source/apps/satrx/src/logic.rs"]
mod satrx;

mod flash_map {
    pub mod addr {
        pub const BOOT_LOGO_ADDR: u32 = 0x000C_0000;
    }
}

mod drivers {
    pub mod st7735 {
        pub const WIDTH: u16 = 160;
        pub const HEIGHT: u16 = 128;
    }
}

pub mod ui {
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics::prelude::*;

    pub(crate) const SCREEN_W: i32 = 160;
    pub(crate) const SCREEN_H: i32 = 128;

    #[derive(Clone, Copy, PartialEq)]
    pub struct TextBuf<const N: usize> {
        buf: [u8; N],
        len: usize,
    }

    impl<const N: usize> TextBuf<N> {
        pub fn new() -> Self {
            TextBuf { buf: [0; N], len: 0 }
        }
        pub fn as_str(&self) -> &str {
            core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
        }
    }

    impl<const N: usize> core::fmt::Write for TextBuf<N> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            let n = bytes.len().min(N - self.len);
            self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
            self.len += n;
            Ok(())
        }
    }

    #[path = "../../backdrop.rs"]
    pub mod backdrop;
    #[path = "../../../firmware-source/src/ui/list.rs"]
    pub mod list;
    #[path = "../../../firmware-source/src/ui/theme.rs"]
    #[allow(dead_code)]
    pub(crate) mod theme;

    /// Same as firmware-source/src/ui/mod.rs (list.rs uses it).
    pub(crate) fn num(w: &mut dyn core::fmt::Write, mut v: u32, width: usize) {
        let mut d = [b'0'; 10];
        let mut n = 0;
        while v > 0 || n < width.max(1) {
            d[9 - n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
        w.write_str(unsafe { core::str::from_utf8_unchecked(&d[10 - n..]) }).ok();
    }
}

const W: usize = 160;
const H: usize = 128;

struct Signal {
    hz: u32,
    level: u16,
    tone: i32,
    active: Box<dyn Fn(u32) -> bool>,
}

struct Sim {
    now: u32,
    utc_origin: Option<(u32, u32)>,
    fb: Vec<u16>,
    tuned: u32,
    wide: bool,
    cfg_rx: u32,
    modulation: u8,
    filter_on: bool,
    filter_since: u32,
    holds_rx: bool,
    af: bool,
    floor: u16,
    noise: u32,
    desense_when: Option<(u32, u16)>,
    signals: Vec<Signal>,
    master: u32,
    master_wide: bool,
    chans: HashMap<u16, [u8; 32]>,
    ptt_calls: u32,
    tx_enable_calls: u32,
    vfo_saved: Option<(u32, u16)>,
    retunes: u32,
    probes_on_output_audio_gap: u32,
    freq_scan_on: bool,
    sub_saved: Option<(u16, bool)>,
    transmitting: bool,
    tx_enabled: bool,
    tx_block: bool,
    power: u8,
}

static mut SIM: Option<Sim> = None;

fn sim() -> &'static mut Sim {
    unsafe { SIM.as_mut().unwrap() }
}

fn reset(master: u32) {
    unsafe {
        SIM = Some(Sim {
            now: 1000,
            utc_origin: None,
            fb: vec![0; W * H],
            tuned: master,
            wide: true,
            cfg_rx: master,
            modulation: 0,
            filter_on: false,
            filter_since: 0,
            holds_rx: false,
            af: false,
            floor: 60,
            noise: 12345,
            desense_when: None,
            signals: Vec::new(),
            master,
            master_wide: true,
            chans: HashMap::new(),
            ptt_calls: 0,
            tx_enable_calls: 0,
            vfo_saved: None,
            retunes: 0,
            probes_on_output_audio_gap: 0,
            freq_scan_on: false,
            sub_saved: None,
            transmitting: false,
            tx_enabled: false,
            tx_block: false,
            power: 0xFF,
        });
    }
}

fn add(hz: u32, level: u16, tone: i32, active: impl Fn(u32) -> bool + 'static) {
    sim().signals.push(Signal { hz, level, tone, active: Box::new(active) });
}

/// The strongest signal the receiver would see at `hz`, wide filter: about
/// +/-12.5 kHz passes, with a sharp fall-off beyond.
fn level_at(hz: u32) -> u16 {
    let s = sim();
    s.noise = s.noise.wrapping_mul(1664525).wrapping_add(1013904223);
    let jitter = ((s.noise >> 24) % 5) as u16;
    let mut level = s.floor + jitter;
    if let Some((other, extra)) = s.desense_when {
        if s.signals.iter().any(|sig| sig.hz == other && (sig.active)(s.now)) {
            level += extra;
        }
    }
    for sig in &s.signals {
        if !(sig.active)(s.now) {
            continue;
        }
        let d = sig.hz.abs_diff(hz);
        let seen = if d <= 7_500 {
            sig.level
        } else if d <= 15_000 {
            sig.level.saturating_sub(12)
        } else {
            0
        };
        level = level.max(seen);
    }
    level
}

// fake firmware table

extern "C" fn uptime() -> u32 {
    sim().now = sim().now.wrapping_add(1);
    sim().now
}
extern "C" fn delay_ms(ms: u32) {
    sim().now = sim().now.wrapping_add(ms * 10);
}

impl OriginDimensions for Sim {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Sim {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < W && (p.y as usize) < H {
                self.fb[p.y as usize * W + p.x as usize] = RawU16::from(c).into_inner();
            }
        }
        Ok(())
    }
}

extern "C" fn fill_rect(x: i16, y: i16, w: u16, h: u16, c: u16) {
    let s = sim();
    for yy in y as i32..y as i32 + h as i32 {
        for xx in x as i32..x as i32 + w as i32 {
            assert!(xx >= 0 && yy >= 0 && (xx as usize) < W && (yy as usize) < H, "fill off screen {xx},{yy}");
            s.fb[yy as usize * W + xx as usize] = c;
        }
    }
}
extern "C" fn blit(_x: i16, _y: i16, _w: u16, _h: u16, _p: *const u16) {}
extern "C" fn draw_text(x: i16, y: i16, t: *const u8, len: u16, fg: u16, bg: u16, font: u8) -> u16 {
    let bytes = unsafe { std::slice::from_raw_parts(t, len as usize) };
    let text = std::str::from_utf8(bytes).unwrap();
    let f: &MonoFont<'_> = match font {
        2 => &FONT_9X18,
        1 => &FONT_6X10,
        _ => &FONT_5X8,
    };
    assert!(
        x as i32 + text.len() as i32 * f.character_size.width as i32 <= W as i32 + 1,
        "text runs off the screen: {text:?} at x={x}"
    );
    let style = MonoTextStyleBuilder::new()
        .font(f)
        .text_color(Rgb565::from(RawU16::new(fg)))
        .background_color(Rgb565::from(RawU16::new(bg)))
        .build();
    Text::new(text, Point::new(x as i32, y as i32 + f.baseline as i32), style).draw(sim()).ok();
    x as u16 + (text.len() as u16 * f.character_size.width as u16)
}
extern "C" fn draw_line(_: i16, _: i16, _: i16, _: i16, _: u16) {}
extern "C" fn draw_circle(_: i16, _: i16, _: u16, _: u16, _: bool) {}
extern "C" fn set_rx_freq(hz: u32) {
    sim().cfg_rx = hz;
}
extern "C" fn set_tx_freq(_: u32) {}
extern "C" fn set_ptt(on: bool) {
    let s = sim();
    s.ptt_calls += 1;
    s.transmitting = on && s.tx_enabled && !s.tx_block;
    if s.transmitting {
        chip().tx_key_time = s.now;
    }
}
extern "C" fn set_power(level: u8) {
    sim().power = level;
}
extern "C" fn read_rssi() -> u16 {
    sim().now = sim().now.wrapping_add(2);
    level_at(sim().tuned)
}
extern "C" fn set_modulation(m: u8) {
    sim().modulation = m;
}
static mut TONE_ON_CALLS: u32 = 0;
extern "C" fn tone_on(_: u16) {
    unsafe { TONE_ON_CALLS += 1 };
}
extern "C" fn nop_u8(_: u8) {}
extern "C" fn nop_u16(_: u16) {}
extern "C" fn nop() {}
extern "C" fn nop_bool(_: bool) {}
static mut SPEAKER_ON: bool = false;
static mut SPEAKER_TURN_ONS: u32 = 0;
extern "C" fn speaker(on: bool) {
    unsafe {
        if on && !SPEAKER_ON {
            SPEAKER_TURN_ONS += 1;
        }
        SPEAKER_ON = on;
    }
}
extern "C" fn read_mic() -> u8 {
    0
}
// A 2 MB flash chip that keeps its contents across app launches. Like the real one,
// writes can only clear bits, and only an erase sets them.
static mut NOR: Vec<u8> = Vec::new();
static mut NOR_ERASES: u32 = 0;

fn nor() -> &'static mut Vec<u8> {
    unsafe {
        if NOR.is_empty() {
            NOR = vec![0xFF; 0x20_0000];
        }
        &mut NOR
    }
}

fn nor_clear() {
    nor().iter_mut().for_each(|b| *b = 0xFF);
}

extern "C" fn nor_read(addr: u32, out: *mut u8, len: u32) -> bool {
    let s = &nor()[addr as usize..(addr + len) as usize];
    unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), out, len as usize) };
    true
}
extern "C" fn nor_write(addr: u32, data: *const u8, len: u32) -> bool {
    assert!((0x1F_0000..0x1F_4000).contains(&addr), "TEXT wrote outside its log: {addr:#x}");
    assert!(addr as u64 + len as u64 <= 0x1F_4000);
    assert!(addr / 256 == (addr + len - 1) / 256, "this test expects records inside one page");
    let d = unsafe { std::slice::from_raw_parts(data, len as usize) };
    for (i, &b) in d.iter().enumerate() {
        nor()[addr as usize + i] &= b;
    }
    true
}
extern "C" fn nor_erase(addr: u32) -> bool {
    assert!((0x1F_0000..0x1F_4000).contains(&addr), "TEXT erased outside its log: {addr:#x}");
    assert!(addr % 4096 == 0);
    unsafe { NOR_ERASES += 1 };
    nor()[addr as usize..addr as usize + 4096].iter_mut().for_each(|b| *b = 0xFF);
    true
}
fn copy_str(s: &str, out: *mut u8) -> u16 {
    unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), out, s.len()) };
    s.len() as u16
}
extern "C" fn fmt_u32(v: u32, out: *mut u8) -> u16 {
    copy_str(&v.to_string(), out)
}
extern "C" fn fmt_freq(hz: u32, out: *mut u8) -> u16 {
    copy_str(&format!("{}.{:05}", hz / 1_000_000, (hz % 1_000_000) / 10), out)
}
extern "C" fn fm_tune(_: u32) {}
extern "C" fn fm_status() -> u32 {
    0
}
extern "C" fn fm_load(_: *mut u8, _: u16) -> bool {
    false
}
extern "C" fn fm_save(_: *const u8, _: u16) -> bool {
    false
}
extern "C" fn chan_read(num: u16, out: *mut u8, _len: u16) -> bool {
    match sim().chans.get(&num) {
        Some(r) => {
            unsafe { std::ptr::copy_nonoverlapping(r.as_ptr(), out, 32) };
            true
        }
        None => false,
    }
}
extern "C" fn settings_get(id: u16) -> u32 {
    if id == 0 {
        3
    } else {
        0
    }
}
extern "C" fn app_fault(line: u32) -> ! {
    panic!("app fault {line}");
}
extern "C" fn chan_write(num: u16, buf: *const u8, len: u16) -> bool {
    assert_eq!(len, 32);
    let mut r = [0u8; 32];
    unsafe { std::ptr::copy_nonoverlapping(buf, r.as_mut_ptr(), 32) };
    sim().chans.insert(num, r);
    true
}
extern "C" fn get_master_freq() -> u32 {
    sim().master
}
extern "C" fn draw_list_stub(_: *const u8, _: u16, _: *const ListRow, _: u16, _: u16, _: u16, _: u16, _: bool) {}
const CTCSS: [u16; 50] = [
    670, 693, 719, 744, 770, 797, 825, 854, 885, 915, 948, 974, 1000, 1035, 1072, 1109, 1148, 1188, 1230, 1273,
    1318, 1365, 1413, 1462, 1514, 1567, 1598, 1622, 1655, 1679, 1713, 1738, 1773, 1799, 1835, 1862, 1899, 1928,
    1966, 1995, 2035, 2065, 2107, 2181, 2257, 2291, 2336, 2418, 2503, 2541,
];
const IDX_100: i32 = 12;
const IDX_131_8: i32 = 20;
extern "C" fn index_of(_: u16) -> i32 {
    -1
}
extern "C" fn code_of(v: i32) -> u16 {
    if v >= 0 && v < 50 {
        CTCSS[v as usize]
    } else if v >= 50 {
        (v - 49) as u16
    } else {
        0
    }
}
extern "C" fn sub_format(v: i32, out: *mut u8, _cap: u16) -> u16 {
    let s = if v < 0 {
        "OFF".to_string()
    } else if v < 50 {
        format!("{}.{}Hz", CTCSS[v as usize] / 10, CTCSS[v as usize] % 10)
    } else {
        format!("D{:03o}N", v - 49)
    };
    copy_str(&s, out)
}
extern "C" fn handoff_w(_: *const u8, _: u16) -> bool {
    false
}
extern "C" fn handoff_r(_: *mut u8, _: u16) -> u16 {
    0
}
extern "C" fn set_bandwidth(w: bool) {
    sim().wide = w;
}
extern "C" fn enter_rx() {
    let s = sim();
    s.holds_rx = true;
    s.tuned = s.cfg_rx;
    s.filter_on = false;
    s.af = false;
    s.now = s.now.wrapping_add(20);
}
extern "C" fn retune_rx(hz: u32, wide: bool) {
    let s = sim();
    s.tuned = hz;
    s.wide = wide;
    s.retunes += 1;
    s.now = s.now.wrapping_add(2);
}
extern "C" fn read_gains(_: *mut u16) -> bool {
    false
}
extern "C" fn adjust_gain(_: u8, _: bool) {}
extern "C" fn tx_state() -> u32 {
    sim().transmitting as u32
}
extern "C" fn set_tx_enabled(on: bool) {
    sim().tx_enable_calls += 1;
    sim().tx_enabled = on;
}
extern "C" fn utc_get() -> u32 {
    let s = sim();
    s.utc_origin.map_or(0, |(secs, tick)| secs + s.now.wrapping_sub(tick) / 10_000)
}
extern "C" fn utc_set(secs: u32) {
    let s = sim();
    s.utc_origin = Some((secs, s.now));
}
extern "C" fn get_master_wide() -> bool {
    sim().master_wide
}
extern "C" fn squelch_open() -> bool {
    level_at(sim().tuned) >= sim().floor + 14
}
extern "C" fn sub_filter(on: bool) {
    let s = sim();
    s.filter_on = on;
    s.filter_since = s.now;
}
extern "C" fn detect() -> i32 {
    let s = sim();
    if !s.filter_on || s.now.wrapping_sub(s.filter_since) < 2000 {
        return -1;
    }
    for sig in &s.signals {
        if sig.hz == s.tuned && (sig.active)(s.now) && sig.tone >= 0 {
            return sig.tone;
        }
    }
    -1
}
extern "C" fn save_sub(code: u16, also_tx: bool) {
    sim().sub_saved = Some((code, also_tx));
}
extern "C" fn scan_on() {
    sim().freq_scan_on = true;
}
extern "C" fn scan_off() {
    sim().freq_scan_on = false;
}
/// The frequency counter locks onto the strongest signal, in 10 Hz words.
extern "C" fn check_scan() -> u32 {
    let s = sim();
    assert!(s.freq_scan_on, "counter read while off");
    s.now = s.now.wrapping_add(2);
    s.noise = s.noise.wrapping_mul(1664525).wrapping_add(1013904223);
    let jitter = (s.noise >> 24) % 5;
    let now = s.now;
    s.signals.iter().filter(|g| (g.active)(now)).max_by_key(|g| g.level)
        .map_or(u32::MAX, |g| g.hz / 10 + jitter)
}
extern "C" fn correct(v: u32) -> u32 {
    v
}
extern "C" fn tune_cand(hz: u32, _: bool) {
    sim().tuned = hz;
}
extern "C" fn save_vfo(hz: u32, code: u16) {
    sim().vfo_saved = Some((hz, code));
}
extern "C" fn settings_read(_: *mut u8, _: u16) -> bool {
    false
}
extern "C" fn settings_w(_: *const u8, _: u16) -> bool {
    false
}
extern "C" fn factory() -> ! {
    panic!("factory reset called");
}
extern "C" fn batt() -> u16 {
    0
}
extern "C" fn holds(on: bool) {
    sim().holds_rx = on;
}
extern "C" fn af(on: bool) {
    sim().af = on;
}
extern "C" fn agc(_: i8) {}

static API: Api = Api {
    uptime_100us: uptime,
    delay_ms,
    fill_rect,
    blit,
    draw_text,
    draw_line,
    draw_circle,
    set_rx_freq,
    set_tx_freq,
    set_ptt,
    read_rssi,
    set_modulation,
    set_power,
    set_subaudio_tx: nop_u16,
    set_subaudio_rx: nop_u16,
    tone_on,
    tone_off: nop,
    set_speaker: speaker,
    read_mic_level: read_mic,
    nor_read,
    nor_write,
    nor_erase_sector: nor_erase,
    fmt_u32,
    fmt_freq,
    fm_tune_khz: fm_tune,
    fm_seek: nop_bool,
    fm_status,
    fm_tuned_freq_khz: fm_status,
    fm_power_off: nop,
    fm_channels_load: fm_load,
    fm_channels_save: fm_save,
    chan_read,
    settings_get,
    app_fault,
    chan_write,
    get_master_freq,
    draw_list: draw_list_stub,
    subaudio_index_of_code: index_of,
    subaudio_code_of_index: code_of,
    subaudio_format: sub_format,
    handoff_write: handoff_w,
    handoff_read: handoff_r,
    set_bandwidth,
    set_sql_level: nop_u8,
    set_monitor: nop_bool,
    enter_rx,
    retune_rx,
    read_rf_gains: read_gains,
    adjust_rf_gain: adjust_gain,
    tx_state,
    set_tx_enabled,
    utc_get,
    utc_set,
    set_backlight_hold: nop_bool,
    get_master_wide,
    squelch_open,
    set_subaudio_scan_filter: sub_filter,
    detect_subaudio: detect,
    save_master_subaudio: save_sub,
    freq_scan_enable: scan_on,
    freq_scan_disable: scan_off,
    check_freq_scan: check_scan,
    correct_measured_freq_word: correct,
    tune_search_candidate: tune_cand,
    save_master_vfo: save_vfo,
    settings_read,
    settings_preview: settings_w,
    settings_commit: settings_w,
    side_cfg_read: settings_read,
    side_cfg_write: settings_w,
    factory_reset: factory,
    battery_raw12_avg: batt,
    set_holds_rx: holds,
    set_af_out: af,
    set_agc_fix: agc,
};

type Entry = fn(&Api, AppEvent) -> AppResult;

/// Runs the main loop's tick + draw for `secs` of radio time.
fn run(app: Entry, secs: u32) {
    let end = sim().now.wrapping_add(secs * 10_000);
    let mut last = sim().now;
    while sim().now.wrapping_sub(end) > u32::MAX / 2 {
        sim().now = sim().now.wrapping_add(50); // main loop gap between ticks
        let dt = sim().now.wrapping_sub(last);
        last = sim().now;
        app(&API, AppEvent::Tick { dt_100us: dt });
        app(&API, AppEvent::Draw);
    }
}

fn press(app: Entry, id: u8) -> bool {
    let r = app(&API, AppEvent::Key { id, kind: 2 });
    app(&API, AppEvent::Draw);
    matches!(r, AppResult::Exit)
}

fn hold(app: Entry, id: u8) -> bool {
    let r = app(&API, AppEvent::Key { id, kind: 3 });
    app(&API, AppEvent::Draw);
    matches!(r, AppResult::Exit)
}

fn screen_text() -> String {
    String::new()
}

fn shot(name: &str) {
    let dir = std::env::args().nth(1).unwrap();
    let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
    for &p in &sim().fb {
        out.push((((p >> 11) & 0x1F) * 255 / 31) as u8);
        out.push((((p >> 5) & 0x3F) * 255 / 63) as u8);
        out.push(((p & 0x1F) * 255 / 31) as u8);
    }
    std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
}

static mut FAILS: u32 = 0;
static mut PASSES: u32 = 0;

fn check(cond: bool, what: &str) {
    unsafe {
        if cond {
            PASSES += 1;
            println!("  ok   {what}");
        } else {
            FAILS += 1;
            println!("  FAIL {what}");
        }
    }
}

// 6 seconds of talk every 12, plus the repeater's 2 second tail on the output.
const fn period_s() -> u32 {
    12
}
fn talking(t: u32) -> bool {
    (t % (period_s() * 10_000)) < 60_000
}
fn output_on(t: u32) -> bool {
    (t % (period_s() * 10_000)) < 80_000
}

const OUT: u32 = 146_940_000;
const IN_LOW: u32 = 146_340_000;
const IN_HIGH: u32 = 147_540_000;

fn rpt_fresh(master: u32) {
    reset(master);
    rptfind::entry(&API, AppEvent::Enter);
    rptfind::entry(&API, AppEvent::Draw);
}

fn test_repeater() {
    let app: Entry = rptfind::entry;
    println!("Repeater Finder: normal traffic, input reachable, 100.0 Hz tone");
    reset(OUT);
    add(OUT, 120, IDX_131_8, output_on);
    add(IN_LOW, 100, IDX_100, talking);
    app(&API, AppEvent::Enter);
    app(&API, AppEvent::Draw);
    shot("rpt_1_waiting");
    run(app, 4);
    shot("rpt_2_during");
    run(app, 120);
    shot("rpt_3_found");
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("0:146340000 seen>=3"), "found 146.340 as the input on several transmissions");
    check(dump.contains("tone=12"), "read the input's 100.0 tone, not the output's 131.8");
    check(dump.contains("1:147540000 seen=0"), "didn't report the empty +600 kHz frequency");
    check(sim().tuned == OUT || !sim().holds_rx, "ended back on the output");

    println!("Repeater Finder: saving");
    sim().chans.insert(0, [1; 32]);
    press(app, 12);
    for d in [1u8] {
        press(app, d);
    }
    press(app, 12);
    shot("rpt_4_in_use");
    check(sim().chans.get(&0) == Some(&[1; 32]), "refused to overwrite an occupied channel");
    for d in [3u8, 5, 0] {
        press(app, d);
    }
    shot("rpt_5_typing");
    press(app, 12);
    shot("rpt_6_saved");
    let r = sim().chans.get(&349).copied();
    check(r.is_some(), "saved to channel 350 (slot 349)");
    if let Some(r) = r {
        println!("     {}", r.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
        check(r[0..4] == [0x00, 0x40, 0x69, 0x14], "RX is the repeater's output, 146.940");
        check(r[4..8] == [0x00, 0x40, 0x63, 0x14], "TX is the found input, 146.340");
        check(r[8..10] == [0, 0], "no receive tone");
        check(u16::from_le_bytes([r[10], r[11]]) == 1000, "transmit tone 100.0");
        check(r[14] == 1 && r[15] == 0x04, "low power, in scan, wide");
        check(r[16..20] == [0xFF; 4], "decoder bytes match existing channels");
        check(&r[20..31] == b"RPT 146.940" && r[31] == 0xFF, "named RPT 146.940");
    }

    println!("Repeater Finder: bad channel numbers");
    rpt_fresh(OUT);
    add(OUT, 120, IDX_131_8, output_on);
    add(IN_LOW, 100, IDX_100, talking);
    run(app, 40);
    press(app, 12);
    for d in [9u8, 9, 9, 9] {
        press(app, d);
    }
    press(app, 12);
    check(sim().chans.is_empty(), "channel 9999 rejected");
    for d in [0u8] {
        press(app, d);
    }
    press(app, 12);
    check(sim().chans.is_empty(), "channel 0 rejected");

    println!("Repeater Finder: can't hear the users, only the repeater");
    rpt_fresh(OUT);
    add(OUT, 120, IDX_131_8, output_on);
    run(app, 60);
    shot("rpt_7_nothing");
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("0:146340000 seen=0") && dump.contains("1:147540000 seen=0"), "reports nothing rather than guessing");
    press(app, 12);
    check(sim().chans.is_empty() && !rptfind_in_save(), "nothing to save");

    println!("Repeater Finder: the 'input' is another station that's always on");
    rpt_fresh(OUT);
    add(OUT, 120, IDX_131_8, output_on);
    add(IN_LOW, 110, -1, |_| true);
    run(app, 90);
    shot("rpt_8_busy");
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("idle>0"), "flags it BUSY because it's on when the repeater isn't");

    println!("Repeater Finder: a very strong output raises everything nearby");
    rpt_fresh(OUT);
    add(OUT, 160, IDX_131_8, output_on);
    sim().desense_when = Some((OUT, 25));
    run(app, 60);
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("0:146340000 seen=0"), "no false input from desense");

    println!("Repeater Finder: DCS and no-tone inputs");
    rpt_fresh(OUT);
    add(OUT, 120, -1, output_on);
    add(IN_LOW, 100, 50 + 18, talking);
    run(app, 60);
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("tone=68"), "reads DCS");
    rpt_fresh(OUT);
    add(OUT, 120, -1, output_on);
    add(IN_LOW, 100, -1, talking);
    run(app, 60);
    shot("rpt_9_no_tone");
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("no_tone>0"), "reports NO TONE when there isn't one");

    println!("Repeater Finder: other frequencies");
    rpt_fresh(462_675_000);
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("n=1 0:467675000"), "GMRS checks +5 MHz only");
    rpt_fresh(449_500_000);
    let dump = rptfind_debug();
    println!("     {dump}");
    check(dump.contains("n=1 0:444500000"), "70 cm drops the offset that leaves the band");
    rpt_fresh(224_000_000);
    check(rptfind_debug().contains("n=1 0:222400000"), "1.25 m uses -1.6 MHz");
    rpt_fresh(155_000_000);
    shot("rpt_10_no_plan");
    check(rptfind_debug().contains("n=0"), "non-repeater frequency says so");
    check(sim().ptt_calls == 0 && sim().tx_enable_calls == 0, "never touched the transmitter");
    check(press(app, 13), "Exit leaves");
    app(&API, AppEvent::Leave);
}

fn rptfind_debug() -> String {
    rptfind::debug_dump()
}

fn rptfind_in_save() -> bool {
    rptfind::debug_in_save()
}

fn bs_fresh(master: u32) {
    reset(master);
}

fn on_for(period_s: u32, on_s: u32) -> impl Fn(u32) -> bool {
    move |t| (t % (period_s * 10_000)) < on_s * 10_000
}

fn test_sweep() {
    let app: Entry = bandsweep::entry;
    println!("Band Sweep: VHF with constant and intermittent signals");
    bs_fresh(146_950_000);
    add(146_950_000, 100, -1, |_| true);
    add(162_550_000, 90, -1, |_| true);
    add(155_350_000, 110, -1, on_for(45, 10));
    add(151_880_000, 80, -1, |_| true);
    app(&API, AppEvent::Enter);
    app(&API, AppEvent::Draw);
    shot("bs_1_start");
    run(app, 2);
    shot("bs_2_partial");
    run(app, 60);
    shot("bs_3_vhf");
    let dump = bandsweep::debug_dump();
    println!("     {dump}");
    check(dump.contains("hz=155350000") && dump.contains("hz=146950000") && dump.contains("hz=162550000") && dump.contains("hz=151875000"), "found all four signals");
    check(dump.contains("n=4"), "no false signals from noise");
    let order = dump.find("155350000").unwrap_or(0) < dump.find("146950000").unwrap_or(0)
        && dump.find("146950000").unwrap_or(0) < dump.find("162550000").unwrap_or(0);
    check(order, "strongest first");
    check(dump.contains("pass_s="), "timed a full pass");
    println!("     pass time: {}", bandsweep::debug_pass());

    println!("Band Sweep: listening");
    let tuned_before = sim().retunes;
    let listening_to = bandsweep::debug_selected_hz();
    press(app, 12);
    shot("bs_4_listen");
    check(sim().cfg_rx == listening_to && sim().holds_rx && sim().modulation == 0, "Menu tunes the selected signal in FM");
    run(app, 3);
    check(sim().retunes == tuned_before, "sweep pauses while listening");
    press(app, 12);
    check(!sim().holds_rx, "Menu returns to sweeping");
    run(app, 3);
    check(sim().retunes > tuned_before, "sweep resumed");

    println!("Band Sweep: skip a signal");
    let before = bandsweep::debug_count();
    let hidden = format!("hz={}", bandsweep::debug_selected_hz());
    press(app, 19);
    run(app, 30);
    let dump = bandsweep::debug_dump();
    println!("     {dump}");
    check(bandsweep::debug_count() == before - 1 && !dump.contains(&hidden), "Side 1 hides the selected signal and it stays hidden");

    println!("Band Sweep: send to VFO for Spectrum");
    let selected = bandsweep::debug_selected_hz();
    check(hold(app, 12), "holding Menu leaves");
    check(sim().vfo_saved == Some((selected, 0)), "saved the selected frequency to VFO with no tone");
    app(&API, AppEvent::Leave);

    println!("Band Sweep: airband");
    bs_fresh(121_500_000);
    add(121_500_000, 100, -1, |_| true);
    app(&API, AppEvent::Enter);
    run(app, 12);
    check(bandsweep::debug_band() == 2, "opens on the band the radio is tuned to");
    press(app, 12);
    check(sim().modulation == 1 && sim().cfg_rx == 121_500_000, "listens in AM");
    press(app, 12);
    check(sim().modulation == 0, "back to FM for sweeping");

    println!("Band Sweep: band, step and custom range");
    press(app, 17);
    check(bandsweep::debug_total() == (220_000_000u32 + 40_000_000 - 200_000_000 - 20_000_000 + 20_000_000) / 25_000 + 1, "A/B moves to the next band and restarts");
    bs_fresh(440_000_000);
    app(&API, AppEvent::Enter);
    check(bandsweep::debug_total() == 4801, "UHF 400-520 at 25 kHz is 4801 steps");
    press(app, 1);
    check(bandsweep::debug_total() == 9601, "1 changes to 12.5 kHz steps");
    run(app, 40);
    println!("     pass time at 12.5 kHz: {}", bandsweep::debug_pass());
    shot("bs_5_uhf");
    press(app, 5);
    for d in [4u8, 5, 0, 0, 0, 0] {
        press(app, d);
    }
    shot("bs_6_custom");
    for d in [4u8, 5, 3, 0, 0, 0] {
        press(app, d);
    }
    check(bandsweep::debug_lo() == 450_000_000 && bandsweep::debug_total() == 3_000_000 / 12_500 + 1, "custom 450-453 MHz");
    add(452_975_000, 90, -1, |_| true);
    run(app, 12);
    shot("bs_7_custom_run");
    let dump = bandsweep::debug_dump();
    println!("     {dump}");
    check(dump.contains("hz=452975000"), "finds a signal in the custom range");
    press(app, 5);
    for d in [5u8, 0, 0, 0, 0, 0] {
        press(app, d);
    }
    for d in [4u8, 0, 0, 0, 0, 0] {
        press(app, d);
    }
    check(bandsweep::debug_lo() == 450_000_000, "stop below start is rejected");
    press(app, 13);
    press(app, 13);

    println!("Band Sweep: more signals than the list holds");
    bs_fresh(440_000_000);
    for i in 0..40u32 {
        add(400_000_000 + i * 2_000_000, 80 + i as u16, -1, |_| true);
    }
    app(&API, AppEvent::Enter);
    run(app, 60);
    let dump = bandsweep::debug_dump();
    check(bandsweep::debug_count() == 24 && dump.contains("hz=478000000"), "keeps the 24 strongest");
    for _ in 0..30 {
        press(app, 15);
    }
    shot("bs_8_full_list");
    check(sim().ptt_calls == 0 && sim().tx_enable_calls == 0, "never touched the transmitter");
    check(press(app, 13), "Exit leaves");
    app(&API, AppEvent::Leave);
}

/// What's on screen must match a from-scratch redraw, or something went stale.
fn fox_screen_ok(name: &str) {
    let app: Entry = foxhunt::entry;
    let before = sim().fb.clone();
    foxhunt::debug_force_redraw();
    app(&API, AppEvent::Draw);
    check(before == sim().fb, &format!("{name}: screen matches a full redraw"));
}

fn set_level(level: u16) {
    sim().signals[0].level = level;
}

fn test_fox() {
    let app: Entry = foxhunt::entry;
    const HZ: u32 = 146_520_000;
    println!("Fox Hunt: waiting");
    reset(HZ);
    add(HZ, 100, -1, |_| false);
    app(&API, AppEvent::Enter);
    app(&API, AppEvent::Draw);
    shot("fox_1_waiting");
    run(app, 5);
    check(!foxhunt::debug_present(), "noise alone doesn't count as a signal");
    press(app, 12);
    check(foxhunt::debug_notice() == 1, "calibrating with no signal is refused");
    shot("fox_2_needs_signal");

    println!("Fox Hunt: steady signal that never unkeys");
    reset(HZ);
    add(HZ, 100, -1, |_| true);
    app(&API, AppEvent::Enter);
    run(app, 120);
    check(foxhunt::debug_present(), "a weather-style steady signal stays a signal for 2 minutes");
    check((100..=104).contains(&foxhunt::debug_level()), "reads about -70 dBm");
    shot("fox_3_steady");
    fox_screen_ok("steady");

    println!("Fox Hunt: walking toward and away, between transmissions");
    reset(HZ);
    add(HZ, 90, -1, on_for(10, 4));
    app(&API, AppEvent::Enter);
    run(app, 9);
    check(foxhunt::debug_has_reference(), "remembers the last transmission");
    shot("fox_4_between");
    fox_screen_ok("between");
    set_level(102);
    run(app, 3);
    check(foxhunt::debug_present() && foxhunt::debug_trend() >= 8, "stronger next transmission reads as closer");
    shot("fox_5_closer");
    fox_screen_ok("closer");
    run(app, 7);
    set_level(90);
    run(app, 3);
    check(foxhunt::debug_present() && foxhunt::debug_trend() <= -8, "weaker next transmission reads as farther");
    shot("fox_6_farther");
    fox_screen_ok("farther");
    run(app, 7);
    set_level(90);
    run(app, 3);
    check(foxhunt::debug_trend().abs() < 3, "same strength reads as the same");

    println!("Fox Hunt: calibrating at 1.0 mile");
    reset(HZ);
    add(HZ, 100, -1, |_| true);
    app(&API, AppEvent::Enter);
    run(app, 3);
    fox_screen_ok("before calibrating");
    press(app, 12);
    press(app, 1);
    press(app, 0);
    shot("fox_7_typing");
    press(app, 12);
    check(foxhunt::debug_has_cal(), "calibration saved");
    run(app, 6);
    let near = |v: u64, want: u64| v.abs_diff(want) <= want / 4 + 1;
    let (lo, hi) = foxhunt::debug_range();
    println!("     same strength: {lo}-{hi} tenths of a mile");
    check(near(lo, 10) && near(hi, 10), "same strength is about the calibrated 1.0 mile");
    set_level(88);
    run(app, 11);
    let (lo, hi) = foxhunt::debug_range();
    println!("     12 dB weaker: {lo}-{hi} tenths of a mile");
    check(near(lo, 20) && near(hi, 40), "12 dB weaker is about 2-4 miles");
    shot("fox_8_estimate");
    fox_screen_ok("estimate");
    set_level(112);
    run(app, 11);
    let (lo, hi) = foxhunt::debug_range();
    println!("     12 dB stronger: {lo}-{hi} tenths of a mile");
    check(near(lo, 2) && near(hi, 5), "12 dB stronger is about a quarter to half a mile");
    set_level(30);
    let far = foxhunt::debug_range();
    check(far.0 <= far.1, "range stays in order");

    println!("Fox Hunt: calibrating right as a transmission starts");
    reset(HZ);
    add(HZ, 100, -1, |_| true);
    app(&API, AppEvent::Enter);
    run(app, 1);
    press(app, 12);
    press(app, 1);
    press(app, 0);
    press(app, 12);
    run(app, 3);
    let (lo, hi) = foxhunt::debug_range();
    println!("     {lo}-{hi} tenths of a mile");
    check(near(lo, 10) && near(hi, 10), "an early calibration still reads about 1.0 mile");
    set_level(30);

    println!("Fox Hunt: bad input and reset");
    press(app, 12);
    press(app, 12);
    check(foxhunt::debug_notice() == 2, "0 miles is refused");
    press(app, 0);
    check(!foxhunt::debug_has_cal(), "0 clears the calibration");

    println!("Fox Hunt: other frequencies");
    press(app, 5);
    for d in [1u8, 2, 1, 5, 0, 0] {
        press(app, d);
    }
    check(foxhunt::debug_hz() == 121_500_000 && sim().modulation == 1, "typed airband frequency listens in AM");
    press(app, 5);
    for d in [9u8, 9, 9, 0, 0, 0] {
        press(app, d);
    }
    check(foxhunt::debug_hz() == 121_500_000 && foxhunt::debug_notice() == 2, "out-of-range frequency is refused");
    shot("fox_9_bad_freq");
    press(app, 5);
    for d in [4u8, 4, 6, 0, 0, 0] {
        press(app, d);
    }
    check(sim().modulation == 0 && sim().cfg_rx == 446_000_000, "back to FM on 446.000");

    println!("Fox Hunt: audio");
    check(sim().holds_rx, "starts with normal receive audio");
    press(app, 1);
    check(!sim().holds_rx && !sim().af, "1 mutes");
    shot("fox_10_muted");
    fox_screen_ok("muted");
    let before = foxhunt::debug_level();
    let _ = before;
    run(app, 2);
    press(app, 1);
    check(sim().holds_rx, "1 turns it back on");

    println!("Fox Hunt: very close");
    reset(HZ);
    add(HZ, 150, -1, |_| true);
    app(&API, AppEvent::Enter);
    run(app, 3);
    shot("fox_11_too_strong");
    check(foxhunt::debug_level() >= 140, "very strong signal is flagged");

    check(sim().ptt_calls == 0 && sim().tx_enable_calls == 0, "never touched the transmitter");
    check(unsafe { TONE_ON_CALLS } == 0, "never called the tone function that keys the transmitter");
    check(press(app, 13), "Exit leaves");
    app(&API, AppEvent::Leave);
}

// menu list, using the firmware's real list.rs and backdrop.rs

struct Menu {
    rows: &'static [(&'static str, Option<&'static str>)],
    logo: Vec<u8>,
}

impl ui::list::ListSource for Menu {
    fn row_count(&mut self) -> usize {
        self.rows.len()
    }
    fn read_flash(&mut self, addr: u32, buf: &mut [u8]) {
        let off = (addr - flash_map::addr::BOOT_LOGO_ADDR) as usize;
        buf.copy_from_slice(&self.logo[off..off + buf.len()]);
    }
    fn label(&mut self, i: usize, w: &mut dyn core::fmt::Write) {
        let _ = w.write_str(self.rows[i].0);
    }
    fn value(&mut self, i: usize, w: &mut dyn core::fmt::Write) -> bool {
        match self.rows[i].1 {
            Some(v) => {
                let _ = w.write_str(v);
                true
            }
            None => false,
        }
    }
}

fn test_menus() {
    println!("Menus: firmware list code over the real logo");
    let logo = std::fs::read(std::env::args().nth(2).unwrap()).unwrap();
    assert_eq!(logo.len(), 160 * 128 * 2);
    const MODES: &[(&str, Option<&str>)] = &[
        ("ALL", Some("*")), ("WEATHER", None), ("POLICE FIRE EMS", None), ("FED & GOV", None), ("UTILITIES", None),
        ("RAILROAD", None), ("AIR TRAFFIC", None), ("REPEATERS", None), ("FRS", None), ("GMRS", None), ("MURS", None),
        ("FRS GMRS MURS", None), ("CUSTOM RANGE", None),
    ];
    const APPS: &[(&str, Option<&str>)] = &[
        ("SETTINGS", None), ("CHAN MGR", None), ("SEARCH/QT SCAN", None), ("SPECTRUM", None), ("BAND SWEEP", None),
        ("FOX HUNT", None), ("RPT FINDER", None), ("TEXT", None), ("FM", None),
    ];
    const RADIO: &[(&str, Option<&str>)] = &[
        ("SQL", Some("3")), ("STEP", Some("25.00")), ("TOT", Some("60S")), ("TDR", Some("OFF")), ("SAVE", Some("ON")),
        ("BCL", Some("OFF")), ("TXINH", Some("OFF")), ("W/N", Some("WIDE")), ("PWR", Some("HIGH")), ("R-CTC", Some("OFF")),
        ("T-CTC", Some("100.0Hz")),
    ];
    let lists: [(&str, &str, &'static [(&'static str, Option<&'static str>)], usize); 4] = [
        ("menu_1_mode", "MODE", MODES, 0),
        ("menu_2_mode_scrolled", "MODE", MODES, 8),
        ("menu_3_apps", "MENU", APPS, 8),
        ("menu_4_radio", "RADIO", RADIO, 10),
    ];
    for (name, title, rows, selected) in lists {
        reset(0);
        let mut menu = Menu { rows, logo: logo.clone() };
        let mut cache = ui::list::Cache::new();
        ui::list::draw_list(sim(), title, &mut menu, selected, false, &mut cache);
        shot(name);
        let red = sim().fb.iter().filter(|&&p| (p >> 11) > 2 && (p >> 5) & 0x3F < 4).count();
        // The 2026-11 redesign dropped the skull behind lists for legibility:
        // rows are plain theme background now, so almost no dim red remains.
        check(red < 200, &format!("{name}: no skull behind the list"));
        // Moving the cursor must repaint only two rows, and match a full draw.
        if selected > 0 {
            ui::list::draw_list(sim(), title, &mut menu, selected - 1, false, &mut cache);
            let moved = sim().fb.clone();
            ui::list::draw_list(sim(), title, &mut menu, selected, false, &mut cache);
            let back = sim().fb.clone();
            ui::list::draw_list(sim(), title, &mut menu, selected - 1, false, &mut ui::list::Cache::new());
            check(moved == sim().fb, &format!("{name}: cursor move matches a full redraw"));
            let mut fresh = ui::list::Cache::new();
            reset(0);
            ui::list::draw_list(sim(), title, &mut menu, selected, false, &mut fresh);
            check(back == sim().fb, &format!("{name}: moving back leaves nothing behind"));
        }
    }
    // A scroll can't leave the previous screen's bottom edge behind.
    reset(0);
    let mut menu = Menu { rows: MODES, logo: logo.clone() };
    let mut cache = ui::list::Cache::new();
    ui::list::draw_list(sim(), "MODE", &mut menu, 0, false, &mut cache);
    ui::list::draw_list(sim(), "MODE", &mut menu, 8, false, &mut cache);
    let scrolled = sim().fb.clone();
    reset(0);
    ui::list::draw_list(sim(), "MODE", &mut menu, 8, false, &mut ui::list::Cache::new());
    check(scrolled == sim().fb, "scrolling matches a full redraw");

    // Blank logo: text must still be readable rather than white on white.
    reset(0);
    let mut menu = Menu { rows: MODES, logo: vec![0xFF; 160 * 128 * 2] };
    ui::list::draw_list(sim(), "MODE", &mut menu, 3, false, &mut ui::list::Cache::new());
    shot("menu_5_blank_logo");
    let white = sim().fb.iter().filter(|&&p| p == 0xFFFF).count();
    check(white < 2000, "an erased logo shows black, not white");
}

// SEARCH QT: two parts in one package, with the firmware's Chain emulated.
static mut SQ_SEG: u8 = 0;
static mut SQ_LEAK: bool = false;
static mut SQ_CHAINS: u32 = 0;

fn sq(api: &Api, ev: AppEvent) -> AppResult {
    let r = if unsafe { SQ_SEG } == 0 { qtscan2::entry(api, ev) } else { search2::entry(api, ev) };
    match r {
        AppResult::Chain(seg) => {
            let s = sim();
            unsafe {
                // Real entering-RX doesn't clear the counter, so it must already be off.
                SQ_LEAK = SQ_LEAK || s.freq_scan_on || s.filter_on;
                SQ_SEG = seg;
                SQ_CHAINS += 1;
            }
            // stop_for_app: the chip sleeps, audio off, then the next part's Enter.
            s.holds_rx = false;
            s.af = false;
            s.tuned = 0;
            sq(api, AppEvent::Enter)
        }
        other => other,
    }
}

fn sq_open(master: u32) {
    reset(master);
    unsafe {
        SQ_SEG = 0;
        SQ_LEAK = false;
        SQ_CHAINS = 0;
    }
    sq(&API, AppEvent::Enter);
    sq(&API, AppEvent::Draw);
}

const KEY_ONE: u8 = 1;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_AB: u8 = 17;
const FRS: u32 = 462_562_500;

fn test_search_qt() {
    let app: Entry = sq;

    println!("SEARCH QT: opens on QT Scan");
    sq_open(OUT);
    shot("sq_1_qt_waiting");
    check(unsafe { SQ_SEG } == 0, "opens on QT Scan");
    check(sim().holds_rx && sim().tuned == OUT, "listens on the current channel");
    check(!press(app, KEY_MENU), "Menu with nothing found does nothing");
    check(sim().sub_saved.is_none(), "and saves nothing");

    println!("SEARCH QT: QT Scan behaves as before");
    add(OUT, 120, IDX_100, |_| true);
    run(app, 1);
    shot("sq_2_qt_found");
    check(qtscan2::debug_found() == IDX_100, "reads 100.0 on the channel");
    press(app, KEY_UP);
    check(qtscan2::debug_found() == -1, "Up starts over");
    run(app, 1);
    check(qtscan2::debug_found() == IDX_100, "and reads it again");
    check(press(app, KEY_MENU), "Menu saves and exits");
    check(sim().sub_saved == Some((1000, true)), "saved 100.0 to receive and transmit");
    check(!sim().filter_on, "tone filter off after leaving");

    println!("SEARCH QT: switching to Search");
    sq_open(OUT);
    check(!press(app, KEY_ONE), "1 doesn't leave the app");
    check(unsafe { SQ_SEG } == 1, "1 switches to Search");
    let switched = sim().fb.clone();
    shot("sq_3_search_hunting");
    reset(OUT);
    unsafe { SQ_SEG = 1 };
    sq(&API, AppEvent::Enter);
    sq(&API, AppEvent::Draw);
    check(switched == sim().fb, "nothing left on screen from QT Scan");

    println!("SEARCH QT: Search behaves as before");
    sq_open(OUT);
    press(app, KEY_ONE);
    check(!press(app, KEY_MENU), "Menu while hunting does nothing");
    run(app, 1);
    check(sim().freq_scan_on, "frequency counter running");
    check(search2::debug_found().is_none(), "nothing on the air, still hunting");
    add(FRS, 120, IDX_131_8, |_| true);
    run(app, 1);
    shot("sq_4_search_found");
    check(search2::debug_found() == Some((FRS, IDX_131_8)), "found 462.5625 with 131.8");
    check(sim().tuned == FRS, "tuned to what it found");
    check(press(app, KEY_MENU), "Menu saves and exits");
    check(sim().vfo_saved == Some((FRS, 1318)), "saved frequency and tone to the VFO");
    check(!sim().freq_scan_on && !sim().filter_on, "counter and filter off after leaving");

    println!("SEARCH QT: Search bands and no-tone signals");
    sq_open(OUT);
    press(app, KEY_ONE);
    add(OUT, 120, -1, |_| true);
    run(app, 2);
    check(search2::debug_found().is_none(), "a VHF signal isn't reported on the UHF band");
    press(app, KEY_AB);
    run(app, 2);
    shot("sq_5_search_no_tone");
    check(search2::debug_found() == Some((OUT, -1)), "A/B to VHF finds 146.940, no tone");
    check(press(app, KEY_MENU), "Menu saves and exits");
    check(sim().vfo_saved == Some((OUT, 0)), "saved with no tone");

    println!("SEARCH QT: switching back at every stage");
    // While hunting
    sq_open(OUT);
    press(app, KEY_ONE);
    run(app, 1);
    press(app, KEY_ONE);
    check(unsafe { SQ_SEG } == 0 && sim().tuned == OUT && sim().holds_rx, "from hunting: back on the channel");
    check(!sim().freq_scan_on, "from hunting: counter stopped");
    run(app, 1);
    check(!sim().freq_scan_on && qtscan2::debug_found() == -1, "from hunting: QT Scan starts clean");
    // After Search found something on another frequency
    sq_open(OUT);
    press(app, KEY_ONE);
    add(FRS, 120, IDX_131_8, |_| true);
    run(app, 1);
    check(search2::debug_found().is_some() && sim().tuned == FRS, "found something off channel");
    press(app, KEY_ONE);
    check(sim().tuned == OUT, "from found: back on your channel, not the found one");
    sq(&API, AppEvent::Draw);
    shot("sq_6_back_on_qt");
    // While Search listens for a tone
    sq_open(OUT);
    press(app, KEY_ONE);
    add(FRS, 120, -1, |_| true);
    let mut waited = 0;
    while !sim().filter_on && waited < 2000 {
        sq(&API, AppEvent::Tick { dt_100us: 50 });
        sim().now = sim().now.wrapping_add(50);
        waited += 1;
    }
    check(sim().filter_on, "reached the tone check");
    press(app, KEY_ONE);
    check(!sim().filter_on && sim().tuned == OUT, "from the tone check: filter off, back on the channel");
    // While QT Scan is detecting a tone
    sq_open(OUT);
    add(OUT, 120, -1, |_| true);
    run(app, 1);
    check(sim().filter_on, "QT Scan is detecting");
    press(app, KEY_ONE);
    check(unsafe { SQ_SEG } == 1, "from detecting: switched to Search");
    check(unsafe { !SQ_LEAK }, "the counter and filter were never left on across a switch");

    println!("SEARCH QT: Search's band resets, keys, repeated switching");
    sq_open(OUT);
    press(app, KEY_ONE);
    hold(app, KEY_ONE);
    check(unsafe { SQ_SEG } == 1, "a long press doesn't switch");
    for _ in 0..41 {
        press(app, KEY_ONE);
        run(app, 1);
    }
    check(unsafe { SQ_SEG } == 0 && unsafe { SQ_CHAINS } == 42, "42 switches land where expected");
    check(sim().tuned == OUT && sim().holds_rx, "still on the channel");
    check(press(app, KEY_EXIT), "Exit leaves QT Scan");
    sq_open(OUT);
    press(app, KEY_ONE);
    run(app, 1);
    check(press(app, KEY_EXIT), "Exit leaves Search");
    check(!sim().freq_scan_on && !sim().filter_on, "counter and filter off after Exit");
    check(sim().ptt_calls == 0 && sim().tx_enable_calls == 0, "never transmits");
}

// ---- simulated RF chip registers and FSK modem, for TEXT ----

#[derive(Clone)]
enum AirEvent {
    Sync,
    Word(u16),
    Burst(Vec<u16>),
    Done,
}

struct Chip {
    regs: [u16; 128],
    pending: u16,
    latched: u16,
    tx_fifo: Vec<u16>,
    rx_fifo: std::collections::VecDeque<u16>,
    script: std::collections::VecDeque<AirEvent>,
    aired: Vec<Vec<u16>>,
    aired_ctrl: Vec<(u16, u16, u16, u16, bool)>,
    tx_never_done: bool,
    tx_without_key: u32,
    fsk_writes: Vec<u16>,
    tx_key_time: u32,
    state_writes: Vec<u16>,
    tones: Vec<u16>,
}

static mut CHIP: Option<Chip> = None;

fn chip() -> &'static mut Chip {
    unsafe { CHIP.as_mut().unwrap() }
}

fn chip_reset() {
    let mut regs = [0u16; 128];
    regs[0x59] = 0x0028;
    regs[0x2B] = 0x0300;
    regs[0x40] = 0x34D0;
    regs[0x72] = 0x1111;
    unsafe {
        CHIP = Some(Chip {
            regs,
            pending: 0,
            latched: 0,
            tx_fifo: Vec::new(),
            rx_fifo: Default::default(),
            script: Default::default(),
            aired: Vec::new(),
            aired_ctrl: Vec::new(),
            tx_never_done: false,
            tx_without_key: 0,
            fsk_writes: Vec::new(),
            tx_key_time: 0,
            state_writes: Vec::new(),
            tones: Vec::new(),
        });
    }
}

pub(crate) fn sim_rf_write(addr: u8, value: u16) {
    let c = chip();
    match addr {
        0x5F => c.tx_fifo.push(value),
        0x02 => {
            c.latched = c.pending;
            c.pending = 0;
        }
        0x59 => {
            if value & 0x8000 != 0 {
                c.tx_fifo.clear();
            }
            if value & 0x4000 != 0 {
                c.rx_fifo.clear();
                // A restarted receiver waits for the next sync, so the noise it was decoding is gone.
                while matches!(c.script.front(), Some(e) if !matches!(e, AirEvent::Sync)) {
                    c.script.pop_front();
                }
            }
            if value & 0x0800 != 0 {
                if !sim().transmitting {
                    c.tx_without_key += 1;
                }
                let words = std::mem::take(&mut c.tx_fifo);
                c.aired_ctrl.push((c.regs[0x58], c.regs[0x72], c.regs[0x2B], c.regs[0x40], sim().transmitting));
                c.aired.push(words);
                // A packet's real time on the air at 450 baud.
                if !c.tx_never_done {
                    sim().now = sim().now.wrapping_add(9_400);
                }
                if !c.tx_never_done && c.regs[0x3F] & 0x8000 != 0 {
                    c.pending |= 0x8000;
                }
            }
            c.regs[0x59] = value & 0x3FFF;
        }
        0x30 => {
            c.state_writes.push(value);
            c.regs[0x30] = value;
        }
        0x71 => {
            if value != 0 {
                c.tones.push(value);
            }
            c.regs[0x71] = value;
        }
        0x58 => {
            c.fsk_writes.push(value);
            c.regs[0x58] = value;
        }
        _ => c.regs[addr as usize] = value,
    }
}

pub(crate) fn sim_rf_read(addr: u8) -> u16 {
    let c = chip();
    match addr {
        0x0C => {
            // The modem only hears anything with RX on.
            if c.pending == 0 && c.regs[0x59] & 0x1000 != 0 {
                match c.script.pop_front() {
                    Some(AirEvent::Sync) => c.pending |= 2,
                    Some(AirEvent::Word(w)) => {
                        c.rx_fifo.push_back(w);
                        c.pending |= 0x1000;
                        // A word's real time on the air at 450 baud.
                        sim().now = sim().now.wrapping_add(356);
                    }
                    Some(AirEvent::Burst(words)) => c.rx_fifo.extend(words),
                    Some(AirEvent::Done) => c.pending |= 0x2000,
                    None => {}
                }
            }
            (c.pending != 0) as u16
        }
        0x02 => c.latched,
        0x5F => c.rx_fifo.pop_front().unwrap_or(0),
        _ => c.regs[addr as usize],
    }
}

fn air_packet(words: &[u16]) {
    let c = chip();
    c.script.push_back(AirEvent::Sync);
    for &w in words {
        c.script.push_back(AirEvent::Word(w));
    }
    // Seen on the real chip: no done flag, and noise keeps arriving as words.
    for i in 0..73u16 {
        c.script.push_back(AirEvent::Word(i.wrapping_mul(0x9E37)));
    }
}

fn words_of(plain: &[u8; 32]) -> Vec<u16> {
    let mut p = *plain;
    textapp::debug_scramble(&mut p);
    p.chunks_exact(2).map(|b| ((b[1] as u16) << 8) | b[0] as u16).collect()
}

fn make_packet(from: u8, id: u8, part: u8, total: u8, text: &[u8]) -> [u8; 32] {
    let mut p = [0u8; 32];
    p[0] = 0xB5;
    p[1] = from;
    p[2] = id;
    p[3] = (part << 4) | total;
    p[4..4 + text.len()].copy_from_slice(text);
    textapp::debug_sign(&mut p);
    p
}

fn text_ok_screen(name: &str) {
    let before = sim().fb.clone();
    textapp::debug_force_redraw();
    textapp::entry(&API, AppEvent::Draw);
    check(before == sim().fb, &format!("{name}: screen matches a full redraw"));
}

fn tap(app: Entry, digit: u8, times: u32) {
    for _ in 0..times {
        press(app, digit);
    }
}

fn text_open() {
    nor_clear();
    text_reopen();
}

/// Opens the app again without wiping what it saved.
fn text_reopen() {
    reset(OUT);
    chip_reset();
    textapp::entry(&API, AppEvent::Enter);
    textapp::entry(&API, AppEvent::Draw);
}

const RECEIVED: u8 = 0;
const SENDING: u8 = 3;
const DELIVERED: u8 = 1;
const FAILED: u8 = 2;

fn plain_of(words: &[u16]) -> [u8; 32] {
    let mut p = [0u8; 32];
    for (i, w) in words.iter().enumerate() {
        p[i * 2] = *w as u8;
        p[i * 2 + 1] = (*w >> 8) as u8;
    }
    textapp::debug_scramble(&mut p);
    p
}

fn make_reply(from: u8, to: u8, id: u8) -> [u8; 32] {
    let mut p = [0u8; 32];
    p[0] = 0xB5;
    p[1] = from;
    p[4] = to;
    p[5] = id;
    textapp::debug_sign(&mut p);
    p
}

/// Ticks and draws for a number of milliseconds of radio time.
fn run_ms(app: Entry, ms: u32) {
    let end = sim().now.wrapping_add(ms * 10);
    let mut last = sim().now;
    while sim().now.wrapping_sub(end) > u32::MAX / 2 {
        sim().now = sim().now.wrapping_add(50);
        let dt = sim().now.wrapping_sub(last);
        last = sim().now;
        app(&API, AppEvent::Tick { dt_100us: dt });
        app(&API, AppEvent::Draw);
    }
}

/// Like run_ms, but the first tick's dt counts from , as the real main loop's does.
fn run_span(app: Entry, last: u32, ms: u32) {
    let end = sim().now.wrapping_add(ms * 10);
    let mut last = last;
    while sim().now.wrapping_sub(end) > u32::MAX / 2 {
        sim().now = sim().now.wrapping_add(50);
        let dt = sim().now.wrapping_sub(last);
        last = sim().now;
        app(&API, AppEvent::Tick { dt_100us: dt });
        app(&API, AppEvent::Draw);
    }
}

fn type_text(app: Entry, s: &[u8]) {
    for &c in s {
        let (digit, taps) = match c {
            b' ' => (0, 1),
            b'A'..=b'O' => ((c - b'A') / 3 + 2, (c - b'A') % 3 + 1),
            b'P'..=b'S' => (7, c - b'P' + 1),
            b'T'..=b'V' => (8, c - b'T' + 1),
            _ => (9, c - b'W' + 1),
        };
        tap(app, digit, taps as u32);
        run(app, 2);
    }
}

fn test_text() {
    let app: Entry = textapp::entry;

    println!("TEXT: opening");
    text_open();
    shot("txt_1_open");
    let c = chip();
    check(sim().tuned == OUT && !sim().holds_rx && !sim().af, "listens on the current frequency with the speaker muted");
    check(!sim().tx_enabled && sim().ptt_calls == 0, "opening never transmits");
    check(c.regs[0x58] == 0x0301 && c.regs[0x72] == 4646, "direct FM at 450 baud, the best setting measured");
    check(c.regs[0x5A] == 0x3072 && c.regs[0x5B] == 0x576C && c.regs[0x5C] == 0x5625, "sync words and no chip CRC");
    check(c.regs[0x5D] == 34 << 8 && c.regs[0x5E] == (64 << 3) | 1, "RX size and FIFO threshold");
    check(c.regs[0x59] == 0x1008 && c.regs[0x3F] == 0x3002, "receive is on, with its three interrupts");
    run(app, 5);
    check(c.aired.is_empty() && sim().ptt_calls == 0, "sitting idle never transmits");

    println!("TEXT: typing");
    tap(app, 4, 2);
    run(app, 2);
    tap(app, 4, 3);
    check(textapp::debug_comp() == "HI", "two taps then three taps of 4, with a pause, types HI");
    shot("txt_2_typing");
    press(app, 13);
    check(textapp::debug_comp() == "H", "Exit deletes a letter and stays open");
    tap(app, 4, 1);
    check(textapp::debug_comp() == "HG", "the delete ends the tap sequence, so 4 starts a new letter");
    press(app, 13);
    run(app, 2);
    tap(app, 2, 5);
    check(textapp::debug_comp() == "HA", "tapping past the end of a key's letters wraps around");
    press(app, 13);
    press(app, 13);
    check(textapp::debug_comp().is_empty(), "deleted back to empty");
    press(app, 12);
    check(chip().aired.is_empty() && textapp::debug_notice().contains("TYPE"), "Menu with nothing typed doesn't transmit");

    println!("TEXT: first try of a message");
    tap(app, 4, 2);
    run(app, 2);
    tap(app, 4, 3);
    press(app, 12);
    let c = chip();
    check(c.aired.len() == 1 && c.aired[0].len() == 16, "one 32-byte packet on the air");
    check(c.tx_without_key == 0, "the modem only ran while keyed");
    let (fsk, baud, filter, dev, _) = c.aired_ctrl[0];
    check(fsk == 0x0301 && baud == 4646, "sent with the right settings");
    check(filter == 0x0005 && dev == 0x3000 | 1300, "TX filter and deviation set during the packet");
    check(c.regs[0x2B] == 0x0300 && c.regs[0x40] == 0x34D0, "filter and deviation restored after");
    check(!sim().transmitting && !sim().tx_enabled && sim().ptt_calls == 2, "keyed once, released, and TX locked again");
    check(c.regs[0x59] == 0x1008 && c.regs[0x5D] == 34 << 8 && c.regs[0x3F] == 0x3002, "back to listening for the reply");
    check(sim().power == 1, "low power by default");
    check(textapp::debug_comp().is_empty(), "the typed message clears");
    check(textapp::debug_msgs() == vec![(SENDING, "HI".to_string())], "it shows as still sending");
    check(textapp::debug_notice().is_empty(), "an old notice doesn't cover the try count");
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_3_sending");
    text_ok_screen("sending");
    let first = plain_of(&chip().aired[0]);
    check(first[0] == 0xB5 && first[3] == 0x01 && &first[4..6] == b"HI" && first[6] == 0, "the packet holds the message");
    let (me, id) = (first[1], first[2]);
    let w = chip().aired[0].clone();
    let bits: Vec<bool> = w.iter().flat_map(|&x| (0..16).map(move |i| x & (1 << i) != 0)).collect();
    let mut longest = 0;
    let mut run_len = 0;
    for pair in bits.windows(2) {
        run_len = if pair[0] == pair[1] { run_len + 1 } else { 0 };
        longest = longest.max(run_len + 1);
    }
    check(longest <= 12, "scrambling leaves no long run of identical bits");

    println!("TEXT: the reply");
    run_ms(app, 1500);
    check(chip().aired.len() == 1, "no retry while a reply could still be on its way");
    air_packet(&words_of(&make_reply(7, me, id)));
    run_ms(app, 500);
    check(textapp::debug_msgs() == vec![(DELIVERED, "HI".to_string())], "a reply marks it delivered");
    check(textapp::debug_notice().contains("DELIVERED"), "and says so");
    check(!textapp::debug_pending().0, "and nothing is waiting");
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_4_delivered");
    text_ok_screen("delivered");
    run(app, 30);
    check(chip().aired.len() == 1, "no retries after that");
    air_packet(&words_of(&make_reply(7, me, id)));
    run(app, 1);
    check(textapp::debug_msgs() == vec![(DELIVERED, "HI".to_string())] && chip().aired.len() == 1, "a late second reply changes nothing and isn't answered");

    println!("TEXT: retrying");
    text_open();
    press(app, 10);
    let first = chip().aired[0].clone();
    let plain = plain_of(&first);
    let (me, id) = (plain[1], plain[2]);
    check(&plain[4..10] == b"PING 1", "* sends a numbered ping the same way");
    run(app, 4);
    check(chip().aired.len() == 2 && chip().aired[1] == first, "no reply: the same packet goes again");
    check(textapp::debug_pending() == (true, 2), "on the second try");
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_5_retrying");
    air_packet(&words_of(&make_reply(7, me, id.wrapping_add(1))));
    run_ms(app, 500);
    check(textapp::debug_pending().0, "a reply to a different message doesn't count");
    air_packet(&words_of(&make_reply(7, me.wrapping_add(1), id)));
    run_ms(app, 500);
    check(textapp::debug_pending().0, "nor one meant for a different radio");
    press(app, 10);
    check(textapp::debug_notice().contains("WAIT") && textapp::debug_pending().0, "a second send waits for the first to finish");
    tap(app, 2, 1);
    press(app, 12);
    check(textapp::debug_comp() == "A", "and what you typed stays");
    air_packet(&words_of(&make_reply(7, me, id)));
    run_ms(app, 500);
    check(textapp::debug_msgs()[0].0 == DELIVERED, "a reply after a retry still counts");
    press(app, 12);
    check(textapp::debug_msgs().len() == 2, "and the next message can go");

    println!("TEXT: no reply at all");
    text_open();
    press(app, 10);
    run(app, 60);
    check(chip().aired.len() == 8, "stops after eight tries");
    check(textapp::debug_msgs()[0].0 == FAILED, "marks it failed");
    check(textapp::debug_notice().contains("NOT CONFIRMED") || !textapp::debug_pending().0, "and stops waiting");
    check(!sim().transmitting && !sim().tx_enabled && sim().ptt_calls == 16, "unkeyed after each try");
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_6_failed");
    text_ok_screen("failed");
    run(app, 30);
    check(chip().aired.len() == 8, "and no more go out");
    press(app, 10);
    check(chip().aired.len() == 9, "a new message can still go");
    check(!press(app, 13), "Exit during the wait doesn't leave the app");
    check(textapp::debug_msgs()[1].0 == FAILED && textapp::debug_notice().contains("STOPPED"), "it stops the tries");
    run(app, 30);
    check(chip().aired.len() == 9, "and none follow");

    println!("TEXT: timing");
    // Like the real main loop, the tick's dt includes however long the last tick blocked.
    text_open();
    let start = sim().now;
    air_packet(&words_of(&make_packet(9, 42, 0, 1, b"TIMING")));
    run_span(app, start, 3000);
    check(chip().aired.len() == 1, "one reply went out");
    let reply_ms = chip().tx_key_time.wrapping_sub(start) / 10;
    let packet_ms = 16 * 356 / 10;
    println!("     reply keyed {} ms after the packet ended", reply_ms as i32 - packet_ms);
    check(reply_ms as i32 - packet_ms >= 200 && reply_ms as i32 - packet_ms <= 400, "with a quiet channel the reply waits its minimum");
    text_open();
    let start = sim().now;
    press(app, 10);
    let ended = sim().now;
    let sent = chip().aired.len();
    check(sent == 1, "first try sent");
    run_span(app, start, 3000);
    check(chip().aired.len() == 2, "second try sent");
    let retry_ms = chip().tx_key_time.wrapping_sub(ended) / 10;
    println!("     retry keyed {retry_ms} ms after the first try ended");
    check(retry_ms >= 1650 && retry_ms <= 2300, "a retry leaves time for a whole reply, however long the send took");

    println!("TEXT: listening before talking");
    text_open();
    air_packet(&words_of(&make_packet(9, 60, 0, 1, b"HOLD")));
    add(OUT, 120, -1, |_| true);
    run_ms(app, 1000);
    check(textapp::debug_got() == 1 && chip().aired.is_empty() && !sim().transmitting, "no reply while the sender is still keyed, as with STE on");
    sim().signals.clear();
    run_ms(app, 60);
    check(chip().aired.is_empty(), "nor the moment its carrier stops");
    run_ms(app, 200);
    check(chip().aired.len() == 1, "the reply goes once the channel has stayed quiet");
    text_open();
    air_packet(&words_of(&make_packet(9, 61, 0, 1, b"STUCK")));
    add(OUT, 120, -1, |_| true);
    run(app, 6);
    check(textapp::debug_got() == 1 && chip().aired.is_empty(), "with a carrier that never stops, the reply is dropped rather than talked over it");
    sim().signals.clear();
    run(app, 2);
    check(chip().aired.is_empty(), "and doesn't go out later on its own");
    air_packet(&words_of(&make_packet(9, 61, 0, 1, b"STUCK")));
    run(app, 1);
    check(chip().aired.len() == 1 && textapp::debug_got() == 1, "the sender's next try gets it");
    text_open();
    press(app, 10);
    add(OUT, 120, -1, |_| true);
    run(app, 4);
    check(chip().aired.len() == 1, "a retry waits while someone is on the channel");
    sim().signals.clear();
    run(app, 1);
    check(chip().aired.len() == 2, "and goes when they stop");
    text_open();
    press(app, 10);
    add(OUT, 120, -1, |_| true);
    run(app, 9);
    check(chip().aired.len() == 2, "a stuck carrier can't hold the tries up forever");
    sim().signals.clear();

    println!("TEXT: receiving");
    text_open();
    let w = words_of(&make_packet(9, 42, 0, 1, b"HELLO"));
    air_packet(&w);
    run_ms(app, 100);
    check(textapp::debug_msgs() == vec![(RECEIVED, "HELLO".to_string())], "a message arrives");
    check(textapp::debug_got() == 1 && textapp::debug_counts() == (1, 1, 0, 0), "counted once");
    check(chip().aired.is_empty() && !sim().transmitting, "the reply waits for the sender to finish");
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_7_received");
    text_ok_screen("received");
    run_ms(app, 500);
    check(chip().aired.len() == 1, "then one reply goes out");
    let r = plain_of(&chip().aired[0]);
    check(r[0] == 0xB5 && r[3] == 0 && r[4] == 9 && r[5] == 42, "addressed to the sender, for that message");
    check(!sim().transmitting && !sim().tx_enabled && chip().regs[0x59] == 0x1008, "then unkeys and listens");
    check(chip().regs[0x2B] == 0x0300 && chip().regs[0x40] == 0x34D0, "filter and deviation restored");
    run(app, 5);
    check(chip().aired.len() == 1, "and sends nothing more");
    air_packet(&w);
    run(app, 1);
    check(textapp::debug_msgs().len() == 1 && textapp::debug_got() == 1, "a repeat isn't shown twice");
    check(chip().aired.len() == 2, "but is answered, since the last reply must have been missed");
    let other = words_of(&make_packet(9, 43, 0, 1, b"AGAIN"));
    air_packet(&other);
    run(app, 1);
    check(textapp::debug_msgs().len() == 2 && textapp::debug_got() == 2, "the next message from the same radio arrives");
    check(plain_of(chip().aired.last().unwrap())[5] == 43, "and gets its own reply");
    let same = words_of(&make_packet(5, 43, 0, 1, b"FRIEND"));
    air_packet(&same);
    run(app, 1);
    check(textapp::debug_got() == 3, "a different radio's message with the same number isn't mistaken for a repeat");
    sim().tx_block = true;
    air_packet(&words_of(&make_packet(5, 50, 0, 1, b"LOCKED")));
    run(app, 1);
    check(textapp::debug_got() == 4 && textapp::debug_notice().contains("REPLY"), "with transmit locked it still shows, and says it can't reply");
    sim().tx_block = false;

    println!("TEXT: long messages");
    text_open();
    let long = b"MEET AT THE NORTH GATE AT DAWN BRING WATER AND RADIOS";
    let typed = &long[..50];
    type_text(app, typed);
    check(textapp::debug_comp().as_bytes() == typed, "typed 50 characters exactly");
    shot("txt_8_full");
    tap(app, 2, 1);
    check(textapp::debug_comp().as_bytes() == typed && textapp::debug_notice().contains("FULL"), "a 51st character is refused");
    press(app, 12);
    check(chip().aired.len() == 2 && sim().ptt_calls == 2, "50 characters take two packets on one key-up");
    run(app, 5);
    check(chip().aired.len() == 4 && chip().aired[2] == chip().aired[0] && chip().aired[3] == chip().aired[1], "a retry sends both again");
    let (a, b) = (chip().aired[0].clone(), chip().aired[1].clone());
    let (from, id) = (plain_of(&a)[1], plain_of(&a)[2]);
    text_open();
    air_packet(&a);
    air_packet(&b);
    run_ms(app, 200);
    check(textapp::debug_msgs()[0].1.as_bytes() == typed, "both halves join into the original");
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_9_long_received");
    text_ok_screen("long");
    run(app, 1);
    check(chip().aired.len() == 1 && plain_of(&chip().aired[0])[5] == id, "and one reply goes out");
    air_packet(&a);
    run(app, 1);
    check(chip().aired.len() == 1 && textapp::debug_got() == 1, "the first half of a repeat doesn't get a reply, since the second half is still on the air");
    air_packet(&b);
    run(app, 1);
    check(chip().aired.len() == 2 && textapp::debug_got() == 1, "its last half does");

    println!("TEXT: long messages with a piece missing");
    text_open();
    air_packet(&a);
    run(app, 5);
    check(textapp::debug_msgs().is_empty() && chip().aired.is_empty(), "half a message isn't shown or answered");
    air_packet(&b);
    run_ms(app, 200);
    check(textapp::debug_msgs()[0].1.as_bytes() == typed, "the other half from a later try completes it");
    check(textapp::debug_counts().2 == 0, "with nothing counted as bad");
    text_open();
    air_packet(&b);
    run(app, 5);
    check(textapp::debug_msgs().is_empty(), "the same when only the second half came first");
    air_packet(&a);
    run_ms(app, 200);
    check(textapp::debug_msgs()[0].1.as_bytes() == typed, "it joins in the right order");
    run_ms(app, 500);
    check(chip().aired.is_empty(), "and the reply waits, since the second half is still on the air");
    run_ms(app, 1000);
    check(chip().aired.len() == 1, "then goes out");
    text_open();
    air_packet(&a);
    run(app, 45);
    air_packet(&b);
    run(app, 1);
    check(textapp::debug_msgs().is_empty(), "a piece that waited too long is dropped rather than joined");
    air_packet(&words_of(&make_packet(from.wrapping_add(1), 7, 0, 1, b"OTHER")));
    run(app, 1);
    check(textapp::debug_msgs().len() == 1 && textapp::debug_msgs()[0].1 == "OTHER", "and other messages still get through");

    println!("TEXT: damage on the air");
    text_open();
    let good = words_of(&make_packet(9, 6, 0, 1, b"SLOW TICK"));
    let mut bad = good.clone();
    bad[3] ^= 0x0100;
    air_packet(&bad);
    run(app, 1);
    check(textapp::debug_msgs().is_empty() && textapp::debug_counts().2 == 1, "a flipped bit fails the checksum and isn't shown");
    check(chip().aired.is_empty(), "and gets no reply");
    for bad_head in [make_packet(9, 2, 3, 2, b"X"), make_packet(9, 3, 0, 7, b"X"), make_packet(9, 4, 5, 0, b"X")] {
        air_packet(&words_of(&bad_head));
    }
    run(app, 2);
    check(textapp::debug_msgs().is_empty() && textapp::debug_counts().2 == 4, "impossible part numbers are refused, even with a good checksum");
    let mut odd = make_packet(9, 5, 0, 1, b"A");
    odd[5] = 0xC8;
    odd[6] = 0x07;
    textapp::debug_sign(&mut odd);
    air_packet(&words_of(&odd));
    run_ms(app, 100);
    textapp::entry(&API, AppEvent::Draw);
    check(textapp::debug_msgs()[0].1 == "A??", "characters the screen can't show become question marks");
    text_open();
    chip().script.push_back(AirEvent::Sync);
    chip().script.push_back(AirEvent::Word(good[0]));
    chip().script.push_back(AirEvent::Burst(good[1..].to_vec()));
    chip().script.push_back(AirEvent::Done);
    run_ms(app, 100);
    check(textapp::debug_msgs().first().map(|m| m.1.as_str()) == Some("SLOW TICK"), "words a slow tick missed are read when a done flag comes");
    text_open();
    chip().script.push_back(AirEvent::Sync);
    run(app, 3);
    check(textapp::debug_counts().2 == 1 && chip().regs[0x59] == 0x1008, "a false sync times out and receive carries on");
    text_ok_screen("after a false sync");
    text_open();
    chip().script.push_back(AirEvent::Sync);
    chip().script.push_back(AirEvent::Word(0x1234));
    run(app, 2);
    check(textapp::debug_counts().2 == 1, "a packet cut off partway counts as bad");
    air_packet(&good);
    run_ms(app, 100);
    check(textapp::debug_msgs().len() == 1, "and the next packet still arrives");
    text_open();
    let mut real = [0u8; 32];
    real[..13].copy_from_slice(&[0xB5, 0x0A, 0x25, 0x01, 0x50, 0x49, 0x4E, 0x47, 0x20, 0x31, 0, 0, 0]);
    textapp::debug_sign(&mut real);
    air_packet(&words_of(&real));
    run_ms(app, 100);
    check(textapp::debug_msgs() == vec![(RECEIVED, "PING 1".to_string())], "the bytes seen on the air read as PING 1");
    check(textapp::debug_counts() == (1, 1, 0, 0), "one good packet, despite the noise after it");
    check(chip().regs[0x59] == 0x1008, "and it's listening again");
    text_open();
    let w = words_of(&make_packet(4, 9, 0, 1, b"WITH DONE FLAG"));
    let c = chip();
    c.script.push_back(AirEvent::Sync);
    for &x in &w {
        c.script.push_back(AirEvent::Word(x));
    }
    c.script.push_back(AirEvent::Done);
    run_ms(app, 100);
    check(textapp::debug_msgs().len() == 1 && textapp::debug_counts().2 == 0, "a done flag, if one ever comes, does no harm");
    text_open();
    let x = words_of(&make_packet(3, 1, 0, 1, b"X"));
    for &v in &x {
        chip().script.push_back(AirEvent::Word(v));
    }
    run(app, 1);
    check(textapp::debug_counts().0 == 0 && textapp::debug_msgs().is_empty(), "words with no sync before them aren't treated as a message");

    println!("TEXT: both radios talking");
    text_open();
    press(app, 10);
    let plain = plain_of(&chip().aired[0]);
    let (me, id) = (plain[1], plain[2]);
    air_packet(&words_of(&make_packet(9, 70, 0, 1, b"CROSSED")));
    run_ms(app, 200);
    check(textapp::debug_got() == 1, "a message arrives while waiting for a reply");
    run_ms(app, 500);
    check(chip().aired.len() == 2 && plain_of(&chip().aired[1])[3] == 0, "and is answered first");
    check(textapp::debug_pending().0, "while still waiting on its own message");
    air_packet(&words_of(&make_reply(9, me, id)));
    run_ms(app, 500);
    check(textapp::debug_msgs()[0].0 == DELIVERED, "whose reply can still arrive");
    check(chip().regs[0x59] == 0x1008 && !sim().transmitting, "and it ends listening");
    text_open();
    press(app, 10);
    run(app, 3);
    let n = chip().aired.len();
    air_packet(&words_of(&make_packet(9, 71, 0, 1, b"LATE")));
    run(app, 1);
    check(plain_of(&chip().aired[n])[3] == 0, "a due retry waits for a reply going out");

    println!("TEXT: power and message history");
    text_open();
    press(app, 17);
    press(app, 10);
    check(sim().power == 0, "A/B switches to high power");
    press(app, 13);
    press(app, 17);
    press(app, 10);
    check(sim().power == 1, "and back to low");
    press(app, 13);
    check(chip().regs[0x58] == 0x0301 && chip().regs[0x72] == 4646, "# no longer changes anything");
    press(app, 11);
    check(chip().regs[0x58] == 0x0301 && chip().regs[0x72] == 4646 && !textapp::debug_pending().0, "really");
    for i in 0..4 {
        air_packet(&words_of(&make_packet(7, 100 + i, 0, 1, format!("OLD {i}").as_bytes())));
        run(app, 1);
    }
    let m = textapp::debug_msgs();
    check(m.len() == 6 && m[5].1 == "OLD 3" && m[0].1 == "PING 1", "keeps them in order");
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_10_history");
    for _ in 0..6 {
        press(app, 14);
    }
    text_ok_screen("scrolled");
    press(app, 15);
    text_ok_screen("scrolled down");

    println!("TEXT: tones");
    text_open();
    let turned = unsafe { SPEAKER_TURN_ONS };
    run(app, 3);
    check(unsafe { SPEAKER_TURN_ONS } == turned && chip().tones.is_empty(), "no tone while nothing is happening");
    air_packet(&words_of(&make_packet(9, 1, 0, 1, b"RING")));
    run(app, 2);
    check(unsafe { SPEAKER_TURN_ONS } == turned + 1, "a new message turns the speaker on once");
    check(chip().tones.len() == 2 && chip().tones[0] < chip().tones[1], "for two rising notes");
    check(unsafe { !SPEAKER_ON } && chip().regs[0x71] == 0, "then the tone and speaker stop");
    check(!sim().holds_rx && !sim().af, "the speaker stays muted for data afterward");
    check(chip().regs[0x58] == 0x0301 && chip().regs[0x72] == 4646 && chip().regs[0x59] == 0x1008 && chip().regs[0x3F] == 0x3002, "the modem is set up again");
    check(chip().aired.len() == 1, "and the reply still goes out");
    check(chip().state_writes.iter().all(|&v| v == 0 || v == 0x0302), "the tone never uses a transmit state");
    let w = words_of(&make_packet(9, 2, 0, 1, b"NEXT"));
    air_packet(&w);
    run(app, 2);
    check(textapp::debug_got() == 2, "the next message still arrives after a tone");
    air_packet(&w);
    let turned = unsafe { SPEAKER_TURN_ONS };
    run(app, 2);
    check(unsafe { SPEAKER_TURN_ONS } == turned, "a repeat doesn't ring again");
    text_open();
    press(app, 10);
    let plain = plain_of(&chip().aired[0]);
    let turned = unsafe { SPEAKER_TURN_ONS };
    air_packet(&words_of(&make_reply(7, plain[1], plain[2])));
    run(app, 1);
    check(unsafe { SPEAKER_TURN_ONS } == turned + 1 && chip().tones.len() == 1, "a delivery confirmation plays one note");
    check(chip().regs[0x59] == 0x1008 && !sim().transmitting, "and it keeps listening");
    text_open();
    press(app, 10);
    let turned = unsafe { SPEAKER_TURN_ONS };
    run(app, 40);
    check(unsafe { SPEAKER_TURN_ONS } == turned, "no tone when a message fails");

    println!("TEXT: saving");
    text_open();
    tap(app, 2, 1);
    press(app, 12);
    let plain = plain_of(&chip().aired[0]);
    air_packet(&words_of(&make_reply(9, plain[1], plain[2])));
    run(app, 1);
    air_packet(&words_of(&make_packet(9, 80, 0, 1, b"KEPT")));
    run(app, 1);
    press(app, 10);
    let before = textapp::debug_msgs();
    check(hold(app, 13), "closing the app");
    text_reopen();
    textapp::entry(&API, AppEvent::Draw);
    shot("txt_11_reopened");
    let m = textapp::debug_msgs();
    check(m.len() == 3 && m[0] == (DELIVERED, "A".to_string()) && m[1] == (RECEIVED, "KEPT".to_string()), "messages and their marks are still there after reopening");
    check(m[2] == (FAILED, "PING 1".to_string()) && before[2].0 == SENDING, "one closed mid-send comes back as not confirmed");
    text_ok_screen("reopened");
    press(app, 10);
    let m = textapp::debug_msgs();
    check(m.len() == 4 && m[3].1 == "PING 1" && m[3].0 == SENDING, "new messages add to the end");
    press(app, 13);
    text_open();
    let erases = unsafe { NOR_ERASES };
    for i in 0..300u16 {
        air_packet(&words_of(&make_packet(9, i as u8, 0, 1, format!("M{i}").as_bytes())));
        run_ms(app, 60);
        chip().script.clear();
    }
    run(app, 2);
    let m = textapp::debug_msgs();
    println!("     {} saved after 300 messages, {} erases", m.len(), unsafe { NOR_ERASES } - erases);
    check(m.last().map(|x| x.1.as_str()) == Some("M299"), "after 300 messages the newest is last");
    check(m.len() >= 192 && m.len() <= 256, "with at least 192 kept");
    check(m.windows(2).all(|p| p[1].1[1..].parse::<u32>().unwrap() == p[0].1[1..].parse::<u32>().unwrap() + 1), "all in order with none missing");
    check(unsafe { NOR_ERASES } - erases <= 6, "erasing a sector only as the ring reaches it");
    let count = m.len();
    press(app, 13);
    text_reopen();
    check(textapp::debug_msgs().len() == count && textapp::debug_msgs().last().map(|x| x.1.as_str()) == Some("M299"), "and the ring is found again after reopening");
    for _ in 0..(count + 5) {
        press(app, 14);
    }
    textapp::entry(&API, AppEvent::Draw);
    text_ok_screen("scrolled to the oldest");
    air_packet(&words_of(&make_packet(9, 200, 0, 1, b"NEWEST")));
    run(app, 1);
    textapp::entry(&API, AppEvent::Draw);
    text_ok_screen("back to the newest");
    check(textapp::debug_msgs().last().map(|x| x.1.as_str()) == Some("NEWEST"), "a new message after a reopen lands at the end");
    nor_clear();
    nor()[0x1F_0040] = 0xA7;
    nor()[0x1F_0043] = 200;
    text_reopen();
    check(textapp::debug_msgs().is_empty(), "a damaged record isn't treated as a message");
    textapp::entry(&API, AppEvent::Draw);

    println!("TEXT: erasing saved messages");
    text_open();
    air_packet(&words_of(&make_packet(9, 1, 0, 1, b"SECRET")));
    run(app, 2);
    check(!hold(app, 11), "holding # doesn't leave the app");
    check(textapp::debug_msgs().len() == 1 && textapp::debug_notice().contains("AGAIN"), "the first hold only asks");
    press(app, 14);
    hold(app, 11);
    check(textapp::debug_msgs().len() == 1, "another key cancels the question");
    run(app, 4);
    hold(app, 11);
    check(textapp::debug_msgs().len() == 1, "and so does waiting");
    hold(app, 11);
    check(textapp::debug_msgs().is_empty() && textapp::debug_notice().contains("ERASED"), "holding # twice erases everything");
    check(nor()[0x1F_0000..0x1F_4000].iter().all(|&b| b == 0xFF), "with nothing left in flash");
    text_reopen();
    check(textapp::debug_msgs().is_empty(), "and nothing comes back");
    tap(app, 2, 1);
    press(app, 12);
    check(textapp::debug_msgs().len() == 1, "the log works again afterward");

    println!("TEXT: when transmitting fails");
    text_open();
    sim().tx_block = true;
    tap(app, 2, 1);
    let r = press(app, 12);
    check(!r && chip().aired.is_empty(), "a refused PTT sends nothing");
    check(sim().ptt_calls == 2, "and releases PTT, which clears the firmware's refused flag");
    check(textapp::debug_notice().contains("BLOCKED"), "says so");
    check(textapp::debug_comp() == "A" && textapp::debug_msgs().is_empty(), "and keeps what you typed");
    check(!sim().tx_enabled && !textapp::debug_pending().0, "TX locked again, nothing waiting");
    check(chip().regs[0x59] == 0x1008, "still receiving");
    sim().tx_block = false;
    chip().tx_never_done = true;
    let before = sim().now;
    press(app, 12);
    let ms = sim().now.wrapping_sub(before) / 10;
    println!("     stuck send gave up after {ms} ms");
    check(ms <= 1800, "a stuck modem gives up in under two seconds");
    check(!sim().transmitting && !sim().tx_enabled, "and unkeys");
    check(textapp::debug_notice().contains("FAILED"), "says it failed");
    check(textapp::debug_comp() == "A" && !textapp::debug_pending().0, "and keeps the message to try again");
    check(chip().regs[0x2B] == 0x0300 && chip().regs[0x40] == 0x34D0, "filter and deviation restored");
    check(chip().regs[0x59] == 0x1008, "still receiving");
    chip().tx_never_done = false;
    press(app, 12);
    check(textapp::debug_pending().0, "the send works once the fault clears");
    sim().tx_block = true;
    run(app, 5);
    check(textapp::debug_msgs()[0].0 == FAILED && !textapp::debug_pending().0, "transmit locked during retries ends them as failed");
    sim().tx_block = false;

    println!("TEXT: leaving");
    tap(app, 2, 1);
    press(app, 13);
    check(textapp::debug_comp().is_empty(), "with nothing waiting, Exit deletes the last letter");
    run(app, 2);
    check(press(app, 13), "Exit with nothing typed leaves");
    let c = chip();
    check(c.regs[0x58] == 0 && c.regs[0x70] == 0 && c.regs[0x3F] == 0, "the modem is switched off");
    check(c.regs[0x59] == 0x0028 && c.regs[0x72] == 0x1111 && c.regs[0x2B] == 0x0300, "the chip's other settings are as found");
    check(!sim().transmitting, "not transmitting");
    text_open();
    tap(app, 2, 1);
    check(hold(app, 13), "holding Exit leaves even with text typed");
    check(chip().regs[0x58] == 0, "and switches the modem off too");
    text_open();
    press(app, 10);
    check(hold(app, 13), "holding Exit leaves even while waiting for a reply");
    check(chip().regs[0x58] == 0 && !sim().transmitting, "with the modem off");
    app(&API, AppEvent::Leave);
    check(chip().regs[0x58] == 0, "a second clean-up is harmless");

    println!("TEXT: CPS drafts");
    text_open();
    hold(app, 10);
    check(textapp::debug_notice().contains("CPS"), "empty flash tells the user where to add drafts");
    let slot = 0x14_C000 + 7 * 0x8000;
    nor()[slot..slot + 4].copy_from_slice(b"OVL1");
    nor()[slot + 12..slot + 17].copy_from_slice(b"TEXT\0");
    let drafts = slot + 0x7E00;
    nor()[drafts..drafts + 5].copy_from_slice(b"TXT1\x02");
    let first = drafts + 8;
    nor()[first..first + 3].copy_from_slice(b"\x02OK");
    let second = first + 51;
    nor()[second..second + 10].copy_from_slice(b"\x09ON MY WAY");
    press(app, 2);
    hold(app, 10);
    press(app, 15);
    app(&API, AppEvent::Draw);
    press(app, 13);
    check(textapp::debug_comp() == "A" && chip().aired.is_empty(), "preview and cancel preserve typed text without transmitting");
    hold(app, 10);
    press(app, 15);
    press(app, 12);
    check(textapp::debug_comp() == "ON MY WAY" && chip().aired.is_empty(), "choosing a draft only loads it into the composer");
    press(app, 12);
    check(chip().aired.len() == 1 && textapp::debug_pending().0, "sending a draft uses the existing acknowledgement flow");
}

fn test_radio_watch() {
    println!("Radio Watch: named lists and activity recall");
    let app: Entry = radiowatch::entry;
    reset(OUT);
    nor_clear();
    let slot = 0x14_C000 + 4 * 0x8000;
    nor()[slot..slot + 4].copy_from_slice(b"OVL1");
    nor()[slot + 12..slot + 24].copy_from_slice(b"RADIO WATCH\0");
    let mut cfg = [0xffu8; 512];
    cfg[..5].copy_from_slice(b"RW01\x02");
    cfg[8..13].copy_from_slice(b"Local");
    cfg[20] = 3;
    cfg[22..28].copy_from_slice(&[1, 0, 232, 3, 2, 0]);
    cfg[86..93].copy_from_slice(b"Airband");
    cfg[98] = 1;
    cfg[100..102].copy_from_slice(&[3, 0]);
    let mut crc = 0u16;
    for &b in &cfg[..510] {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x1021 } else { 0 };
        }
    }
    cfg[510..].copy_from_slice(&crc.to_be_bytes());
    nor()[slot + 0x7E00..slot + 0x8000].copy_from_slice(&cfg);
    let channel = |bcd: [u8; 4], name: &[u8]| {
        let mut row = [0u8; 32];
        row[..4].copy_from_slice(&bcd);
        row[20..20 + name.len()].copy_from_slice(name);
        row
    };
    sim().chans.insert(999, channel([0x00, 0x20, 0x65, 0x14], b"LOCAL"));
    sim().chans.insert(1, channel([0x00, 0x00, 0x60, 0x14], b"QUIET"));
    sim().chans.insert(2, channel([0x00, 0x00, 0x23, 0x13], b"ATIS"));
    add(146_520_000, 120, -1, |_| true);
    app(&API, AppEvent::Enter);
    app(&API, AppEvent::Draw);
    shot("watch_1_lists");
    press(app, 12);
    check(sim().tuned == 146_520_000, "scan skips an empty channel and reaches channel 1000");
    run(app, 2);
    check(sim().tuned == 146_520_000, "stays on an active channel");
    shot("watch_2_scan");
    sim().signals.clear();
    run(app, 1);
    press(app, 11);
    shot("watch_3_activity");
    press(app, 12);
    check(sim().tuned == 146_520_000 && sim().holds_rx, "recalls a completed hit for listening");
    press(app, 13);
    press(app, 13);
    press(app, 15);
    press(app, 12);
    check(sim().tuned == 132_300_000 && sim().modulation == 1, "switches lists and applies airband AM");
    check(sim().ptt_calls == 0 && sim().tx_enable_calls == 0, "scanner and recall never enable TX");
}

fn test_satellite_rx() {
    println!("Satellite RX: UTC clock, scheduled receive and Doppler");
    let app: Entry = satrx::entry;
    reset(OUT);
    nor_clear();
    let slot = 0x14_C000 + 5 * 0x8000;
    nor()[slot..slot + 4].copy_from_slice(b"OVL1");
    nor()[slot + 12..slot + 25].copy_from_slice(b"SATELLITE RX\0");
    let now = sgp4mini::days_from_civil(2026, 11, 8) as u32 * 86_400 + 4 * 3600;
    let mut plan = [0xffu8; 4096];
    plan[..6].copy_from_slice(b"SRX1\x01\x01");
    plan[8..20].fill(0);
    plan[8..11].copy_from_slice(b"ISS");
    plan[20..24].copy_from_slice(&437_800_000u32.to_le_bytes());
    plan[32..36].copy_from_slice(&(now + 5).to_le_bytes());
    plan[36..40].copy_from_slice(&(now + 30).to_le_bytes());
    plan[40..42].copy_from_slice(&45u16.to_le_bytes());
    for i in 0..41 {
        let at = 44 + i * 6;
        let az = (3590 + 2 * i as u16) % 3600;
        let shift = 3000 - 150 * i as i16;
        plan[at..at + 2].copy_from_slice(&az.to_le_bytes());
        plan[at + 2..at + 4].copy_from_slice(&300i16.to_le_bytes());
        plan[at + 4..at + 6].copy_from_slice(&shift.to_le_bytes());
    }
    let mut crc = 0u16;
    for &b in &plan[..4094] {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x1021 } else { 0 };
        }
    }
    plan[4094..].copy_from_slice(&crc.to_be_bytes());
    nor()[slot + 0x7000..slot + 0x8000].copy_from_slice(&plan);
    app(&API, AppEvent::Enter);
    press(app, 10);
    for digit in b"261108040000" {
        press(app, digit - b'0');
    }
    press(app, 12);
    check(utc_get() == now, "accepts the UTC date and time");
    shot("sat_1_passes");
    press(app, 12);
    run(app, 4);
    check(!sim().holds_rx, "waits for the pass before opening receive");
    run(app, 3);
    check(sim().holds_rx && sim().tuned > 437_800_000, "starts receive above the nominal downlink on approach");
    shot("sat_2_tracking");
    run(app, 15);
    check(sim().tuned < 437_800_000 && sim().retunes > 0, "retunes below the nominal downlink as the satellite recedes");
    run(app, 10);
    check(!sim().holds_rx, "mutes after the pass ends");
    check(sim().ptt_calls == 0 && sim().tx_enable_calls == 0, "Satellite RX never enables TX");
}

fn main() {
    test_repeater();
    test_sweep();
    test_fox();
    test_search_qt();
    test_text();
    test_radio_watch();
    test_satellite_rx();
    test_menus();
    check(unsafe { TONE_ON_CALLS } == 0, "no app called the transmitting tone function");
    unsafe {
        println!("\n{} passed, {} failed", PASSES, FAILS);
        if FAILS > 0 {
            std::process::exit(1);
        }
    }
}
