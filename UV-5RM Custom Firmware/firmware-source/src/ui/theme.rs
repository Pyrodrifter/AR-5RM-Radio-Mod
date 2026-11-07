//! Colour themes, picked by `Settings::theme` (high bits of settings byte 30).
//! Index 0 keeps the original black and red look.

use embedded_graphics::pixelcolor::Rgb565;

pub struct Theme {
    /// Screen background, where no card is drawn.
    pub bg: Rgb565,
    /// Cards, status bar and list rows.
    pub card: Rgb565,
    /// Raised parts of a card: tags, the channel box, unlit meter segments.
    pub card_hi: Rgb565,
    pub text: Rgb565,
    pub dim: Rgb565,
    pub accent: Rgb565,
    /// Text drawn on `accent`.
    pub on_accent: Rgb565,
    pub ok: Rgb565,
    pub warn: Rgb565,
    pub alert: Rgb565,
}

const fn c(r: u8, g: u8, b: u8) -> Rgb565 {
    Rgb565::new(r >> 3, g >> 2, b >> 3)
}

static THEMES: [Theme; 4] = [
    // 0: classic black and red
    Theme {
        bg: c(0, 0, 0),
        card: c(8, 8, 10),
        card_hi: c(28, 20, 24),
        text: c(255, 255, 255),
        dim: c(160, 150, 155),
        accent: c(240, 32, 48),
        on_accent: c(0, 0, 0),
        ok: c(0, 220, 50),
        warn: c(255, 200, 0),
        alert: c(255, 40, 40),
    },
    // 1: blue
    Theme {
        bg: c(0, 4, 12),
        card: c(4, 12, 24),
        card_hi: c(8, 28, 52),
        text: c(230, 242, 250),
        dim: c(140, 170, 195),
        accent: c(0, 205, 250),
        on_accent: c(0, 8, 16),
        ok: c(80, 230, 170),
        warn: c(255, 200, 90),
        alert: c(255, 80, 96),
    },
    // 2: purple
    Theme {
        bg: c(8, 0, 16),
        card: c(18, 6, 30),
        card_hi: c(38, 14, 60),
        text: c(242, 230, 255),
        dim: c(185, 158, 205),
        accent: c(220, 40, 235),
        on_accent: c(0, 0, 0),
        ok: c(60, 240, 220),
        warn: c(255, 200, 80),
        alert: c(255, 70, 120),
    },
    // 3: sunlight, with dark text on light cards
    Theme {
        bg: c(230, 230, 220),
        card: c(255, 255, 245),
        card_hi: c(210, 215, 205),
        text: c(0, 0, 0),
        dim: c(48, 52, 55),
        accent: c(0, 65, 125),
        on_accent: c(255, 255, 255),
        ok: c(0, 100, 45),
        warn: c(110, 65, 0),
        alert: c(155, 0, 0),
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
