#![allow(dead_code)]
//! Static 160x128 mockups of a friendlier radio UI, drawn with embedded-graphics
//! and only the fonts the firmware already links (5x8, 6x10, 9x18), plus big
//! frequency digits drawn as segments (code, not font data, so cheap in flash).
//!
//!   cargo run --release --bin mockups -- OUT_DIR

use embedded_graphics::mono_font::ascii::{FONT_5X8, FONT_6X10, FONT_9X18};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{CornerRadii, PrimitiveStyle, Rectangle, RoundedRectangle, Triangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use std::path::Path;

const W: usize = 160;
const H: usize = 128;

struct Fb(Vec<u16>);

impl OriginDimensions for Fb {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Fb {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, it: I) -> Result<(), Self::Error> {
        for Pixel(p, c) in it {
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < W && (p.y as usize) < H {
                self.0[p.y as usize * W + p.x as usize] = RawU16::from(c).into_inner();
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Pal {
    bg: Rgb565,
    card: Rgb565,
    card_hi: Rgb565,
    text: Rgb565,
    dim: Rgb565,
    accent: Rgb565,
    on_accent: Rgb565,
    ok: Rgb565,
    warn: Rgb565,
    alert: Rgb565,
}

const fn c(r: u8, g: u8, b: u8) -> Rgb565 {
    Rgb565::new(r >> 3, g >> 2, b >> 3)
}

const ARCTIC: Pal = Pal {
    bg: c(6, 12, 22),
    card: c(16, 30, 48),
    card_hi: c(26, 50, 78),
    text: c(230, 242, 250),
    dim: c(110, 140, 165),
    accent: c(30, 170, 210),
    on_accent: c(4, 16, 26),
    ok: c(80, 230, 170),
    warn: c(255, 200, 90),
    alert: c(255, 80, 96),
};

const VIOLET: Pal = Pal {
    bg: c(14, 6, 26),
    card: c(32, 16, 52),
    card_hi: c(50, 26, 80),
    text: c(242, 230, 255),
    dim: c(150, 130, 175),
    accent: c(190, 60, 200),
    on_accent: c(255, 255, 255),
    ok: c(60, 240, 220),
    warn: c(255, 200, 80),
    alert: c(255, 70, 120),
};

fn rect(x: i32, y: i32, w: u32, h: u32) -> Rectangle {
    Rectangle::new(Point::new(x, y), Size::new(w, h))
}

fn fill(fb: &mut Fb, r: Rectangle, col: Rgb565) {
    r.into_styled(PrimitiveStyle::with_fill(col)).draw(fb).ok();
}

fn pill(fb: &mut Fb, r: Rectangle, radius: u32, col: Rgb565) {
    RoundedRectangle::new(r, CornerRadii::new(Size::new(radius, radius)))
        .into_styled(PrimitiveStyle::with_fill(col))
        .draw(fb)
        .ok();
}

fn text(fb: &mut Fb, s: &str, x: i32, y: i32, f: &MonoFont<'static>, col: Rgb565, a: Alignment) -> i32 {
    let ts = TextStyleBuilder::new().alignment(a).baseline(Baseline::Top).build();
    Text::with_text_style(s, Point::new(x, y), MonoTextStyle::new(f, col), ts).draw(fb).ok();
    x + f.character_size.width as i32 * s.len() as i32
}

/// A tag like "FM" in a small rounded box; returns the x after it.
fn tag(fb: &mut Fb, s: &str, x: i32, y: i32, bg: Rgb565, fg: Rgb565) -> i32 {
    let w = 5 * s.len() as u32 + 6;
    pill(fb, rect(x, y, w, 11), 3, bg);
    text(fb, s, x + 3, y + 2, &FONT_5X8, fg, Alignment::Left);
    x + w as i32 + 3
}

// Seven-segment digits: a,b,c,d,e,f,g as bits 0..6.
const SEGS: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];

fn seg_digit(fb: &mut Fb, d: u8, x: i32, y: i32, w: i32, h: i32, t: i32, col: Rgb565, off: Rgb565) {
    let m = SEGS[d as usize];
    let half = h / 2;
    let parts: [(i32, i32, i32, i32); 7] = [
        (x + t, y, w - 2 * t, t),                     // a
        (x + w - t, y + t, t, half - t),              // b
        (x + w - t, y + half + 1, t, half - t - 1),   // c
        (x + t, y + h - t, w - 2 * t, t),             // d
        (x, y + half + 1, t, half - t - 1),           // e
        (x, y + t, t, half - t),                      // f
        (x + t, y + half - t / 2, w - 2 * t, t),      // g
    ];
    for (i, &(px, py, pw, ph)) in parts.iter().enumerate() {
        let on = m & (1 << i) != 0;
        if on || off != Rgb565::BLACK {
            pill(fb, rect(px, py, pw as u32, ph as u32), 1, if on { col } else { off });
        }
    }
}

/// "146.52000": big MHz.kHz digits, small last two digits.
fn big_freq(fb: &mut Fb, hz: u32, right: i32, y: i32, col: Rgb565, ghost: Rgb565, p: &Pal) {
    let mhz = hz / 1_000_000;
    let khz = (hz / 1000) % 1000;
    let tail = (hz / 10) % 100;
    let (dw, dh, t, gap) = (13, 24, 3, 3);
    let digits: Vec<u8> = format!("{mhz:3}{khz:03}").bytes().map(|b| if b == b' ' { 10 } else { b - b'0' }).collect();
    let tail_w = 2 * 6;
    let total = 6 * (dw + gap) + 5 + tail_w + 2;
    let mut x = right - total;
    for (i, &d) in digits.iter().enumerate() {
        if d < 10 {
            seg_digit(fb, d, x, y, dw, dh, t, col, ghost);
        }
        x += dw + gap;
        if i == 2 {
            pill(fb, rect(x - 1, y + dh - t, t as u32, t as u32), 1, col);
            x += 5;
        }
    }
    text(fb, &format!("{tail:02}"), x + 1, y + dh - 10, &FONT_6X10, p.dim, Alignment::Left);
}

fn status_bar(fb: &mut Fb, p: &Pal, left: &str) {
    fill(fb, rect(0, 0, 160, 12), p.card);
    text(fb, left, 4, 2, &FONT_5X8, p.dim, Alignment::Left);
    // battery: outline, 4 bars, nub
    let bx = 138;
    pill(fb, rect(bx, 2, 17, 8), 1, p.dim);
    fill(fb, rect(bx + 1, 3, 15, 6), p.card);
    for i in 0..4 {
        fill(fb, rect(bx + 2 + i * 4, 4, 3, 4), p.ok);
    }
    fill(fb, rect(bx + 17, 4, 2, 4), p.dim);
    text(fb, "7.9V", bx - 4, 2, &FONT_5X8, p.dim, Alignment::Right);
}

fn soft_keys(fb: &mut Fb, p: &Pal, keys: &[(&str, &str)]) {
    fill(fb, rect(0, 116, 160, 12), p.card);
    let n = keys.len() as i32;
    let cell = 160 / n;
    for (i, (k, label)) in keys.iter().enumerate() {
        let x0 = i as i32 * cell;
        let kw = 5 * k.len() as i32 + 4;
        let lw = 5 * label.len() as i32;
        let x = x0 + (cell - (kw + 2 + lw)) / 2;
        pill(fb, rect(x, 118, kw as u32, 9), 2, p.card_hi);
        text(fb, k, x + 2, 118, &FONT_5X8, p.accent, Alignment::Left);
        text(fb, label, x + kw + 2, 118, &FONT_5X8, p.dim, Alignment::Left);
    }
}

fn meter(fb: &mut Fb, p: &Pal, y: i32, level: u8, label: &str) {
    // 15 segments: 1-9 then +10..+60, warm toward the top
    let x0 = 6;
    for i in 0..15u8 {
        let x = x0 + i as i32 * 7;
        let h = 4 + (i as i32 * 6) / 14;
        let col = if i >= level {
            p.card_hi
        } else if i < 9 {
            p.ok
        } else {
            p.warn
        };
        fill(fb, rect(x, y + 10 - h, 5, h as u32), col);
    }
    text(fb, label, 154, y + 2, &FONT_6X10, p.text, Alignment::Right);
}

enum HomeState {
    Receive,
    Typing,
    Transmit,
}

fn home(p: &Pal, state: HomeState) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let fb_ = &mut fb;
    status_bar(fb_, p, "SCAN: ALL");

    // Main side card.
    let top = 15;
    let (edge, card) = match state {
        HomeState::Transmit => (p.alert, p.card),
        _ => (p.accent, p.card),
    };
    pill(fb_, rect(3, top, 154, 66), 6, card);
    pill(fb_, rect(3, top, 4, 66), 2, edge);

    // Line 1: memory + name, RX/TX badge right.
    text(fb_, "M001", 12, top + 4, &FONT_5X8, p.dim, Alignment::Left);
    text(fb_, "CALL 2M", 36, top + 3, &FONT_6X10, p.text, Alignment::Left);
    match state {
        HomeState::Receive => {
            pill(fb_, rect(130, top + 3, 22, 11), 3, p.ok);
            text(fb_, "RX", 135, top + 5, &FONT_5X8, p.on_accent_for(p.ok), Alignment::Left);
        }
        HomeState::Transmit => {
            pill(fb_, rect(130, top + 3, 22, 11), 3, p.alert);
            text(fb_, "TX", 135, top + 5, &FONT_5X8, Rgb565::WHITE, Alignment::Left);
        }
        HomeState::Typing => {}
    }

    // Line 2: frequency.
    match state {
        HomeState::Typing => {
            // Typed digits bright, the rest as ghost segments with dashes.
            let ghost = p.card_hi;
            let typed = [1u8, 4, 6];
            let (dw, dh, t, gap) = (13, 24, 3, 3);
            let mut x = 152 - (6 * (dw + gap) + 5 + 14);
            for i in 0..6 {
                if i < typed.len() {
                    seg_digit(fb_, typed[i], x, top + 18, dw, dh, t, p.warn, ghost);
                } else {
                    seg_digit(fb_, 8, x, top + 18, dw, dh, t, ghost, Rgb565::BLACK);
                    pill(fb_, rect(x + 2, top + 18 + dh / 2 - 1, (dw - 4) as u32, 3), 1, p.warn);
                }
                x += dw + gap;
                if i == 2 {
                    pill(fb_, rect(x - 1, top + 18 + dh - t, t as u32, t as u32), 1, p.warn);
                    x += 5;
                }
            }
        }
        _ => big_freq(fb_, 146_520_000, 152, top + 18, p.text, p.card_hi, p),
    }

    // Line 3: tags.
    let mut x = 12;
    x = tag(fb_, "FM", x, top + 50, p.card_hi, p.text);
    x = tag(fb_, "LOW", x, top + 50, p.card_hi, p.text);
    x = tag(fb_, "WIDE", x, top + 50, p.card_hi, p.text);
    x = tag(fb_, "T 88.5", x, top + 50, p.card_hi, p.accent);
    let _ = tag(fb_, "+600K", x, top + 50, p.card_hi, p.accent);

    // Meter or TX timer.
    match state {
        HomeState::Transmit => {
            text(fb_, "MIC", 6, 86, &FONT_5X8, p.dim, Alignment::Left);
            for i in 0..12 {
                let col = if i < 8 { p.alert } else { p.card_hi };
                fill(fb_, rect(26 + i * 7, 86, 5, 7), col);
            }
            text(fb_, "0:07", 154, 84, &FONT_6X10, p.text, Alignment::Right);
        }
        HomeState::Typing => {
            text(fb_, "TYPE 6 DIGITS, OR MENU", 80, 86, &FONT_5X8, p.warn, Alignment::Center);
        }
        HomeState::Receive => meter(fb_, p, 82, 12, "S9+35"),
    }

    // Other side, one line.
    pill(fb_, rect(3, 99, 154, 14), 4, p.bg);
    text(fb_, "B", 8, 102, &FONT_6X10, p.dim, Alignment::Left);
    text(fb_, "VFO", 18, 103, &FONT_5X8, p.dim, Alignment::Left);
    text(fb_, "446.00625", 118, 102, &FONT_6X10, p.dim, Alignment::Right);
    text(fb_, "FM", 152, 103, &FONT_5X8, p.dim, Alignment::Right);

    match state {
        HomeState::Typing => soft_keys(fb_, p, &[("MENU", "OK"), ("EXIT", "CLEAR")]),
        HomeState::Transmit => soft_keys(fb_, p, &[("PTT", "RELEASE TO RX")]),
        HomeState::Receive => soft_keys(fb_, p, &[("MENU", "APPS"), ("A/B", "SIDE"), ("V/M", "MEM")]),
    }
    fb
}

impl Pal {
    fn on_accent_for(&self, _bg: Rgb565) -> Rgb565 {
        self.on_accent
    }
}

fn menu(p: &Pal, title: &str, items: &[(&str, &str)], sel: usize, top: usize, total: usize) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let fb_ = &mut fb;
    // Title with position.
    fill(fb_, rect(0, 0, 160, 16), p.card);
    text(fb_, title, 6, 3, &FONT_6X10, p.text, Alignment::Left);
    text(fb_, &format!("{}/{}", sel + 1, total), 154, 4, &FONT_5X8, p.dim, Alignment::Right);
    fill(fb_, rect(0, 16, 160, 1), p.accent);

    let row_h = 16;
    for (i, (label, value)) in items.iter().enumerate() {
        let y = 20 + i as i32 * row_h;
        let selected = top + i == sel;
        if selected {
            pill(fb_, rect(3, y, 150, (row_h - 2) as u32), 5, p.accent);
        }
        let fg = if selected { p.on_accent } else { p.text };
        text(fb_, label, 10, y + 2, &FONT_6X10, fg, Alignment::Left);
        if !value.is_empty() {
            let vc = if selected { p.on_accent } else { p.dim };
            text(fb_, value, 140, y + 2, &FONT_6X10, vc, Alignment::Right);
        }
        if selected {
            // chevron
            let cx = 146;
            let cy = y + 7;
            Triangle::new(Point::new(cx, cy - 3), Point::new(cx, cy + 3), Point::new(cx + 3, cy))
                .into_styled(PrimitiveStyle::with_fill(p.on_accent))
                .draw(fb_)
                .ok();
        }
    }
    // Scrollbar.
    let track_y = 20;
    let track_h = 6 * row_h - 4;
    fill(fb_, rect(156, track_y, 2, track_h as u32), p.card);
    let thumb_h = (track_h as usize * items.len() / total).max(8) as i32;
    let thumb_y = track_y + (track_h - thumb_h) * top as i32 / (total - items.len()).max(1) as i32;
    fill(fb_, rect(156, thumb_y, 2, thumb_h as u32), p.accent);

    soft_keys(fb_, p, &[("^v", "MOVE"), ("MENU", "OPEN"), ("EXIT", "BACK")]);
    fb
}

fn setting(p: &Pal) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let fb_ = &mut fb;
    fill(fb_, rect(0, 0, 160, 16), p.card);
    text(fb_, "RADIO > SQUELCH", 6, 3, &FONT_6X10, p.text, Alignment::Left);
    fill(fb_, rect(0, 16, 160, 1), p.accent);

    text(fb_, "How strong a signal must be", 80, 26, &FONT_5X8, p.dim, Alignment::Center);
    text(fb_, "before you hear it.", 80, 36, &FONT_5X8, p.dim, Alignment::Center);

    // Big value with arrows.
    pill(fb_, rect(40, 52, 80, 34), 8, p.card);
    text(fb_, "3", 80, 60, &FONT_9X18, p.text, Alignment::Center);
    for (x, dir) in [(30, -1), (130, 1)] {
        let cy = 69;
        Triangle::new(Point::new(x - 4 * dir, cy - 6), Point::new(x - 4 * dir, cy + 6), Point::new(x + 4 * dir, cy))
            .into_styled(PrimitiveStyle::with_fill(p.accent))
            .draw(fb_)
            .ok();
    }
    // Position dots 0..9.
    for i in 0..10 {
        let col = if i == 3 { p.accent } else { p.card_hi };
        pill(fb_, rect(36 + i * 9, 94, 6, 6), 3, col);
    }
    text(fb_, "OFF", 34, 103, &FONT_5X8, p.dim, Alignment::Left);
    text(fb_, "TIGHT", 126, 103, &FONT_5X8, p.dim, Alignment::Right);
    soft_keys(fb_, p, &[("^v", "CHANGE"), ("MENU", "SAVE"), ("EXIT", "BACK")]);
    fb
}

/// Medium segment frequency for the dual-watch cards.
fn mid_freq(fb: &mut Fb, hz: u32, right: i32, y: i32, col: Rgb565, ghost: Rgb565, p: &Pal) {
    let (dw, dh, t, gap) = (10, 18, 2, 3);
    let digits: Vec<u8> = format!("{:3}{:03}", hz / 1_000_000, (hz / 1000) % 1000)
        .bytes()
        .map(|b| if b == b' ' { 10 } else { b - b'0' })
        .collect();
    let mut x = right - (6 * (dw + gap) + 4 + 12 + 2);
    for (i, &d) in digits.iter().enumerate() {
        if d < 10 {
            seg_digit(fb, d, x, y, dw, dh, t, col, ghost);
        }
        x += dw + gap;
        if i == 2 {
            pill(fb, rect(x - 1, y + dh - t, t as u32, t as u32), 1, col);
            x += 4;
        }
    }
    text(fb, &format!("{:02}", (hz / 10) % 100), x + 1, y + dh - 9, &FONT_5X8, p.dim, Alignment::Left);
}

/// Dual watch: both sides at equal size. The side you transmit on (A) has
/// the accent edge; the side that is receiving right now (B) shows RX and a
/// signal bar.
fn dual(p: &Pal) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let fb_ = &mut fb;
    status_bar(fb_, p, "DUAL WATCH");
    let sides: [(&str, &str, &str, u32, bool, bool, &[&str]); 2] = [
        ("A", "M001", "CALL 2M", 146_520_000, true, false, &["FM", "LOW", "W"]),
        ("B", "M004", "GMRS 1", 462_562_500, false, true, &["FM", "HIGH", "T 141.3"]),
    ];
    for (i, (side, mem, name, hz, master, rx, tags)) in sides.into_iter().enumerate() {
        let top = 14 + i as i32 * 51;
        pill(fb_, rect(3, top, 154, 49), 6, p.card);
        if master {
            pill(fb_, rect(3, top, 4, 49), 2, p.accent);
        }
        let label_col = if master { p.accent } else { p.dim };
        text(fb_, side, 11, top + 3, &FONT_6X10, label_col, Alignment::Left);
        text(fb_, mem, 21, top + 4, &FONT_5X8, p.dim, Alignment::Left);
        text(fb_, name, 45, top + 3, &FONT_6X10, p.text, Alignment::Left);
        if rx {
            pill(fb_, rect(130, top + 3, 22, 11), 3, p.ok);
            text(fb_, "RX", 135, top + 5, &FONT_5X8, p.on_accent, Alignment::Left);
        }
        let fcol = if master || rx { p.text } else { p.dim };
        mid_freq(fb_, hz, 152, top + 16, fcol, p.card_hi, p);
        let mut x = 11;
        for t in tags {
            x = tag(fb_, t, x, top + 36, p.card_hi, if t.starts_with('T') { p.accent } else { p.text });
        }
        if rx {
            // signal strength, compact
            for s in 0..9 {
                let col = if s < 7 { p.ok } else { p.card_hi };
                fill(fb_, rect(112 + s * 4, top + 44 - s / 2, 3, (3 + s / 2) as u32), col);
            }
        } else {
            text(fb_, "TX", 152, top + 38, &FONT_5X8, if master { p.accent } else { p.card_hi }, Alignment::Right);
        }
    }
    soft_keys(fb_, p, &[("A/B", "SWAP"), ("MENU", "APPS"), ("V/M", "MEM")]);
    fb
}

// ---------------------------------------------------------------------------
// Version 2: bolder text, big channel number, status icons, no key hints, and
// two frequency styles (bigger segments, or the 9x18 font drawn at 2x).
// ---------------------------------------------------------------------------

/// Draws with every pixel blown up to k x k, offset by (ox, oy): the 9x18
/// font at 2x without adding any font data to the firmware.
struct Scaled<'a> {
    fb: &'a mut Fb,
    ox: i32,
    oy: i32,
    k: i32,
}

impl OriginDimensions for Scaled<'_> {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Scaled<'_> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, it: I) -> Result<(), Self::Error> {
        let k = self.k;
        for Pixel(p, c) in it {
            for dy in 0..k {
                for dx in 0..k {
                    let (x, y) = (self.ox + p.x * k + dx, self.oy + p.y * k + dy);
                    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
                        self.fb.0[y as usize * W + x as usize] = RawU16::from(c).into_inner();
                    }
                }
            }
        }
        Ok(())
    }
}

fn big_text(fb: &mut Fb, s: &str, x: i32, y: i32, k: i32, col: Rgb565) -> i32 {
    let ts = TextStyleBuilder::new().baseline(Baseline::Top).build();
    let mut t = Scaled { fb, ox: x, oy: y, k };
    Text::with_text_style(s, Point::zero(), MonoTextStyle::new(&FONT_9X18, col), ts).draw(&mut t).ok();
    x + 9 * k * s.len() as i32
}

/// Faux bold: the same glyphs twice, one pixel apart. Code, not font data.
fn bold(fb: &mut Fb, s: &str, x: i32, y: i32, f: &MonoFont<'static>, col: Rgb565, a: Alignment) -> i32 {
    text(fb, s, x + 1, y, f, col, a);
    text(fb, s, x, y, f, col, a) + 1
}

fn tag2(fb: &mut Fb, s: &str, x: i32, y: i32, bg: Rgb565, fg: Rgb565) -> i32 {
    let w = 6 * s.len() as u32 + 7;
    pill(fb, rect(x, y, w, 14), 4, bg);
    bold(fb, s, x + 3, y + 2, &FONT_6X10, fg, Alignment::Left);
    x + w as i32 + 3
}

fn padlock(fb: &mut Fb, x: i32, y: i32, col: Rgb565) {
    rect(x + 1, y, 6, 6).into_styled(PrimitiveStyle::with_stroke(col, 1)).draw(fb).ok();
    fill(fb, rect(x, y + 4, 8, 6), col);
}

struct Icons {
    lock: bool,
    vox: bool,
    mon: bool,
    scan: bool,
}

fn status_bar2(fb: &mut Fb, p: &Pal, list: &str, icons: &Icons) {
    fill(fb, rect(0, 0, 160, 14), p.card);
    let mut x = 3;
    pill(fb, rect(x, 1, 6 * list.len() as u32 + 6, 12), 3, p.card_hi);
    bold(fb, list, x + 3, 2, &FONT_6X10, p.accent, Alignment::Left);
    x += 6 * list.len() as i32 + 10;
    if icons.lock {
        padlock(fb, x, 2, p.warn);
        x += 12;
    }
    for (on, label, col) in [(icons.vox, "VOX", p.ok), (icons.mon, "MON", p.warn), (icons.scan, "SCN", p.accent)] {
        if on {
            pill(fb, rect(x, 2, 19, 10), 2, col);
            text(fb, label, x + 2, 3, &FONT_5X8, p.bg, Alignment::Left);
            x += 22;
        }
    }
    // Battery and voltage.
    let bx = 136;
    pill(fb, rect(bx, 3, 20, 9), 1, p.dim);
    fill(fb, rect(bx + 1, 4, 18, 7), p.card);
    for i in 0..4 {
        fill(fb, rect(bx + 2 + i * 4, 5, 3, 5), p.ok);
    }
    fill(fb, rect(bx + 20, 5, 2, 5), p.dim);
    bold(fb, "7.9V", bx - 3, 3, &FONT_6X10, p.text, Alignment::Right);
}

#[derive(Clone, Copy, PartialEq)]
enum FreqStyle {
    Seg,
    Bold,
}

#[derive(Clone, Copy, PartialEq)]
enum State2 {
    Rx,
    Typing,
    Tx,
}

/// Frequency right-aligned at `right`, `tall` sets the size for each style.
fn freq2(fb: &mut Fb, p: &Pal, style: FreqStyle, hz: u32, right: i32, y: i32, big: bool, col: Rgb565, typed: Option<&str>) {
    let main = match typed {
        Some(t) => {
            let mut s: String = t.chars().take(3).collect();
            s.push('.');
            s.extend(t.chars().skip(3));
            while s.len() < 7 {
                s.push('-');
            }
            s
        }
        None => format!("{:3}.{:03}", hz / 1_000_000, (hz / 1000) % 1000),
    };
    let tail = format!("{:02}", (hz / 10) % 100);
    match style {
        FreqStyle::Bold => {
            // 9x18 at 2x, drawn twice a pixel apart so strokes are 3 px, and the
            // dot only gets half a cell.
            let w = 18 * 6 + 10 + 14;
            let mut x = right - w;
            for ch in main.chars() {
                let c = if ch == '-' { p.card_hi } else { col };
                let s = ch.to_string();
                if ch == '.' {
                    big_text(fb, &s, x - 4, y - 6, 2, c);
                    big_text(fb, &s, x - 3, y - 6, 2, c);
                    x += 10;
                } else {
                    big_text(fb, &s, x, y - 6, 2, c);
                    big_text(fb, &s, x + 1, y - 6, 2, c);
                    x += 18;
                }
            }
            if typed.is_none() {
                bold(fb, &tail, x + 1, y + 16, &FONT_6X10, p.dim, Alignment::Left);
            }
        }
        FreqStyle::Seg => {
            let (dw, dh, t, gap) = if big { (16, 30, 4, 3) } else { (14, 26, 3, 3) };
            let total = 6 * (dw + gap) + 5 + 13;
            let mut x = right - total;
            let mut d = 0;
            for ch in main.chars() {
                if ch == '.' {
                    pill(fb, rect(x - 1, y + dh - t, t as u32, t as u32), 1, col);
                    x += 5;
                    continue;
                }
                match ch.to_digit(10) {
                    Some(v) => seg_digit(fb, v as u8, x, y, dw, dh, t, col, p.card_hi),
                    None if ch == '-' => {
                        seg_digit(fb, 8, x, y, dw, dh, t, p.card_hi, Rgb565::BLACK);
                        pill(fb, rect(x + 2, y + dh / 2 - 1, (dw - 4) as u32, t as u32), 1, col);
                    }
                    None => {}
                }
                x += dw + gap;
                d += 1;
            }
            let _ = d;
            if typed.is_none() {
                bold(fb, &tail, x + 1, y + dh - 10, &FONT_6X10, p.dim, Alignment::Left);
            }
        }
    }
}

fn badge(fb: &mut Fb, s: &str, right: i32, y: i32, bg: Rgb565, fg: Rgb565) {
    pill(fb, rect(right - 24, y, 24, 14), 4, bg);
    bold(fb, s, right - 12, y + 2, &FONT_6X10, fg, Alignment::Center);
}

fn home2(p: &Pal, style: FreqStyle, state: State2) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let f = &mut fb;
    let icons = Icons { lock: state == State2::Rx, vox: state == State2::Rx, mon: false, scan: false };
    status_bar2(f, p, "ALL", &icons);

    let top = 16;
    let edge = if state == State2::Tx { p.alert } else { p.accent };
    pill(f, rect(2, top, 156, 80), 6, p.card);
    pill(f, rect(2, top, 4, 80), 2, edge);

    // Big channel number, then the name.
    pill(f, rect(10, top + 4, 33, 20), 4, p.card_hi);
    bold(f, "001", 13, top + 5, &FONT_9X18, p.accent, Alignment::Left);
    bold(f, "CALL 2M", 49, top + 5, &FONT_9X18, p.text, Alignment::Left);
    match state {
        State2::Rx => badge(f, "RX", 155, top + 7, p.ok, p.bg),
        State2::Tx => badge(f, "TX", 155, top + 7, p.alert, Rgb565::WHITE),
        State2::Typing => {}
    }

    let fcol = if state == State2::Typing { p.warn } else { p.text };
    let typed = (state == State2::Typing).then_some("146");
    freq2(f, p, style, 146_520_000, 155, top + 28, true, fcol, typed);

    if state == State2::Typing {
        bold(f, "TYPE 6 DIGITS", 80, top + 64, &FONT_6X10, p.warn, Alignment::Center);
    } else {
        let mut x = 10;
        x = tag2(f, "FM", x, top + 62, p.card_hi, p.text);
        x = tag2(f, "LOW", x, top + 62, p.card_hi, p.text);
        x = tag2(f, "T88.5", x, top + 62, p.card_hi, p.accent);
        tag2(f, "+600K", x, top + 62, p.card_hi, p.accent);
    }

    // Meter, or mic level while transmitting.
    let my = 99;
    if state == State2::Tx {
        bold(f, "MIC", 4, my + 1, &FONT_6X10, p.dim, Alignment::Left);
        for i in 0..12 {
            fill(f, rect(28 + i * 7, my + 2, 5, 8), if i < 8 { p.alert } else { p.card_hi });
        }
        bold(f, "0:07", 155, my + 1, &FONT_6X10, p.text, Alignment::Right);
    } else {
        let level = if state == State2::Rx { 12 } else { 0 };
        for i in 0..14 {
            let h = 4 + i * 6 / 13;
            let col = if i >= level { p.card_hi } else if i < 9 { p.ok } else { p.warn };
            fill(f, rect(4 + i * 7, my + 11 - h, 5, h as u32), col);
        }
        if state == State2::Rx {
            bold(f, "S9+35", 155, my + 1, &FONT_6X10, p.text, Alignment::Right);
        }
    }

    // The other side, one bold line.
    pill(f, rect(2, 114, 156, 14), 4, p.card);
    bold(f, "B", 7, 116, &FONT_6X10, p.dim, Alignment::Left);
    bold(f, "VFO", 18, 116, &FONT_6X10, p.dim, Alignment::Left);
    bold(f, "446.00625", 131, 116, &FONT_6X10, p.dim, Alignment::Right);
    bold(f, "FM", 155, 116, &FONT_6X10, p.dim, Alignment::Right);
    fb
}

fn dual2(p: &Pal, style: FreqStyle) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let f = &mut fb;
    status_bar2(f, p, "ALL", &Icons { lock: false, vox: false, mon: false, scan: true });
    let sides = [
        ("A", "001", "CALL 2M", 146_520_000u32, true, false, "FM L"),
        ("B", "004", "GMRS 1", 462_562_500u32, false, true, "FM H"),
    ];
    for (i, (side, ch, name, hz, master, rx, mode)) in sides.into_iter().enumerate() {
        let top = 16 + i as i32 * 56;
        pill(f, rect(2, top, 156, 54), 6, p.card);
        if master {
            pill(f, rect(2, top, 4, 54), 2, p.accent);
        }
        bold(f, side, 9, top + 3, &FONT_6X10, if master { p.accent } else { p.dim }, Alignment::Left);
        pill(f, rect(18, top + 2, 23, 13), 3, p.card_hi);
        bold(f, ch, 20, top + 3, &FONT_6X10, p.accent, Alignment::Left);
        bold(f, name, 46, top + 3, &FONT_6X10, p.text, Alignment::Left);
        if rx {
            badge(f, "RX", 155, top + 2, p.ok, p.bg);
        } else {
            bold(f, mode, 155, top + 3, &FONT_6X10, p.dim, Alignment::Right);
        }
        let col = if master || rx { p.text } else { p.dim };
        freq2(f, p, style, hz, 155, top + 20, false, col, None);
        if rx {
            for s in 0..20 {
                fill(f, rect(8 + s * 7, top + 49, 5, 3), if s < 15 { p.ok } else { p.card_hi });
            }
        }
    }
    fb
}

fn menu2(p: &Pal, title: &str, items: &[(&str, &str)], sel: usize, total: usize) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let f = &mut fb;
    fill(f, rect(0, 0, 160, 20), p.card);
    bold(f, title, 6, 1, &FONT_9X18, p.text, Alignment::Left);
    bold(f, &format!("{}/{}", sel + 1, total), 155, 5, &FONT_6X10, p.dim, Alignment::Right);
    fill(f, rect(0, 20, 160, 2), p.accent);
    let row_h = 17;
    for (i, (label, value)) in items.iter().enumerate() {
        let y = 25 + i as i32 * row_h;
        let selected = i == sel;
        if selected {
            pill(f, rect(2, y, 150, 15), 5, p.accent);
        }
        let fg = if selected { p.on_accent } else { p.text };
        bold(f, label, 9, y + 3, &FONT_6X10, fg, Alignment::Left);
        if !value.is_empty() {
            bold(f, value, 145, y + 3, &FONT_6X10, if selected { p.on_accent } else { p.accent }, Alignment::Right);
        }
    }
    let track = 6 * row_h - 2;
    fill(f, rect(155, 25, 3, track as u32), p.card);
    let th = (track as usize * items.len() / total).max(10) as i32;
    fill(f, rect(155, 25, 3, th as u32), p.accent);
    fb
}

fn value2(p: &Pal) -> Fb {
    let mut fb = Fb(vec![RawU16::from(p.bg).into_inner(); W * H]);
    let f = &mut fb;
    fill(f, rect(0, 0, 160, 20), p.card);
    bold(f, "SQUELCH", 6, 1, &FONT_9X18, p.text, Alignment::Left);
    fill(f, rect(0, 20, 160, 2), p.accent);
    bold(f, "How strong a signal", 80, 27, &FONT_6X10, p.dim, Alignment::Center);
    bold(f, "must be to be heard", 80, 38, &FONT_6X10, p.dim, Alignment::Center);
    pill(f, rect(44, 52, 72, 42), 8, p.card);
    big_text(f, "3", 71, 52, 2, p.text);
    for (x, dir) in [(32, -1), (128, 1)] {
        let cy = 73;
        Triangle::new(Point::new(x - 5 * dir, cy - 8), Point::new(x - 5 * dir, cy + 8), Point::new(x + 5 * dir, cy))
            .into_styled(PrimitiveStyle::with_fill(p.accent))
            .draw(f)
            .ok();
    }
    for i in 0..10 {
        pill(f, rect(35 + i * 9, 101, 7, 7), 3, if i == 3 { p.accent } else { p.card_hi });
    }
    bold(f, "OPEN", 32, 113, &FONT_6X10, p.dim, Alignment::Left);
    bold(f, "TIGHT", 128, 113, &FONT_6X10, p.dim, Alignment::Right);
    fb
}

fn save_bmp(path: &Path, fb: &Fb, scale: usize) {
    let (w, h) = (W * scale, H * scale);
    let row = (w * 3 + 3) & !3;
    let mut out = Vec::with_capacity(54 + row * h);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + row * h) as u32).to_le_bytes());
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
            let p = fb.0[(y / scale) * W + x / scale];
            let r = (((p >> 11) & 0x1F) * 255 / 31) as u8;
            let g = (((p >> 5) & 0x3F) * 255 / 63) as u8;
            let b = ((p & 0x1F) * 255 / 31) as u8;
            out.extend_from_slice(&[b, g, r]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out).unwrap();
}

fn main() {
    let dir = std::env::args().nth(1).expect("output folder");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).ok();
    let apps = [
        ("SETTINGS", ""),
        ("CHANNELS", "5"),
        ("FM RADIO", ""),
        ("TEXT", ""),
        ("SPECTRUM", ""),
        ("FOX HUNT", ""),
    ];
    let settings = [
        ("SQUELCH", "3"),
        ("TX POWER", "LOW"),
        ("BACKLIGHT", "10S"),
        ("KEY BEEP", "ON"),
        ("SIDE KEY 1", "FM RADIO"),
        ("ROGER BEEP", "OFF"),
    ];
    for (name, p) in [("arctic", ARCTIC), ("violet", VIOLET)] {
        save_bmp(&dir.join(format!("{name}_home_rx.bmp")), &home(&p, HomeState::Receive), 4);
        save_bmp(&dir.join(format!("{name}_home_typing.bmp")), &home(&p, HomeState::Typing), 4);
        save_bmp(&dir.join(format!("{name}_home_tx.bmp")), &home(&p, HomeState::Transmit), 4);
        save_bmp(&dir.join(format!("{name}_menu.bmp")), &menu(&p, "APPS", &apps, 3, 0, 11), 4);
        save_bmp(&dir.join(format!("{name}_settings.bmp")), &menu(&p, "RADIO", &settings, 0, 0, 14), 4);
        save_bmp(&dir.join(format!("{name}_value.bmp")), &setting(&p), 4);
        save_bmp(&dir.join(format!("{name}_dual.bmp")), &dual(&p), 4);
        for (sname, st) in [("seg", FreqStyle::Seg), ("bold", FreqStyle::Bold)] {
            save_bmp(&dir.join(format!("v2_{name}_home_{sname}.bmp")), &home2(&p, st, State2::Rx), 4);
            save_bmp(&dir.join(format!("v2_{name}_typing_{sname}.bmp")), &home2(&p, st, State2::Typing), 4);
            save_bmp(&dir.join(format!("v2_{name}_tx_{sname}.bmp")), &home2(&p, st, State2::Tx), 4);
            save_bmp(&dir.join(format!("v2_{name}_dual_{sname}.bmp")), &dual2(&p, st), 4);
        }
        let apps2 = [("SETTINGS", ""), ("CHANNELS", "5"), ("FM RADIO", ""), ("TEXT", ""), ("SPECTRUM", ""), ("FOX HUNT", "")];
        save_bmp(&dir.join(format!("v2_{name}_menu.bmp")), &menu2(&p, "APPS", &apps2, 2, 11), 4);
        let set2 = [("THEME", if name == "arctic" { "BLUE" } else { "PURPLE" }), ("SQUELCH", "3"), ("BACKLIGHT", "10S"), ("KEY BEEP", "ON"), ("DUAL WATCH", "ON"), ("SIDE KEY 1", "FM")];
        save_bmp(&dir.join(format!("v2_{name}_settings.bmp")), &menu2(&p, "SETTINGS", &set2, 0, 14), 4);
        save_bmp(&dir.join(format!("v2_{name}_value.bmp")), &value2(&p), 4);
        let disp = [("THEME", if name == "arctic" { "BLUE" } else { "PURPLE" }), ("BRIGHTNESS", "4"), ("BACKLIGHT", "10S"), ("SHOW", "NAME+FREQ"), ("DUAL WATCH", "ON"), ("BOOT LOGO", "SKULL")];
        save_bmp(&dir.join(format!("{name}_display.bmp")), &menu(&p, "DISPLAY", &disp, 0, 0, 6), 4);
    }
    println!("mockups in {}", dir.display());
}
