//! The radio's fonts. The 6x10 one is X11's with a rounded zero in place of
//! its diamond, which looks odd at the 3x size of the home-screen frequency.
//! It replaces embedded-graphics' copy everywhere, so only one is in flash.

use embedded_graphics::geometry::Size;
use embedded_graphics::image::ImageRaw;
use embedded_graphics::mono_font::{mapping::ASCII, DecorationDimensions, MonoFont};

pub(crate) use embedded_graphics::mono_font::ascii::FONT_9X18;

pub(crate) const FONT_6X10: MonoFont = MonoFont {
    image: ImageRaw::new(include_bytes!("font_6x10.raw"), 96),
    glyph_mapping: &ASCII,
    character_size: Size::new(6, 10),
    character_spacing: 0,
    baseline: 7,
    underline: DecorationDimensions::new(9, 1),
    strikethrough: DecorationDimensions::new(5, 1),
};
