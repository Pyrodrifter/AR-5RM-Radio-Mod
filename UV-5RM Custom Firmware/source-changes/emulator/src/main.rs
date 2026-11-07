//! UV-5RM screen emulator: runs the real app sources and the firmware's own
//! menu-list drawing code on the PC, in a window, with the keyboard as keypad.
//!
//!   cargo run --release -- [--ar5rm] [--image CHIRP_IMAGE] [--scale N]
//!
//! See README.md for the key map. Nothing here talks to a radio.
#![allow(static_mut_refs, dead_code, clippy::missing_safety_doc)]

mod app;
mod appbuild;
mod skin;

/// `crate::device::radio` as ui/standby.rs imports it.
mod device {
    pub mod radio {
        pub use crate::app::{Modulation, Power, SubAudio};
    }
}

use bf5rh_abi::{Api, AppEntryFn, AppEvent, AppResult, ListRow, HANDOFF_SIZE};
use ui::rounded_font::FONT_9X18;
use embedded_graphics::mono_font::ascii::{FONT_5X8, FONT_6X10};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use minifb::{Key, MouseButton, MouseMode, Scale, ScaleMode, Window, WindowOptions};
use skin::{Action, Skin};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

// ---- firmware code shared with the radio build, compiled for the PC ----

#[path = "../../../firmware-source/src/flash_map.rs"]
#[allow(unused)]
mod flash_map;

#[path = "../../../firmware-source/src/app/settings.rs"]
#[allow(unused)]
mod tones;

mod drivers {
    pub mod norflash {
        pub const CAL_BLOCK_ADDR: u32 = 0xF210;
    }
    pub mod st7735 {
        pub const WIDTH: u16 = 160;
        pub const HEIGHT: u16 = 128;
    }
}

pub mod ui {
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

        // Same as firmware-source/src/ui/mod.rs.
        pub fn push_num(&mut self, mut value: u32, width: usize, pad: u8) {
            let mut digits = [0u8; 10];
            let mut start = 10;
            loop {
                start -= 1;
                digits[start] = b'0' + (value % 10) as u8;
                value /= 10;
                if value == 0 || start == 0 {
                    break;
                }
            }
            for _ in (digits.len() - start)..width {
                self.push_ascii(pad);
            }
            for &d in &digits[start..] {
                self.push_ascii(d);
            }
        }

        pub fn push_ascii(&mut self, byte: u8) {
            if let Some(next) = self.buf.get_mut(self.len) {
                *next = byte;
                self.len += 1;
            }
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

    #[path = "../../../../firmware-source/src/ui/backdrop.rs"]
    #[allow(unused)]
    pub mod backdrop;
    #[path = "../../../../firmware-source/src/ui/theme.rs"]
    #[allow(unused)]
    pub(crate) mod theme;
    #[path = "../../../../firmware-source/src/ui/rounded_font.rs"]
    pub(crate) mod rounded_font;

    // Same as firmware-source/src/ui/mod.rs.
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

    pub(crate) fn freq_text(w: &mut dyn core::fmt::Write, hz: u32, digits: usize) {
        num(w, hz / 1_000_000, 1);
        w.write_str(".").ok();
        num(w, hz % 1_000_000 / [100_000, 10_000, 1000, 100, 10][digits - 1], digits);
    }
    #[path = "../../../../firmware-source/src/ui/list.rs"]
    #[allow(unused)]
    pub mod list;
    #[path = "../../../../firmware-source/src/ui/icons.rs"]
    #[allow(unused)]
    pub mod icons;
    #[path = "../../../../firmware-source/src/ui/standby.rs"]
    #[allow(unused, private_interfaces)]
    pub mod standby;

    /// The home screen's draw cache (crate-private in the firmware's ui module).
    pub struct HomeCache(standby::Cache);

    impl HomeCache {
        pub fn new() -> Self {
            HomeCache(standby::Cache::new())
        }
    }

    pub fn draw_home<D>(lcd: &mut D, app: &mut crate::app::App, cache: &mut HomeCache)
    where
        D: embedded_graphics::prelude::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
    {
        standby::draw_standby(lcd, app, &mut cache.0);
    }
}

use flash_map::addr;
use ui::list::{draw_list, Cache, ListSource};

const W: usize = 160;
const H: usize = 128;
const NOR_SIZE: usize = 0x20_0000;
const SETTINGS_BYTES: usize = flash_map::SETTINGS_BYTES;
const SIDE_CFG_BYTES: usize = 14;

// ---- key ids and kinds, as device/keypad.rs numbers them ----

const K_STAR: u8 = 10;
const K_POUND: u8 = 11;
const K_MENU: u8 = 12;
const K_EXIT: u8 = 13;
const K_UP: u8 = 14;
const K_DOWN: u8 = 15;
const K_VM: u8 = 16;
const K_AB: u8 = 17;
const K_BAND: u8 = 18;
const K_SIDE1: u8 = 19;
const K_SIDE2: u8 = 20;

const KIND_PRESS: u8 = 0;
const KIND_RELEASE: u8 = 1;
const KIND_SINGLE: u8 = 2;
const KIND_LONG: u8 = 3;
const KIND_REPEAT: u8 = 4;

// Same timings as device/keypad.rs, in 100 us ticks.
const LONG_PRESS_TICKS: u32 = 10_000;
const REPEAT_FIRE_TICKS: u32 = 15_000;
const REPEAT_RESET_TICKS: u32 = 14_000;

fn supports_long(k: u8) -> bool {
    matches!(k, K_UP | K_DOWN | K_VM | K_BAND | K_STAR | K_POUND | 0 | K_EXIT | K_SIDE1 | K_SIDE2)
}

fn supports_repeat(k: u8) -> bool {
    matches!(k, K_UP | K_DOWN)
}

const KEYMAP: &[(Key, u8, &str)] = &[
    (Key::Key0, 0, ""), (Key::Key1, 1, ""), (Key::Key2, 2, ""), (Key::Key3, 3, ""),
    (Key::Key4, 4, ""), (Key::Key5, 5, ""), (Key::Key6, 6, ""), (Key::Key7, 7, ""),
    (Key::Key8, 8, ""), (Key::Key9, 9, ""),
    (Key::NumPad0, 0, ""), (Key::NumPad1, 1, ""), (Key::NumPad2, 2, ""), (Key::NumPad3, 3, ""),
    (Key::NumPad4, 4, ""), (Key::NumPad5, 5, ""), (Key::NumPad6, 6, ""), (Key::NumPad7, 7, ""),
    (Key::NumPad8, 8, ""), (Key::NumPad9, 9, ""),
    (Key::Q, K_STAR, "Q or numpad *  = *"), (Key::NumPadAsterisk, K_STAR, ""),
    (Key::W, K_POUND, "W or numpad /  = #"), (Key::NumPadSlash, K_POUND, ""),
    (Key::Enter, K_MENU, "Enter          = MENU"), (Key::NumPadEnter, K_MENU, ""),
    (Key::Escape, K_EXIT, "Esc/Backspace  = EXIT"), (Key::Backspace, K_EXIT, ""),
    (Key::Up, K_UP, "Up / Down      = arrows"), (Key::Down, K_DOWN, ""),
    (Key::V, K_VM, "V              = V/M"),
    (Key::A, K_AB, "A              = A/B"),
    (Key::B, K_BAND, "B              = BAND"),
    (Key::Z, K_SIDE1, "Z              = side key 1 (FM)"),
    (Key::X, K_SIDE2, "X              = side key 2 (light)"),
];

// ---- emulator state ----

struct Emu {
    fb: Vec<u16>,
    start: Instant,
    nor: Vec<u8>,
    settings: [u8; SETTINGS_BYTES],
    side_cfg: [u8; SIDE_CFG_BYTES],
    master_hz: u32,
    master_wide: bool,
    rx_hz: u32,
    tx_hz: u32,
    chip_id: u16,
    handoff: Vec<u8>,
    list_cache: Option<Cache>,
    utc_base: Option<(u32, Instant)>,
    fm_khz: u32,
    fm_channels: Option<Vec<u8>>,
    tx_enabled: bool,
    ptt_held: bool,
    signal_held: bool,
    noise: u32,
    gains: [u16; 4],
    home_tx: bool,
}

static mut EMU: Option<Emu> = None;

fn emu() -> &'static mut Emu {
    unsafe { EMU.as_mut().expect("emulator state") }
}

impl OriginDimensions for Emu {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Emu {
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

fn now_100us() -> u32 {
    (emu().start.elapsed().as_micros() / 100) as u32
}

fn rand() -> u32 {
    let e = emu();
    e.noise = e.noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    e.noise >> 16
}

fn clear_screen() {
    emu().fb.fill(0);
}

// ---- the API table apps call, mirroring app/overlay.rs ----

extern "C" fn api_uptime() -> u32 {
    now_100us()
}
extern "C" fn api_delay_ms(ms: u32) {
    std::thread::sleep(Duration::from_millis(ms as u64));
}
extern "C" fn api_fill_rect(x: i16, y: i16, w: u16, h: u16, c: u16) {
    Rectangle::new(Point::new(x as i32, y as i32), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::from(RawU16::new(c))))
        .draw(emu())
        .ok();
}
extern "C" fn api_blit(x: i16, y: i16, w: u16, h: u16, px: *const u16) {
    if px.is_null() {
        return;
    }
    let pixels = unsafe { std::slice::from_raw_parts(px, w as usize * h as usize) };
    let area = Rectangle::new(Point::new(x as i32, y as i32), Size::new(w as u32, h as u32));
    emu().fill_contiguous(&area, pixels.iter().map(|&r| Rgb565::from(RawU16::new(r)))).ok();
}
extern "C" fn api_draw_line(x0: i16, y0: i16, x1: i16, y1: i16, c: u16) {
    Line::new(Point::new(x0 as i32, y0 as i32), Point::new(x1 as i32, y1 as i32))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::from(RawU16::new(c)), 1))
        .draw(emu())
        .ok();
}
extern "C" fn api_draw_circle(x: i16, y: i16, r: u16, c: u16, filled: bool) {
    let color = Rgb565::from(RawU16::new(c));
    let style = if filled { PrimitiveStyle::with_fill(color) } else { PrimitiveStyle::with_stroke(color, 1) };
    Circle::new(Point::new(x as i32 - r as i32, y as i32 - r as i32), 2 * r as u32)
        .into_styled(style)
        .draw(emu())
        .ok();
}
extern "C" fn api_draw_text(x: i16, y: i16, t: *const u8, len: u16, fg: u16, bg: u16, font: u8) -> u16 {
    if t.is_null() {
        return x as u16;
    }
    let bytes = unsafe { std::slice::from_raw_parts(t, len as usize) };
    let s = std::str::from_utf8(bytes).unwrap_or("");
    let f: &MonoFont<'_> = match font {
        2 => &FONT_9X18,
        1 => &FONT_6X10,
        _ => &FONT_5X8,
    };
    let style = MonoTextStyleBuilder::new()
        .font(f)
        .text_color(Rgb565::from(RawU16::new(fg)))
        .background_color(Rgb565::from(RawU16::new(bg)))
        .build();
    Text::new(s, Point::new(x as i32, y as i32 + f.baseline as i32), style).draw(emu()).ok();
    (x as i32 + f.character_size.width as i32 * s.chars().count() as i32) as u16
}
extern "C" fn api_set_rx_freq(hz: u32) {
    emu().rx_hz = hz;
}
extern "C" fn api_set_tx_freq(hz: u32) {
    emu().tx_hz = hz;
}
extern "C" fn api_set_ptt(_on: bool) {}
extern "C" fn api_read_rssi() -> u16 {
    let base = if emu().signal_held { 190 } else { 55 };
    base + (rand() % 8) as u16
}
extern "C" fn api_nop_u8(_: u8) {}
extern "C" fn api_nop_u16(_: u16) {}
extern "C" fn api_nop() {}
extern "C" fn api_nop_bool(_: bool) {}
extern "C" fn api_mic() -> u8 {
    if emu().ptt_held { 40 + (rand() % 30) as u8 } else { 0 }
}
extern "C" fn api_nor_read(a: u32, buf: *mut u8, len: u32) -> bool {
    let (a, n) = (a as usize, len as usize);
    if buf.is_null() || a + n > NOR_SIZE {
        return false;
    }
    unsafe { std::ptr::copy_nonoverlapping(emu().nor[a..].as_ptr(), buf, n) };
    true
}
extern "C" fn api_nor_write(a: u32, buf: *const u8, len: u32) -> bool {
    let (a, n) = (a as usize, len as usize);
    if buf.is_null() || a + n > NOR_SIZE {
        return false;
    }
    let data = unsafe { std::slice::from_raw_parts(buf, n) };
    for (d, s) in emu().nor[a..a + n].iter_mut().zip(data) {
        *d &= *s; // NOR programming only clears bits, like the real chip
    }
    true
}
extern "C" fn api_nor_erase(a: u32) -> bool {
    let a = (a as usize) & !0xFFF;
    if a + 0x1000 > NOR_SIZE {
        return false;
    }
    emu().nor[a..a + 0x1000].fill(0xFF);
    true
}
fn copy_out(s: &str, out: *mut u8) -> u16 {
    if out.is_null() {
        return 0;
    }
    unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), out, s.len()) };
    s.len() as u16
}
extern "C" fn api_fmt_u32(v: u32, out: *mut u8) -> u16 {
    copy_out(&v.to_string(), out)
}
extern "C" fn api_fmt_freq(hz: u32, out: *mut u8) -> u16 {
    copy_out(&format!("{}.{:05}", hz / 1_000_000, (hz % 1_000_000) / 10), out)
}
extern "C" fn api_fm_tune(khz: u32) {
    emu().fm_khz = khz;
}
extern "C" fn api_fm_seek(up: bool) {
    let e = emu();
    let next = if up { e.fm_khz + 100 } else { e.fm_khz.saturating_sub(100) };
    e.fm_khz = next.clamp(87_500, 108_000);
}
extern "C" fn api_fm_status() -> u32 {
    let rssi = 20 + (rand() % 30);
    1 | (rssi << 8)
}
extern "C" fn api_fm_tuned() -> u32 {
    emu().fm_khz
}
extern "C" fn api_fm_load(buf: *mut u8, len: u16) -> bool {
    match &emu().fm_channels {
        Some(c) if !buf.is_null() => {
            let n = c.len().min(len as usize);
            unsafe { std::ptr::copy_nonoverlapping(c.as_ptr(), buf, n) };
            true
        }
        _ => false,
    }
}
extern "C" fn api_fm_save(buf: *const u8, len: u16) -> bool {
    if buf.is_null() {
        return false;
    }
    emu().fm_channels = Some(unsafe { std::slice::from_raw_parts(buf, len as usize) }.to_vec());
    true
}
/// SPI flash read for the home screen's background (and anything else).
pub fn nor_read_into(addr: u32, buf: &mut [u8]) {
    let a = addr as usize;
    match emu().nor.get(a..a + buf.len()) {
        Some(src) => buf.copy_from_slice(src),
        None => buf.fill(0xFF),
    }
}

fn chan_addr(num: u16) -> usize {
    (addr::CHAN_ADDR + num as u32 * addr::CHAN_SIZE) as usize
}
extern "C" fn api_chan_read(num: u16, out: *mut u8, out_len: u16) -> bool {
    if out.is_null() || (out_len as u32) < addr::CHAN_SIZE || num >= 1000 {
        return false;
    }
    let a = chan_addr(num);
    let rec = &emu().nor[a..a + 32];
    if rec.iter().all(|&b| b == 0xFF) {
        return false;
    }
    unsafe { std::ptr::copy_nonoverlapping(rec.as_ptr(), out, 32) };
    true
}
extern "C" fn api_chan_write(num: u16, buf: *const u8, len: u16) -> bool {
    if buf.is_null() || num >= 1000 || (len as u32) < addr::CHAN_SIZE {
        return false;
    }
    let a = chan_addr(num);
    let data = unsafe { std::slice::from_raw_parts(buf, 32) };
    emu().nor[a..a + 32].copy_from_slice(data);
    true
}
extern "C" fn api_settings_get(id: u16) -> u32 {
    let s = flash_map::Settings::from_bytes(&emu().settings);
    match id {
        0 => s.sql_level as u32,
        1 => s.tot_level as u32,
        2 => s.vox_switch as u32,
        3 => s.vox_level as u32,
        4 => s.beeps_switch as u32,
        5 => s.obs_lat as u32,
        6 => s.obs_lon as u32,
        7 => emu().chip_id as u32,
        _ => 0,
    }
}
extern "C" fn api_get_master_freq() -> u32 {
    emu().master_hz
}
extern "C" fn api_get_master_wide() -> bool {
    emu().master_wide
}

struct RowsSource<'a> {
    rows: &'a [ListRow],
    window_start: usize,
    total: usize,
}

impl RowsSource<'_> {
    fn row(&self, i: usize) -> Option<&ListRow> {
        i.checked_sub(self.window_start).and_then(|j| self.rows.get(j))
    }
}

fn cstr(b: &[u8]) -> &str {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    std::str::from_utf8(&b[..end]).unwrap_or("")
}

impl ListSource for RowsSource<'_> {
    fn row_count(&mut self) -> usize {
        self.total
    }
    fn read_flash(&mut self, a: u32, buf: &mut [u8]) {
        let a = a as usize;
        buf.copy_from_slice(&emu().nor[a..a + buf.len()]);
    }
    fn label(&mut self, i: usize, w: &mut dyn core::fmt::Write) {
        if let Some(r) = self.row(i) {
            let _ = w.write_str(cstr(&r.label));
        }
    }
    fn value(&mut self, i: usize, w: &mut dyn core::fmt::Write) -> bool {
        match self.row(i) {
            Some(r) if r.has_value => {
                let _ = w.write_str(cstr(&r.value));
                true
            }
            _ => false,
        }
    }
    fn cursor(&mut self, i: usize) -> Option<usize> {
        match self.row(i) {
            Some(r) if r.cursor >= 0 => Some(r.cursor as usize),
            _ => None,
        }
    }
}

fn list_cache() -> &'static mut Cache {
    let e = emu();
    if e.list_cache.is_none() {
        e.list_cache = Some(Cache::new());
    }
    unsafe { EMU.as_mut().unwrap().list_cache.as_mut().unwrap() }
}

extern "C" fn api_draw_list(
    title: *const u8,
    title_len: u16,
    rows: *const ListRow,
    row_count: u16,
    window_start: u16,
    selected: u16,
    total: u16,
    show_arrows: bool,
) {
    let title = if title.is_null() {
        ""
    } else {
        cstr(unsafe { std::slice::from_raw_parts(title, title_len as usize) })
    };
    let rows = if rows.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(rows, row_count as usize) }
    };
    let mut src = RowsSource { rows, window_start: window_start as usize, total: total as usize };
    draw_list(emu(), title, &mut src, selected as usize, show_arrows, list_cache());
}

fn sub_from_code(code: u16) -> (u8, u16, bool) {
    // (kind 0=none 1=ctcss 2=dcs, value, inverted), as app/convert.rs decodes it
    if code > 250 {
        (1, code, false)
    } else if code >= 1 {
        let (idx, inv) = if code <= 105 { (code, false) } else { (code - 105, true) };
        match tones::DCS_TABLE.get((idx as usize).wrapping_sub(1)) {
            Some(&c) => (2, c, inv),
            None => (0, 0, false),
        }
    } else {
        (0, 0, false)
    }
}
extern "C" fn api_sub_index_of_code(code: u16) -> i32 {
    match sub_from_code(code) {
        (1, hz, _) => tones::ctcss_index(Some(hz)).map_or(-1, |i| i as i32),
        (2, c, inv) => match tones::dcs_index(c) {
            Some(i) => (tones::CTCSS_TABLE.len() + if inv { tones::DCS_TABLE.len() } else { 0 } + i) as i32,
            None => -1,
        },
        _ => -1,
    }
}
fn sub_from_index(v: i32) -> (u8, u16, bool) {
    if v < 0 {
        return (0, 0, false);
    }
    let v = v as usize;
    let (nc, nd) = (tones::CTCSS_TABLE.len(), tones::DCS_TABLE.len());
    if v < nc {
        (1, tones::CTCSS_TABLE[v], false)
    } else if v - nc < nd {
        (2, tones::DCS_TABLE[v - nc], false)
    } else {
        (2, tones::DCS_TABLE[(v - nc - nd).min(nd - 1)], true)
    }
}
extern "C" fn api_sub_code_of_index(v: i32) -> u16 {
    match sub_from_index(v) {
        (1, hz, _) => hz,
        (2, c, inv) => match tones::dcs_index(c) {
            Some(i) => i as u16 + 1 + if inv { 105 } else { 0 },
            None => 0,
        },
        _ => 0,
    }
}
extern "C" fn api_sub_format(v: i32, out: *mut u8, cap: u16) -> u16 {
    if out.is_null() || cap == 0 {
        return 0;
    }
    let s = match sub_from_index(v) {
        (1, hz, _) => format!("{}.{}Hz", hz / 10, hz % 10),
        (2, c, inv) => format!("D{:03o}{}", c, if inv { "I" } else { "N" }),
        _ => "OFF".to_string(),
    };
    let n = s.len().min(cap as usize);
    copy_out(&s[..n], out)
}
extern "C" fn api_handoff_write(buf: *const u8, len: u16) -> bool {
    if buf.is_null() || len as usize > HANDOFF_SIZE {
        return false;
    }
    emu().handoff = unsafe { std::slice::from_raw_parts(buf, len as usize) }.to_vec();
    true
}
extern "C" fn api_handoff_read(buf: *mut u8, len: u16) -> u16 {
    if buf.is_null() {
        return 0;
    }
    let h = &emu().handoff;
    let n = h.len().min(len as usize);
    unsafe { std::ptr::copy_nonoverlapping(h.as_ptr(), buf, n) };
    n as u16
}
extern "C" fn api_enter_rx() {}
extern "C" fn api_retune_rx(hz: u32, _wide: bool) {
    emu().rx_hz = hz;
}
extern "C" fn api_read_gains(out: *mut u16) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { std::ptr::copy_nonoverlapping(emu().gains.as_ptr(), out, 4) };
    true
}
extern "C" fn api_adjust_gain(menu: u8, up: bool) {
    if let Some(g) = emu().gains.get_mut(menu as usize) {
        *g = if up { g.saturating_add(1).min(7) } else { g.saturating_sub(1) };
    }
}
extern "C" fn api_tx_state() -> u32 {
    let e = emu();
    match (e.ptt_held, e.tx_enabled) {
        (true, true) => 1,
        (true, false) => 2,
        _ => 0,
    }
}
extern "C" fn api_set_tx_enabled(on: bool) {
    emu().tx_enabled = on;
}
extern "C" fn api_utc_get() -> u32 {
    match emu().utc_base {
        Some((secs, at)) => secs + at.elapsed().as_secs() as u32,
        None => bf5rh_abi::UTC_NOT_SET,
    }
}
extern "C" fn api_utc_set(secs: u32) {
    emu().utc_base = Some((secs, Instant::now()));
}
extern "C" fn api_squelch_open() -> bool {
    emu().signal_held
}
extern "C" fn api_detect_subaudio() -> i32 {
    -1
}
extern "C" fn api_save_sub(_code: u16, _also_tx: bool) {}
extern "C" fn api_check_scan() -> u32 {
    0
}
extern "C" fn api_correct(v: u32) -> u32 {
    v
}
extern "C" fn api_tune_cand(hz: u32, _uhf: bool) {
    emu().rx_hz = hz;
}
extern "C" fn api_save_vfo(hz: u32, _code: u16) {
    emu().master_hz = hz;
}
extern "C" fn api_settings_read(buf: *mut u8, len: u16) -> bool {
    if buf.is_null() || (len as usize) < SETTINGS_BYTES {
        return false;
    }
    unsafe { std::ptr::copy_nonoverlapping(emu().settings.as_ptr(), buf, SETTINGS_BYTES) };
    true
}
extern "C" fn api_settings_write(buf: *const u8, len: u16) -> bool {
    if buf.is_null() || (len as usize) < SETTINGS_BYTES {
        return false;
    }
    let mut arr = [0u8; SETTINGS_BYTES];
    arr.copy_from_slice(unsafe { std::slice::from_raw_parts(buf, SETTINGS_BYTES) });
    // Round-trip through the firmware's parser, so out-of-range values clamp
    // exactly as they would on the radio.
    emu().settings = flash_map::Settings::from_bytes(&arr).to_bytes();
    true
}
extern "C" fn api_side_read(buf: *mut u8, len: u16) -> bool {
    if buf.is_null() || (len as usize) < SIDE_CFG_BYTES {
        return false;
    }
    unsafe { std::ptr::copy_nonoverlapping(emu().side_cfg.as_ptr(), buf, SIDE_CFG_BYTES) };
    true
}
extern "C" fn api_side_write(buf: *const u8, len: u16) -> bool {
    if buf.is_null() || (len as usize) < SIDE_CFG_BYTES {
        return false;
    }
    emu().side_cfg.copy_from_slice(unsafe { std::slice::from_raw_parts(buf, SIDE_CFG_BYTES) });
    true
}
extern "C" fn api_battery() -> u16 {
    2537
}
extern "C" fn api_agc(_: i8) {}

// Faults and factory reset never return. The app runs on its own thread, so
// that thread reports back and parks forever; the host carries on.
thread_local! {
    static REPLY: std::cell::RefCell<Option<Sender<Reply>>> = const { std::cell::RefCell::new(None) };
}
extern "C" fn api_app_fault(line: u32) -> ! {
    REPLY.with(|r| {
        if let Some(tx) = r.borrow().as_ref() {
            tx.send(Reply::Fault(line)).ok();
        }
    });
    loop {
        std::thread::park();
    }
}
extern "C" fn api_factory_reset() -> ! {
    REPLY.with(|r| {
        if let Some(tx) = r.borrow().as_ref() {
            tx.send(Reply::FactoryReset).ok();
        }
    });
    loop {
        std::thread::park();
    }
}

static API: Api = Api {
    uptime_100us: api_uptime,
    delay_ms: api_delay_ms,
    fill_rect: api_fill_rect,
    blit: api_blit,
    draw_text: api_draw_text,
    draw_line: api_draw_line,
    draw_circle: api_draw_circle,
    set_rx_freq: api_set_rx_freq,
    set_tx_freq: api_set_tx_freq,
    set_ptt: api_set_ptt,
    read_rssi: api_read_rssi,
    set_modulation: api_nop_u8,
    set_power: api_nop_u8,
    set_subaudio_tx: api_nop_u16,
    set_subaudio_rx: api_nop_u16,
    tone_on: api_nop_u16,
    tone_off: api_nop,
    set_speaker: api_nop_bool,
    read_mic_level: api_mic,
    nor_read: api_nor_read,
    nor_write: api_nor_write,
    nor_erase_sector: api_nor_erase,
    fmt_u32: api_fmt_u32,
    fmt_freq: api_fmt_freq,
    fm_tune_khz: api_fm_tune,
    fm_seek: api_fm_seek,
    fm_status: api_fm_status,
    fm_tuned_freq_khz: api_fm_tuned,
    fm_power_off: api_nop,
    fm_channels_load: api_fm_load,
    fm_channels_save: api_fm_save,
    chan_read: api_chan_read,
    settings_get: api_settings_get,
    app_fault: api_app_fault,
    chan_write: api_chan_write,
    get_master_freq: api_get_master_freq,
    draw_list: api_draw_list,
    subaudio_index_of_code: api_sub_index_of_code,
    subaudio_code_of_index: api_sub_code_of_index,
    subaudio_format: api_sub_format,
    handoff_write: api_handoff_write,
    handoff_read: api_handoff_read,
    set_bandwidth: api_nop_bool,
    set_sql_level: api_nop_u8,
    set_monitor: api_nop_bool,
    enter_rx: api_enter_rx,
    retune_rx: api_retune_rx,
    read_rf_gains: api_read_gains,
    adjust_rf_gain: api_adjust_gain,
    tx_state: api_tx_state,
    set_tx_enabled: api_set_tx_enabled,
    utc_get: api_utc_get,
    utc_set: api_utc_set,
    set_backlight_hold: api_nop_bool,
    get_master_wide: api_get_master_wide,
    squelch_open: api_squelch_open,
    set_subaudio_scan_filter: api_nop_bool,
    detect_subaudio: api_detect_subaudio,
    save_master_subaudio: api_save_sub,
    freq_scan_enable: api_nop,
    freq_scan_disable: api_nop,
    check_freq_scan: api_check_scan,
    correct_measured_freq_word: api_correct,
    tune_search_candidate: api_tune_cand,
    save_master_vfo: api_save_vfo,
    settings_read: api_settings_read,
    settings_preview: api_settings_write,
    settings_commit: api_settings_write,
    side_cfg_read: api_side_read,
    side_cfg_write: api_side_write,
    factory_reset: api_factory_reset,
    battery_raw12_avg: api_battery,
    set_holds_rx: api_nop_bool,
    set_af_out: api_nop_bool,
    set_agc_fix: api_agc,
};

// The TEXT app's modem registers. Nothing is on the air in the emulator.
extern "C" fn rf_write(_a: u8, _v: u16) {}
extern "C" fn rf_read(_a: u8) -> u16 {
    0
}

// ---- running app code on its own thread ----

enum Reply {
    Done(AppResult),
    Fault(u32),
    FactoryReset,
}

struct Call(AppEntryFn, AppEvent);

struct AppThread {
    tx: Sender<Call>,
    rx: Receiver<Reply>,
}

fn spawn_app_thread() -> AppThread {
    let (call_tx, call_rx) = channel::<Call>();
    let (reply_tx, reply_rx) = channel::<Reply>();
    std::thread::Builder::new()
        .name("app".into())
        .stack_size(8 << 20)
        .spawn(move || {
            REPLY.with(|r| *r.borrow_mut() = Some(reply_tx.clone()));
            for Call(entry, ev) in call_rx {
                let result = entry(&API, ev);
                if reply_tx.send(Reply::Done(result)).is_err() {
                    break;
                }
            }
        })
        .expect("app thread");
    AppThread { tx: call_tx, rx: reply_rx }
}

struct Loaded {
    package: usize,
    segment: u8,
    entry: AppEntryFn,
    _lib: libloading::Library,
}

struct Host {
    gen_root: PathBuf,
    run_dir: PathBuf,
    packages: Vec<appbuild::Package>,
    thread: AppThread,
    app: Option<Loaded>,
    launcher_index: usize,
    load_counter: u32,
    status: String,
    screen: Screen,
    home: app::App,
    home_cache: ui::HomeCache,
    /// The running app was opened from the home screen (FM key), so leaving
    /// it goes back there, as the firmware does.
    return_home: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Menu,
}

enum Outcome {
    Continue,
    Left(String),
}

impl Host {
    fn load(&mut self, package: usize, segment: u8) -> Result<Loaded, String> {
        let seg = self.packages[package]
            .segments
            .get(segment as usize)
            .ok_or("segment out of range")?
            .clone();
        let built = appbuild::dll_path(&self.gen_root, &seg);
        // Windows locks a loaded DLL, so run a fresh copy each time: that also
        // lets F5 rebuild while an app is open.
        self.load_counter += 1;
        let copy = self.run_dir.join(format!("{}-{}.dll", seg, self.load_counter));
        std::fs::copy(&built, &copy).map_err(|e| format!("{}: {e}", built.display()))?;
        let lib = unsafe { libloading::Library::new(&copy) }.map_err(|e| e.to_string())?;
        let entry: AppEntryFn = unsafe { *lib.get::<AppEntryFn>(b"app_entry").map_err(|e| e.to_string())? };
        unsafe {
            if let Ok(f) = lib.get::<extern "C" fn(extern "C" fn(u32) -> !)>(b"emu_install_panic_hook") {
                f(api_app_fault);
            }
            if let Ok(f) = lib.get::<extern "C" fn(extern "C" fn(u8, u16), extern "C" fn(u8) -> u16)>(b"emu_set_rf") {
                f(rf_write, rf_read);
            }
            if let Ok(f) = lib.get::<extern "C" fn(&'static Api)>(b"emu_set_api") {
                f(&API);
            }
        }
        Ok(Loaded { package, segment, entry, _lib: lib })
    }

    fn call(&mut self, ev: AppEvent) -> Outcome {
        let Some(app) = &self.app else { return Outcome::Continue };
        let entry = app.entry;
        if self.thread.tx.send(Call(entry, ev)).is_err() {
            return self.lose_thread("app thread stopped");
        }
        match self.thread.rx.recv() {
            Ok(Reply::Done(r)) => self.handle(r),
            Ok(Reply::Fault(line)) => self.lose_thread(&format!("app fault {line:#x}")),
            Ok(Reply::FactoryReset) => {
                emu().settings = flash_map::Settings::DEFAULT.to_bytes();
                self.lose_thread("factory reset")
            }
            Err(_) => self.lose_thread("app thread stopped"),
        }
    }

    /// The thread is parked inside the app, so leave both behind.
    fn lose_thread(&mut self, why: &str) -> Outcome {
        if let Some(app) = self.app.take() {
            std::mem::forget(app);
        }
        self.thread = spawn_app_thread();
        self.back_to_launcher();
        Outcome::Left(why.to_string())
    }

    fn handle(&mut self, mut result: AppResult) -> Outcome {
        for _ in 0..4 {
            match result {
                AppResult::Continue => return Outcome::Continue,
                AppResult::Exit => {
                    self.app = None;
                    self.back_to_launcher();
                    return Outcome::Left(String::new());
                }
                AppResult::Fault(code) => {
                    self.app = None;
                    self.back_to_launcher();
                    return Outcome::Left(format!("app returned fault {code:#x}"));
                }
                AppResult::Chain(seg) => {
                    let pkg = self.app.as_ref().map(|a| a.package).unwrap_or(0);
                    self.app = None; // unload first: a fresh segment starts clean
                    match self.load(pkg, seg) {
                        Ok(l) => self.app = Some(l),
                        Err(e) => {
                            self.back_to_launcher();
                            return Outcome::Left(format!("chain to segment {seg}: {e}"));
                        }
                    }
                    clear_screen();
                    emu().list_cache = None;
                    emu().tx_enabled = false;
                    let entry = self.app.as_ref().unwrap().entry;
                    if self.thread.tx.send(Call(entry, AppEvent::Enter)).is_err() {
                        return self.lose_thread("app thread stopped");
                    }
                    result = match self.thread.rx.recv() {
                        Ok(Reply::Done(r)) => r,
                        Ok(Reply::Fault(l)) => return self.lose_thread(&format!("app fault {l:#x}")),
                        Ok(Reply::FactoryReset) => return self.lose_thread("factory reset"),
                        Err(_) => return self.lose_thread("app thread stopped"),
                    };
                }
            }
        }
        self.app = None;
        self.back_to_launcher();
        Outcome::Left("too many segment hops".into())
    }

    fn open(&mut self, package: usize) {
        match self.load(package, 0) {
            Ok(l) => {
                clear_screen();
                let e = emu();
                e.list_cache = None;
                e.handoff.clear();
                e.tx_enabled = false;
                self.app = Some(l);
                self.status = self.packages[package].name.clone();
                if let Outcome::Left(why) = self.call(AppEvent::Enter) {
                    self.report(why);
                }
            }
            Err(e) => self.report(format!("can't load {}: {e}", self.packages[package].name)),
        }
    }

    fn report(&mut self, why: String) {
        if !why.is_empty() {
            eprintln!("{why}");
            self.status = why;
        } else {
            self.status = if self.screen == Screen::Home { "HOME".into() } else { "MENU".into() };
        }
    }

    fn back_to_launcher(&mut self) {
        clear_screen();
        emu().list_cache = None;
        self.home_cache = ui::HomeCache::new();
        if std::mem::take(&mut self.return_home) {
            self.screen = Screen::Home;
        }
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.status = if screen == Screen::Home { "HOME".into() } else { "MENU".into() };
        clear_screen();
        emu().list_cache = None;
        self.home_cache = ui::HomeCache::new();
    }

    /// Per frame while no app runs: the home screen's state, then a draw.
    fn draw_idle(&mut self) {
        let settings = flash_map::Settings::from_bytes(&emu().settings);
        let noise = rand();
        if ui::theme::set(settings.theme) {
            clear_screen();
            emu().list_cache = None;
            self.home_cache = ui::HomeCache::new();
        }
        let e = emu();
        self.home.tick(settings, e.ptt_held, e.signal_held, noise);
        e.master_hz = self.home.master_freq_hz();
        e.home_tx = self.screen == Screen::Home && self.home.is_transmitting();
        match self.screen {
            Screen::Home => ui::draw_home(emu(), &mut self.home, &mut self.home_cache),
            Screen::Menu => self.draw_launcher(),
        }
    }

    fn draw_launcher(&mut self) {
        struct Names<'a>(&'a [appbuild::Package]);
        impl ListSource for Names<'_> {
            fn row_count(&mut self) -> usize {
                self.0.len()
            }
            fn read_flash(&mut self, a: u32, buf: &mut [u8]) {
                let a = a as usize;
                buf.copy_from_slice(&emu().nor[a..a + buf.len()]);
            }
            fn label(&mut self, i: usize, w: &mut dyn core::fmt::Write) {
                let _ = w.write_str(&self.0[i].name);
            }
        }
        let mut names = Names(&self.packages);
        draw_list(emu(), "MENU", &mut names, self.launcher_index, false, list_cache());
    }

    fn key(&mut self, id: u8, kind: u8) {
        if self.app.is_some() {
            if let Outcome::Left(why) = self.call(AppEvent::Key { id, kind }) {
                self.report(why);
            }
            return;
        }
        if self.screen == Screen::Home {
            match self.home.key(id, kind, &emu().nor) {
                app::HomeAction::OpenMenu => self.go(Screen::Menu),
                app::HomeAction::OpenApp(name) => {
                    if let Some(i) = self.packages.iter().position(|p| p.name == name) {
                        self.return_home = true;
                        self.open(i);
                        if self.app.is_none() {
                            self.return_home = false;
                        }
                    }
                }
                app::HomeAction::None => {}
            }
            return;
        }
        let n = self.packages.len();
        match (id, kind) {
            (K_UP, KIND_SINGLE | KIND_REPEAT) => self.launcher_index = (self.launcher_index + n - 1) % n,
            (K_DOWN, KIND_SINGLE | KIND_REPEAT) => self.launcher_index = (self.launcher_index + 1) % n,
            (K_MENU, KIND_SINGLE) => self.open(self.launcher_index),
            (K_EXIT, KIND_SINGLE) => self.go(Screen::Home),
            _ => {}
        }
    }
}

// ---- keypad model: one key at a time, like the radio's matrix scan ----

struct Keypad {
    stable: Option<u8>,
    hold: u32,
    long_fired: bool,
    last: u32,
}

impl Keypad {
    fn poll(&mut self, down: Option<u8>, now: u32, out: &mut Vec<(u8, u8)>) {
        let elapsed = now.wrapping_sub(self.last);
        self.last = now;
        if down == self.stable {
            if let Some(k) = self.stable {
                self.hold += elapsed;
                if !self.long_fired && supports_long(k) && self.hold >= LONG_PRESS_TICKS {
                    self.long_fired = true;
                    out.push((k, KIND_LONG));
                }
                if supports_repeat(k) && self.hold >= REPEAT_FIRE_TICKS {
                    self.hold = REPEAT_RESET_TICKS;
                    out.push((k, KIND_REPEAT));
                }
            }
            return;
        }
        if let Some(old) = self.stable {
            if !self.long_fired {
                out.push((old, KIND_SINGLE));
            }
            out.push((old, KIND_RELEASE));
        }
        self.stable = down;
        self.hold = 0;
        self.long_fired = false;
        if let Some(k) = down {
            out.push((k, KIND_PRESS));
        }
    }
}

// ---- persistence and setup ----

fn deci_hz_bcd(hz: u32) -> [u8; 4] {
    let mut v = hz / 10;
    let mut out = [0u8; 4];
    for b in out.iter_mut() {
        let lo = v % 10;
        v /= 10;
        let hi = v % 10;
        v /= 10;
        *b = ((hi << 4) | lo) as u8;
    }
    out
}

fn demo_channel(name: &str, rx: u32, tx: u32) -> [u8; 32] {
    let mut r = [0xFFu8; 32];
    r[0..4].copy_from_slice(&deci_hz_bcd(rx));
    r[4..8].copy_from_slice(&deci_hz_bcd(tx));
    r[8..12].fill(0);
    r[12] = 0;
    r[13] = 0;
    r[14] = 1; // low power
    r[15] = 0x04; // scan add
    let n = name.len().min(12);
    r[20..20 + n].copy_from_slice(&name.as_bytes()[..n]);
    r
}

fn fresh_nor(root: &Path) -> Vec<u8> {
    let mut nor = vec![0xFFu8; NOR_SIZE];
    let logo = root.join("..").join("..").join("skull_logo.rgb565");
    match std::fs::read(&logo) {
        Ok(img) => {
            let a = addr::BOOT_LOGO_ADDR as usize;
            nor[a..a + img.len()].copy_from_slice(&img);
        }
        Err(e) => eprintln!("no menu backdrop ({}): {e}", logo.display()),
    }
    let demo = [
        ("CALL 2M", 146_520_000, 146_520_000),
        ("RPT DEMO", 146_940_000, 146_340_000),
        ("NOAA WX1", 162_550_000, 162_550_000),
        ("GMRS 1", 462_562_500, 462_562_500),
        ("CALL 70CM", 446_000_000, 446_000_000),
    ];
    for (i, (name, rx, tx)) in demo.iter().enumerate() {
        let a = chan_addr(i as u16);
        nor[a..a + 32].copy_from_slice(&demo_channel(name, *rx, *tx));
    }
    nor
}

fn save_bmp(path: &Path, fb: &[u16], scale: usize) -> std::io::Result<()> {
    let (w, h) = (W * scale, H * scale);
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        let start = out.len();
        for x in 0..w {
            let p = fb[(y / scale) * W + x / scale];
            let (r, g, b) = rgb(p);
            out.extend_from_slice(&[b, g, r]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out)
}

/// A 160x128 background: raw little-endian RGB565 (as the CPS tool uploads),
/// or an uncompressed 24-bit BMP of that size.
fn load_background(path: &str) -> Result<Vec<u8>, String> {
    let d = std::fs::read(path).map_err(|e| e.to_string())?;
    if d.len() == W * H * 2 {
        return Ok(d);
    }
    if d.len() < 54 || &d[..2] != b"BM" {
        return Err("expected 40960 bytes of RGB565 or a 160x128 24-bit BMP".into());
    }
    let off = u32::from_le_bytes(d[10..14].try_into().unwrap()) as usize;
    let w = i32::from_le_bytes(d[18..22].try_into().unwrap());
    let h = i32::from_le_bytes(d[22..26].try_into().unwrap());
    let bpp = u16::from_le_bytes(d[28..30].try_into().unwrap());
    if w != W as i32 || h.unsigned_abs() as usize != H || bpp != 24 {
        return Err(format!("BMP is {w}x{h} at {bpp} bpp; need 160x128 at 24 bpp"));
    }
    let row = (W * 3 + 3) & !3;
    let mut out = vec![0u8; W * H * 2];
    for y in 0..H {
        let src_y = if h > 0 { H - 1 - y } else { y };
        for x in 0..W {
            let i = off + src_y * row + x * 3;
            let (b, g, r) = (d[i] as u16, d[i + 1] as u16, d[i + 2] as u16);
            let px = (r >> 3) << 11 | (g >> 2) << 5 | b >> 3;
            out[(y * W + x) * 2..][..2].copy_from_slice(&px.to_le_bytes());
        }
    }
    Ok(out)
}

fn save_bmp32(path: &Path, px: &[u32], w: usize, h: usize) -> std::io::Result<()> {
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        let start = out.len();
        for &p in &px[y * w..(y + 1) * w] {
            out.extend_from_slice(&[p as u8, (p >> 8) as u8, (p >> 16) as u8]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out)
}

fn rgb(p: u16) -> (u8, u8, u8) {
    (
        (((p >> 11) & 0x1F) * 255 / 31) as u8,
        (((p >> 5) & 0x3F) * 255 / 63) as u8,
        ((p & 0x1F) * 255 / 31) as u8,
    )
}

fn draw_message(lines: &[&str]) {
    clear_screen();
    let style = MonoTextStyleBuilder::new().font(&FONT_6X10).text_color(Rgb565::WHITE).build();
    for (i, l) in lines.iter().enumerate() {
        Text::new(l, Point::new(4, 14 + 12 * i as i32), style).draw(emu()).ok();
    }
}

/// What the radio body shows besides the LCD; main updates it every frame.
struct Panel {
    skin: Skin,
    hover: Option<Action>,
    held: Vec<Action>,
    signal_latched: bool,
    status: String,
}

static mut PANEL: Option<Panel> = None;

fn present(window: &mut Window, buf: &mut [u32]) {
    let Some(p) = (unsafe { PANEL.as_ref() }) else { return };
    let e = emu();
    let state = skin::State {
        hover: p.hover,
        held: &|a| p.held.contains(&a),
        tx: api_tx_state() == 1 || e.home_tx,
        rx: e.signal_held,
        signal_latched: p.signal_latched,
        status: &p.status,
    };
    p.skin.render(buf, &e.fb, &state);
    window.update_with_buffer(buf, p.skin.width, p.skin.height).ok();
}

/// Window pixels to skin pixels, allowing for the aspect-ratio letterbox.
fn mouse_on_skin(window: &Window, skin: &Skin) -> Option<(i32, i32)> {
    let (mx, my) = window.get_unscaled_mouse_pos(MouseMode::Discard)?;
    let (ww, wh) = window.get_size();
    if ww == 0 || wh == 0 {
        return None;
    }
    let k = (ww as f32 / skin.width as f32).min(wh as f32 / skin.height as f32);
    let ox = (ww as f32 - skin.width as f32 * k) / 2.0;
    let oy = (wh as f32 - skin.height as f32 * k) / 2.0;
    Some((((mx - ox) / k) as i32, ((my - oy) / k) as i32))
}

fn build(apps: &Path, gen_root: &Path, packages: &[appbuild::Package], mut window: Option<(&mut Window, &mut [u32])>) -> bool {
    draw_message(&["BUILDING APPS...", "", "first build takes", "a minute"]);
    if let Some((w, b)) = window.as_mut() {
        present(w, b);
    }
    let started = Instant::now();
    match appbuild::build_all(apps, gen_root, packages) {
        Ok(()) => {
            println!("apps built in {:.1}s", started.elapsed().as_secs_f32());
            true
        }
        Err(e) => {
            eprintln!("APP BUILD FAILED:\n{e}");
            draw_message(&["APP BUILD FAILED", "", "see the console,", "fix, then press F5"]);
            if let Some((w, b)) = window.as_mut() {
                present(w, b);
            }
            false
        }
    }
}

/// `--selftest DIR`: no window. Opens every app, runs it for a moment, saves
/// a screenshot of each, and exits non-zero if any app fails to load or faults.
fn selftest(host: &mut Host, dir: &Path) -> bool {
    std::fs::create_dir_all(dir).ok();
    let run = |host: &mut Host, ms: u32| {
        let end = now_100us() + ms * 10;
        let mut last = now_100us();
        while now_100us() < end && host.app.is_some() {
            std::thread::sleep(Duration::from_millis(16));
            let now = now_100us();
            let mut r = host.call(AppEvent::Tick { dt_100us: now - last });
            last = now;
            if host.app.is_some() {
                r = host.call(AppEvent::Draw);
            }
            if let Outcome::Left(why) = r {
                return Err(why);
            }
        }
        Ok(())
    };
    let mut ok = true;
    host.launcher_index = 0;
    host.draw_launcher();
    save_bmp(&dir.join("00_menu.bmp"), &emu().fb, 4).ok();
    host.go(Screen::Home);
    host.draw_idle();
    save_bmp(&dir.join("00_home.bmp"), &emu().fb, 4).ok();
    // Channel mode on side A, then A/B to side B and a frequency half typed.
    for id in [K_VM, K_AB, 1, 4, 6] {
        host.key(id, KIND_SINGLE);
    }
    host.draw_idle();
    save_bmp(&dir.join("00_home_input.bmp"), &emu().fb, 4).ok();
    for id in [5u8, 2, 0, K_AB] {
        host.key(id, KIND_SINGLE);
    }
    emu().signal_held = true;
    host.draw_idle();
    host.draw_idle();
    save_bmp(&dir.join("00_home_signal.bmp"), &emu().fb, 4).ok();
    emu().signal_held = false;

    // Every theme: home (single and dual watch), the app list, SETTINGS.
    let saved = emu().settings;
    for theme in 0..ui::theme::COUNT {
        let st = &mut emu().settings;
        st[30] = (st[30] & 3) | theme << 2;
        for (dual, name) in [(0u8, "single"), (1, "dual")] {
            emu().settings[5] = dual;
            host.go(Screen::Home);
            emu().signal_held = true;
            for _ in 0..3 {
                host.draw_idle();
            }
            emu().signal_held = false;
            save_bmp(&dir.join(format!("theme{theme}_{name}.bmp")), &emu().fb, 4).ok();
            emu().settings[17] = 1;
            host.go(Screen::Home);
            host.draw_idle();
            save_bmp(&dir.join(format!("theme{theme}_{name}_name.bmp")), &emu().fb, 4).ok();
            emu().settings[17] = saved[17];
        }
        emu().settings[5] = saved[5];
        host.go(Screen::Menu);
        host.launcher_index = 2;
        host.draw_idle();
        save_bmp(&dir.join(format!("theme{theme}_menu.bmp")), &emu().fb, 4).ok();
        host.open(0);
        if host.app.is_some() {
            for id in [K_DOWN, K_DOWN, K_MENU] {
                host.key(id, KIND_SINGLE);
            }
            let _ = run(host, 300);
            save_bmp(&dir.join(format!("theme{theme}_settings.bmp")), &emu().fb, 4).ok();
        }
        host.app = None;
        host.back_to_launcher();
    }
    emu().settings = saved;
    host.go(Screen::Home);
    // The whole window, radio body and all, with MENU on the LCD.
    let skin = Skin::new(3);
    let mut win = vec![0u32; skin.width * skin.height];
    let held = [Action::Key(K_DOWN)];
    let state = skin::State {
        hover: Some(Action::Key(K_MENU)),
        held: &|a| held.contains(&a),
        tx: false,
        rx: true,
        signal_latched: true,
        status: "MENU",
    };
    skin.render(&mut win, &emu().fb, &state);
    save_bmp32(&dir.join("00_window.bmp"), &win, skin.width, skin.height).ok();
    for i in 0..host.packages.len() {
        let name = host.packages[i].name.clone();
        let file = name.to_lowercase().replace(['/', ' '], "_");
        host.status.clear();
        host.open(i);
        let mut result = if host.app.is_some() { run(host, 1500) } else { Err(host.status.clone()) };
        if result.is_ok() && host.app.is_some() {
            save_bmp(&dir.join(format!("{:02}_{file}.bmp", i + 1)), &emu().fb, 4).ok();
            // One press of Down, then a moment, to show the app reacts to keys.
            host.key(K_DOWN, KIND_PRESS);
            host.key(K_DOWN, KIND_SINGLE);
            host.key(K_DOWN, KIND_RELEASE);
            result = run(host, 400);
            if result.is_ok() && host.app.is_some() {
                save_bmp(&dir.join(format!("{:02}_{file}_down.bmp", i + 1)), &emu().fb, 4).ok();
            }
        }
        match result {
            Ok(()) => println!("  ok    {name}"),
            Err(why) if why.is_empty() => println!("  ok    {name} (exited by itself)"),
            Err(why) => {
                println!("  FAIL  {name}: {why}");
                ok = false;
            }
        }
        host.app = None;
        host.back_to_launcher();
    }
    ok
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let firmware = root.join("..").join("..").join("firmware-source");
    let apps_dir = firmware.join("apps");
    // Short paths: long ones break linking on Windows.
    let scratch = std::env::var_os("EMU_BUILD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("uv5rm-emu"));
    let gen_root = scratch.join("apps");
    let run_dir = scratch.join("run");
    let state_dir = root.join("state");
    std::fs::create_dir_all(&run_dir).ok();
    std::fs::create_dir_all(&state_dir).ok();
    for e in std::fs::read_dir(&run_dir).into_iter().flatten().flatten() {
        std::fs::remove_file(e.path()).ok(); // copies from the last run
    }

    let packages = match appbuild::read_packages(&root.join("..").join("pack_apps.py")) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let nor_file = state_dir.join("nor.bin");
    let settings_file = state_dir.join("settings.bin");
    let mut nor = match std::fs::read(&nor_file) {
        Ok(n) if n.len() == NOR_SIZE => n,
        _ => fresh_nor(&root),
    };
    if let Some(file) = value("--background") {
        match load_background(&file) {
            Ok(px) => {
                let a = addr::BACKGROUND_ADDR as usize;
                nor[a..a + px.len()].copy_from_slice(&px);
            }
            Err(e) => eprintln!("{file}: {e}"),
        }
    }
    if let Some(img) = value("--image") {
        // A CHIRP/radio_read.py image: its first 0x8000 bytes are the channels.
        match std::fs::read(&img) {
            Ok(d) if d.len() >= 0x8000 => nor[..0x8000].copy_from_slice(&d[..0x8000]),
            Ok(_) => eprintln!("{img}: too short for a channel image"),
            Err(e) => eprintln!("{img}: {e}"),
        }
    }
    let settings = match std::fs::read(&settings_file) {
        Ok(s) if s.len() == SETTINGS_BYTES => {
            let mut a = [0u8; SETTINGS_BYTES];
            a.copy_from_slice(&s);
            flash_map::Settings::from_bytes(&a).to_bytes()
        }
        _ => flash_map::Settings::DEFAULT.to_bytes(),
    };

    unsafe {
        EMU = Some(Emu {
            fb: vec![0; W * H],
            start: Instant::now(),
            nor,
            settings,
            side_cfg: [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0],
            master_hz: 146_520_000,
            master_wide: true,
            rx_hz: 146_520_000,
            tx_hz: 146_520_000,
            chip_id: if flag("--ar5rm") { 0x4819 } else { 0x4829 },
            handoff: Vec::new(),
            list_cache: None,
            utc_base: None,
            fm_khz: 100_000,
            fm_channels: None,
            tx_enabled: false,
            ptt_held: false,
            signal_held: false,
            noise: 0x1234_5678,
            gains: [3, 2, 4, 2],
            home_tx: false,
        });
    }

    if let Some(dir) = value("--selftest") {
        if !build(&apps_dir, &gen_root, &packages, None) {
            std::process::exit(2);
        }
        let mut host = Host {
            gen_root: gen_root.clone(),
            run_dir,
            packages,
            thread: spawn_app_thread(),
            app: None,
            launcher_index: 0,
            load_counter: 0,
            status: String::new(),
            screen: Screen::Menu,
            home: app::App::new(flash_map::Settings::from_bytes(&emu().settings), emu().master_hz),
            home_cache: ui::HomeCache::new(),
            return_home: false,
        };
        println!("self-test, screenshots in {dir}:");
        let ok = selftest(&mut host, Path::new(&dir));
        std::process::exit(if ok { 0 } else { 1 });
    }

    let lcd_scale = match value("--scale").as_deref() {
        Some("2") => 2,
        Some("4") => 4,
        _ => 3,
    };
    let skin = Skin::new(lcd_scale);
    let (sw, sh) = (skin.width, skin.height);
    let mut window = Window::new(
        "UV-5RM emulator",
        sw,
        sh,
        WindowOptions {
            scale: Scale::X1,
            resize: true,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .expect("window");
    window.set_target_fps(60);
    let mut buf = vec![0u32; sw * sh];
    unsafe {
        PANEL = Some(Panel { skin, hover: None, held: Vec::new(), signal_latched: false, status: String::new() });
    }

    println!("UV-5RM emulator. Keys:");
    println!("  0-9            = digits");
    for (_, _, help) in KEYMAP.iter().filter(|k| !k.2.is_empty()) {
        println!("  {help}");
    }
    println!("  Space          = PTT (apps only see it; nothing transmits)");
    println!("  S (hold)       = pretend a strong signal is on frequency");
    println!("  or click the on-screen keys; SIGNAL latches on and off");
    println!("  T              = next colour theme");
    println!("  F5             = rebuild apps from source and return to MENU");
    println!("  F12            = screenshot to source-changes/emulator/screenshots");

    let built = build(&apps_dir, &gen_root, &packages, Some((&mut window, &mut buf)));
    let mut host = Host {
        gen_root: gen_root.clone(),
        run_dir,
        packages,
        thread: spawn_app_thread(),
        app: None,
        launcher_index: 0,
        load_counter: 0,
        status: if built { "HOME".into() } else { "BUILD FAILED".into() },
        screen: Screen::Home,
        home: app::App::new(flash_map::Settings::from_bytes(&emu().settings), emu().master_hz),
        home_cache: ui::HomeCache::new(),
        return_home: false,
    };
    if built {
        clear_screen();
    }
    let mut build_ok = built;

    let mut pad = Keypad { stable: None, hold: 0, long_fired: false, last: now_100us() };
    let mut last_tick = now_100us();
    let mut events = Vec::new();
    let mut shots = 0u32;
    let mut title = String::new();
    let mut was_down = false;

    while window.is_open() {
        // On-screen buttons: what the mouse is over, and what it holds down.
        let panel = unsafe { PANEL.as_mut().unwrap() };
        let mouse_down = window.get_mouse_down(MouseButton::Left);
        let hover = mouse_on_skin(&window, &panel.skin).and_then(|(x, y)| panel.skin.hit(x, y));
        let clicked = if mouse_down && !was_down { hover } else { None };
        was_down = mouse_down;
        let mouse_held = if mouse_down { hover } else { None };
        panel.hover = hover;
        if clicked == Some(Action::Signal) {
            panel.signal_latched = !panel.signal_latched;
        }
        if window.is_key_pressed(Key::T, minifb::KeyRepeat::No) {
            // Next colour theme, as SETTINGS > DISP > THEME would set it.
            let s = &mut emu().settings;
            let next = ((s[30] >> 2) + 1) % ui::theme::COUNT;
            s[30] = (s[30] & 3) | next << 2;
        }
        let rebuild = window.is_key_pressed(Key::F5, minifb::KeyRepeat::No) || clicked == Some(Action::Rebuild);
        let screenshot = window.is_key_pressed(Key::F12, minifb::KeyRepeat::No) || clicked == Some(Action::Shot);

        if rebuild {
            host.app = None;
            build_ok = build(&apps_dir, &gen_root, &host.packages, Some((&mut window, &mut buf)));
            host.back_to_launcher();
            host.status = if build_ok { "MENU (rebuilt)".into() } else { "BUILD FAILED".into() };
        }
        if screenshot {
            let dir = root.join("screenshots");
            std::fs::create_dir_all(&dir).ok();
            shots += 1;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let path = dir.join(format!("screen-{stamp}-{shots}.bmp"));
            match save_bmp(&path, &emu().fb, 4) {
                Ok(()) => println!("saved {}", path.display()),
                Err(e) => eprintln!("screenshot: {e}"),
            }
        }

        let panel = unsafe { PANEL.as_mut().unwrap() };
        let ptt = window.is_key_down(Key::Space) || mouse_held == Some(Action::Ptt);
        let signal = window.is_key_down(Key::S) || panel.signal_latched;
        emu().ptt_held = ptt;
        emu().signal_held = signal;

        // Keys held on the keyboard light their on-screen twins.
        let typed = KEYMAP.iter().find(|(k, _, _)| window.is_key_down(*k)).map(|(_, id, _)| *id);
        panel.held.clear();
        if let Some(id) = typed {
            panel.held.push(Action::Key(id));
        }
        if let Some(a) = mouse_held {
            panel.held.push(a);
        }
        if ptt {
            panel.held.push(Action::Ptt);
        }
        if window.is_key_down(Key::S) {
            panel.held.push(Action::Signal);
        }

        if build_ok {
            let down = match mouse_held {
                Some(Action::Key(id)) => Some(id),
                _ => typed,
            };
            let now = now_100us();
            events.clear();
            pad.poll(down, now, &mut events);
            for &(id, kind) in &events {
                host.key(id, kind);
            }

            let dt = now.wrapping_sub(last_tick);
            last_tick = now;
            if host.app.is_some() {
                if let Outcome::Left(why) = host.call(AppEvent::Tick { dt_100us: dt }) {
                    host.report(why);
                }
            }
            if host.app.is_some() {
                if let Outcome::Left(why) = host.call(AppEvent::Draw) {
                    host.report(why);
                }
            } else {
                host.draw_idle();
            }
        }

        let tx = match api_tx_state() {
            1 => "  [TX]",
            2 => "  [TX refused]",
            _ => "",
        };
        let t = format!("UV-5RM emulator - {}{tx}", host.status);
        unsafe { PANEL.as_mut().unwrap() }.status = format!("{}{tx}", host.status);
        if t != title {
            window.set_title(&t);
            title = t;
        }
        present(&mut window, &mut buf);
    }

    std::fs::write(&nor_file, &emu().nor).ok();
    std::fs::write(&settings_file, emu().settings).ok();
}
