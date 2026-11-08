//! The home screen's colour themes for apps, so they follow the radio's
//! theme and the DAY/NIGHT key. Call `load` at the top of `app_entry`, then
//! read colours with `pal()`.
//!
//! The firmware reports its theme as `settings_get(8)`; older firmware
//! answers 0, which is the night theme. This lives outside `bf5rh-abi` so the
//! ABI hash, and with it every installed app, stays valid.
#![no_std]

use bf5rh_abi::Api;

/// RGB565 colours, named as in the firmware's `ui::theme`.
pub struct Palette {
    /// Screen background.
    pub bg: u16,
    /// Title bars and panels, a step off the background.
    pub card: u16,
    /// Rules, grids, tracks and the selected row.
    pub card_hi: u16,
    pub text: u16,
    pub dim: u16,
    /// Labels, headings and graph traces.
    pub accent: u16,
    pub ok: u16,
    pub warn: u16,
    pub alert: u16,
}

const fn c(r: u16, g: u16, b: u16) -> u16 {
    ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
}

static PALETTES: [Palette; 4] = [
    // 0: night
    Palette {
        bg: c(11, 13, 16),
        card: c(20, 23, 28),
        card_hi: c(38, 43, 51),
        text: c(242, 244, 247),
        dim: c(124, 133, 145),
        accent: c(79, 179, 255),
        ok: c(61, 220, 132),
        warn: c(255, 194, 75),
        alert: c(255, 80, 80),
    },
    // 1: night vision
    Palette {
        bg: c(0, 0, 0),
        card: c(0, 10, 2),
        card_hi: c(20, 60, 22),
        text: c(170, 255, 160),
        dim: c(70, 130, 70),
        accent: c(90, 230, 120),
        ok: c(110, 255, 110),
        warn: c(230, 255, 110),
        alert: c(255, 90, 70),
    },
    // 2: high contrast
    Palette {
        bg: c(0, 0, 0),
        card: c(10, 10, 10),
        card_hi: c(56, 56, 56),
        text: c(255, 255, 255),
        dim: c(150, 150, 150),
        accent: c(150, 200, 255),
        ok: c(80, 255, 120),
        warn: c(255, 210, 80),
        alert: c(255, 70, 70),
    },
    // 3: day
    Palette {
        bg: c(244, 241, 234),
        card: c(232, 228, 219),
        card_hi: c(214, 208, 197),
        text: c(18, 20, 23),
        dim: c(93, 100, 109),
        accent: c(0, 96, 192),
        ok: c(10, 143, 74),
        warn: c(168, 100, 0),
        alert: c(190, 0, 0),
    },
];

static mut CURRENT: u8 = 0;

/// Reads the radio's theme. Cheap enough to call on every event.
#[inline(never)]
pub fn load(api: &Api) {
    unsafe { CURRENT = (api.settings_get)(8) as u8 & 3 };
}

/// Kept out of line: apps use colours in many places and have little room.
#[inline(never)]
pub fn pal() -> &'static Palette {
    &PALETTES[unsafe { CURRENT } as usize & 3]
}
