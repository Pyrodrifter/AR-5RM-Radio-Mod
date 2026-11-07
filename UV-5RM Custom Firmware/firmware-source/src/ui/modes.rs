use crate::ui::rounded_font::FONT_9X18;
use super::backdrop::draw_backdrop;
use super::list::{draw_list, Cache, ListSource};
use super::{TextBuf, SCREEN_H};
use crate::app::{self, modes_table};
use core::fmt::Write as _;
use embedded_graphics::mono_font::ascii::{FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;

const FG: Rgb565 = Rgb565::WHITE;
const DIM: Rgb565 = Rgb565::new(13, 26, 13);
const ACTIVE: Rgb565 = Rgb565::new(31, 50, 0);
const ALERT: Rgb565 = Rgb565::new(31, 8, 4);

struct ModeSource<'a, 'b> {
    current: usize,
    app: &'a mut app::App<'b>,
}

impl<'a, 'b> ListSource for ModeSource<'a, 'b> {
    fn row_count(&mut self) -> usize {
        modes_table().len()
    }

    fn read_flash(&mut self, addr: u32, buf: &mut [u8]) {
        self.app.storage_mut().read_raw(addr, buf);
    }

    fn label(&mut self, index: usize, w: &mut dyn core::fmt::Write) {
        let _ = w.write_str(modes_table()[index].name);
    }

    fn value(&mut self, index: usize, w: &mut dyn core::fmt::Write) -> bool {
        if index == self.current {
            let _ = w.write_str("*");
            true
        } else {
            false
        }
    }
}

pub fn draw_menu<D>(lcd: &mut D, app: &mut app::App, cache: &mut Cache)
where
    D: DrawTarget<Color = Rgb565>,
{
    let index = app.mode_menu_index();
    let mut source = ModeSource {
        current: app.chan_mode_index(),
        app,
    };
    draw_list(lcd, "MODE", &mut source, index, false, cache);
}

#[derive(Clone, Copy, PartialEq)]
struct RangeSnapshot {
    stage: u8,
    err: bool,
    lo: u32,
    hi: u32,
    step: u32,
    typed: TextBuf<8>,
    typed_len: usize,
}

pub struct RangeCache {
    snap: Option<RangeSnapshot>,
}

impl RangeCache {
    pub fn new() -> Self {
        RangeCache { snap: None }
    }
}

fn small<D>(lcd: &mut D, s: &str, y: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    Text::new(s, Point::new(4, y), MonoTextStyle::new(&FONT_6X10, color))
        .draw(lcd)
        .ok();
}

fn field<D>(lcd: &mut D, label: &str, value: &str, y: i32, active: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let color = if active { ACTIVE } else { FG };
    small(lcd, label, y, color);
    Text::new(value, Point::new(52, y + 2), MonoTextStyle::new(&FONT_9X18, color))
        .draw(lcd)
        .ok();
}

pub fn draw_range<D>(lcd: &mut D, app: &mut app::App, cache: &mut RangeCache)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut typed: TextBuf<8> = TextBuf::new();
    if app.range_input_len() > 0 {
        app.range_input_write(&mut typed);
    }
    let (lo, hi) = app.range_hz();
    let snap = RangeSnapshot {
        stage: app.range_stage(),
        err: app.range_err(),
        lo,
        hi,
        step: app.range_step_deci_hz(),
        typed,
        typed_len: app.range_input_len(),
    };
    if cache.snap == Some(snap) {
        return;
    }
    cache.snap = Some(snap);

    draw_backdrop(
        lcd,
        &mut |addr, buf| app.storage_mut().read_raw(addr, buf),
        0,
        SCREEN_H,
    );
    small(lcd, "CUSTOM RANGE (MHZ)", 12, FG);

    for (stage, label, hz, y) in [(0u8, "START", lo, 40), (1u8, "STOP", hi, 66)] {
        let mut value: TextBuf<8> = TextBuf::new();
        if snap.stage == stage && snap.typed_len > 0 {
            value.write_str(typed.as_str()).ok();
        } else {
            super::freq_text(&mut value, hz, 3);
        }
        field(lcd, label, value.as_str(), y, snap.stage == stage);
    }

    let mut step: TextBuf<20> = TextBuf::new();
    step.write_str("STEP ").ok();
    super::num(&mut step, snap.step as u32 / 100, 1);
    step.write_str(".").ok();
    super::num(&mut step, snap.step as u32 % 100, 2);
    step.write_str(" KHZ UP/DN").ok();
    small(lcd, step.as_str(), 90, FG);

    if snap.err {
        small(lcd, "BAD RANGE, TRY AGAIN", 106, ALERT);
    }
    small(lcd, "MENU NEXT  EXIT BACK", 120, DIM);
}
