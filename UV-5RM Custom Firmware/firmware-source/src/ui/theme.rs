//! Clean colour themes, picked by `Settings::theme` (bits 2..3 of settings
//! byte 30). Labels are `accent`, readouts `value`, rules and selections
//! `card_hi`. Index 3 is the daylight one that the DAY/NIGHT key switches to.

use embedded_graphics::pixelcolor::Rgb565;

pub struct Theme {
    /// Screen background.
    pub bg: Rgb565,
    /// Status bar and panels, a step off the background.
    pub card: Rgb565,
    /// Rules, unlit meter segments, inactive markings.
    pub card_hi: Rgb565,
    pub text: Rgb565,
    pub dim: Rgb565,
    /// Labels and headings.
    pub accent: Rgb565,
    /// Text drawn on `accent`.
    pub on_accent: Rgb565,
    /// Readouts: the frequency, selected values, and the frames around them.
    pub value: Rgb565,
    pub ok: Rgb565,
    pub warn: Rgb565,
    pub alert: Rgb565,
}

const fn c(r: u8, g: u8, b: u8) -> Rgb565 {
    Rgb565::new(r >> 3, g >> 2, b >> 3)
}

static THEMES: [Theme; 4] = [
    // 0: night, white readouts and blue labels on near-black
    Theme {
        bg: c(11, 13, 16),
        card: c(20, 23, 28),
        card_hi: c(38, 43, 51),
        text: c(242, 244, 247),
        dim: c(124, 133, 145),
        accent: c(79, 179, 255),
        on_accent: c(0, 0, 0),
        value: c(242, 244, 247),
        ok: c(61, 220, 132),
        warn: c(255, 194, 75),
        alert: c(255, 80, 80),
    },
    // 1: night vision, all green
    Theme {
        bg: c(0, 0, 0),
        card: c(0, 10, 2),
        card_hi: c(20, 60, 22),
        text: c(170, 255, 160),
        dim: c(70, 130, 70),
        accent: c(90, 230, 120),
        on_accent: c(0, 0, 0),
        value: c(150, 255, 110),
        ok: c(110, 255, 110),
        warn: c(230, 255, 110),
        alert: c(255, 90, 70),
    },
    // 2: high contrast, white readouts
    Theme {
        bg: c(0, 0, 0),
        card: c(10, 10, 10),
        card_hi: c(56, 56, 56),
        text: c(255, 255, 255),
        dim: c(150, 150, 150),
        accent: c(150, 200, 255),
        on_accent: c(0, 0, 0),
        value: c(255, 255, 255),
        ok: c(80, 255, 120),
        warn: c(255, 210, 80),
        alert: c(255, 70, 70),
    },
    // 3: day, dark ink on a light panel for sunlight
    Theme {
        bg: c(244, 241, 234),
        card: c(232, 228, 219),
        card_hi: c(214, 208, 197),
        text: c(18, 20, 23),
        dim: c(93, 100, 109),
        accent: c(0, 96, 192),
        on_accent: c(255, 255, 255),
        value: c(18, 20, 23),
        ok: c(10, 143, 74),
        warn: c(168, 100, 0),
        alert: c(190, 0, 0),
    },
];

pub const COUNT: u8 = THEMES.len() as u8;

static mut CURRENT: u8 = 0;

/// Selects a theme; an unknown index falls back to the classic one.
/// Returns whether it changed, so callers can repaint.
pub fn set(index: u8) -> bool {
    let index = if index < COUNT { index } else { 0 };
    let changed = unsafe { CURRENT } != index;
    unsafe { CURRENT = index };
    changed
}

#[inline(always)]
pub fn t() -> &'static Theme {
    &THEMES[unsafe { CURRENT } as usize]
}

pub fn sunlight() -> bool {
    unsafe { CURRENT == 3 }
}
