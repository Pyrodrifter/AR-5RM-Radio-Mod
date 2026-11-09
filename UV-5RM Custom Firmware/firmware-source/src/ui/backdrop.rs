use crate::flash_map::addr::{BACKGROUND_ADDR, BACKGROUND_MARKER_ADDR, BOOT_LOGO_ADDR};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use super::{list::fill, SCREEN_W};

const LINE_BYTES: usize = SCREEN_W as usize * 2;
const CARRIER_TOP: i32 = 28;
const CARRIER_H: i32 = 26;
const OUTER_ARC_H: i32 = 34;
const STRETCH_ARC: u8 = 5;

/// Copies part of a 160x128 RGB565 image from flash to the screen, one line at
/// a time (a whole image is more than twice the radio's RAM). Effects 1–4
/// change brightness for readable text. STRETCH_ARC preserves the colors
/// and enlarges Carrier Lock's outer wave vertically.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn paint_image<D>(
    lcd: &mut D,
    read: &mut dyn FnMut(u32, &mut [u8]),
    base: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    effect: u8,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line = [0u8; LINE_BYTES];
    let line = &mut line[..w as usize * 2];
    for row in y..y + h {
        let source_row = if effect == STRETCH_ARC {
            CARRIER_TOP + (row - y) * CARRIER_H / OUTER_ARC_H
        } else {
            row
        };
        read(base + source_row as u32 * LINE_BYTES as u32 + x as u32 * 2, line);
        let area = Rectangle::new(Point::new(x, row), Size::new(w as u32, 1));
        let pixels = line.chunks_exact(2).map(|b| {
            let raw = u16::from_le_bytes([b[0], b[1]]);
            Rgb565::from(RawU16::new(match effect {
                0 | STRETCH_ARC => raw,
                _ if raw == 0xFFFF && base == BOOT_LOGO_ADDR => 0,
                1 => (raw >> 1) & 0x7BEF,
                2 => (raw >> 2) & 0x39E7,
                3 => ((raw >> 1) & 0x7BEF) + 0x7BEF,
                _ => ((raw >> 2) & 0x39E7) + 0xBDF7,
            }))
        });
        lcd.fill_contiguous(&area, pixels).ok();
    }
}

pub(super) fn draw_image<D>(
    lcd: &mut D,
    read: &mut dyn FnMut(u32, &mut [u8]),
    y: i32,
    h: i32,
    dimmed: bool,
) where
    D: DrawTarget<Color = Rgb565>,
{
    paint_image(lcd, read, BOOT_LOGO_ADDR, 0, y, SCREEN_W, h, dimmed as u8);
}

/// Carrier Lock's arc positions are specific to the distributed RGB565 artwork.
#[cfg(feature = "ar5rm-nrf")]
pub(super) fn is_carrier_lock(read: &mut dyn FnMut(u32, &mut [u8])) -> bool {
    let mut signature = [0; 8];
    read(BOOT_LOGO_ADDR + 2 * (38 * SCREEN_W as u32 + 53), &mut signature);
    u64::from_le_bytes(signature) == 0x3d9f_3459_1082_10a3
}

#[cfg(feature = "ar5rm-nrf")]
#[inline(always)]
pub(super) fn draw_carrier_step<D>(lcd: &mut D, read: &mut dyn FnMut(u32, &mut [u8]), step: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (left, width) = match step {
        0 => (53, 18),
        1 => (64, 7),
        _ => (53, 11),
    };
    let spread = if step == 3 { 12 } else { 0 };
    for (x, offset) in [(left, -spread), (SCREEN_W - left - width, spread)] {
        if step == 0 {
            let panel = Rgb565::from(RawU16::new(0x10a3));
            fill(lcd, x, CARRIER_TOP, width, CARRIER_H, panel);
        } else {
            // Read the middle arc and draw it farther out and taller on beat three.
            let base = (BOOT_LOGO_ADDR as i32 - 2 * offset) as u32;
            let (y, h, effect) = if step == 3 {
                (CARRIER_TOP - (OUTER_ARC_H - CARRIER_H) / 2, OUTER_ARC_H, STRETCH_ARC)
            } else {
                (CARRIER_TOP, CARRIER_H, 0)
            };
            paint_image(lcd, read, base, x + offset, y, width, h, effect);
        }
    }
    if step == 0 {
        return;
    }

    let color = if step == 3 { Rgb565::new(31, 48, 9) } else { Rgb565::new(7, 55, 16) };
    for i in (step - 1) * 3..(step * 3).min(8) {
        let x = 42 + i * 10 - i32::from(i >= 4);
        fill(lcd, x, 106, 7, 2, color);
    }
}

/// Whether a home-screen background has been uploaded. Only the AR-5RM has a
/// private region for it; elsewhere that address may hold anything.
pub(super) fn background_present(read: &mut dyn FnMut(u32, &mut [u8])) -> bool {
    #[cfg(feature = "ar5rm-nrf")]
    {
        let mut marker = [0u8; 4];
        read(BACKGROUND_MARKER_ADDR, &mut marker);
        marker == *b"BG01"
    }
    #[cfg(not(feature = "ar5rm-nrf"))]
    {
        let _ = read;
        false
    }
}

/// Repaints a rectangle of the home screen: the background at half
/// brightness (`card` false) or a quarter (`card` true, under text), or the
/// plain `fallback` colour when there is no background.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_background<D>(
    lcd: &mut D,
    read: &mut dyn FnMut(u32, &mut [u8]),
    present: bool,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    card: bool,
    fallback: Rgb565,
) where
    D: DrawTarget<Color = Rgb565>,
{
    if w <= 0 || h <= 0 {
        return;
    }
    if present {
        let shift = if super::theme::sunlight() { 3 + card as u8 } else { 1 + card as u8 };
        paint_image(lcd, read, BACKGROUND_ADDR, x, y, w, h, shift);
    } else {
        fill(lcd, x, y, w, h, fallback);
    }
}
