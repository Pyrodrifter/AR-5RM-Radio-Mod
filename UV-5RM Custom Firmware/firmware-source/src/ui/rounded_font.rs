use embedded_graphics::image::ImageRaw;
use embedded_graphics::mono_font::{ascii, MonoFont};

// Nunito ExtraBold rasterized into the original 9x18 ASCII font's cells.
// Its original metrics keep text aligned in both home and overlay layouts.
// Font attribution and redistribution terms are in Nunito-OFL.txt.
pub(crate) const FONT_9X18: MonoFont = MonoFont {
    image: ImageRaw::new(include_bytes!("rounded_9x18.raw"), 144),
    ..ascii::FONT_9X18
};
