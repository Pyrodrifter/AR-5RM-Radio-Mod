use crate::flash_map::addr::BOOT_LOGO_ADDR;
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use super::SCREEN_W;

const LINE_BYTES: usize = SCREEN_W as usize * 2;

/// Half brightness, so white menu text stays readable over the brightest red.
/// An erased logo reads back white, which would hide that text: treat it as black.
fn dim(raw: u16) -> u16 {
    if raw == 0xFFFF {
        0
    } else {
        (raw >> 1) & 0x7BEF
    }
}

/// Repaint part of a menu with the skull, one line at a time: the whole logo is
/// more than twice the radio's RAM.
pub(crate) fn draw_backdrop<D>(
    lcd: &mut D,
    read: &mut dyn FnMut(u32, &mut [u8]),
    y: i32,
    h: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line = [0u8; LINE_BYTES];
    for row in y..y + h {
        read(BOOT_LOGO_ADDR + row as u32 * LINE_BYTES as u32, &mut line);
        let area = Rectangle::new(Point::new(0, row), Size::new(SCREEN_W as u32, 1));
        let pixels = line.chunks_exact(2).map(|b| {
            Rgb565::from(RawU16::new(dim(u16::from_le_bytes([b[0], b[1]]))))
        });
        lcd.fill_contiguous(&area, pixels).ok();
    }
}
