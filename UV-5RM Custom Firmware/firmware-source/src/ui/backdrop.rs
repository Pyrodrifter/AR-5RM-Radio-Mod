use crate::flash_map::addr::{BACKGROUND_ADDR, BACKGROUND_MARKER_ADDR, BOOT_LOGO_ADDR};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use super::SCREEN_W;

const LINE_BYTES: usize = SCREEN_W as usize * 2;

/// Copies part of a 160x128 RGB565 image from flash to the screen, one line at
/// a time (a whole image is more than twice the radio's RAM). `shift` darkens
/// it: 0 as stored, 1 half (menu text stays readable over the brightest red),
/// 2 a quarter (under home-screen text). An erased image reads back white,
/// which would hide text, so a dimmed white pixel turns black.
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
    shift: u8,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line = [0u8; LINE_BYTES];
    let line = &mut line[..w as usize * 2];
    for row in y..y + h {
        read(base + row as u32 * LINE_BYTES as u32 + x as u32 * 2, line);
        let area = Rectangle::new(Point::new(x, row), Size::new(w as u32, 1));
        let pixels = line.chunks_exact(2).map(|b| {
            let raw = u16::from_le_bytes([b[0], b[1]]);
            Rgb565::from(RawU16::new(match shift {
                0 => raw,
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
        lcd.fill_solid(
            &Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)),
            fallback,
        )
        .ok();
    }
}
