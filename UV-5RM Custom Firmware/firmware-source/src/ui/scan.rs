use super::standby::draw_big_freq;
use super::{theme, TextBuf, SCREEN_H, SCREEN_W};
use crate::app;
use core::fmt::Write as _;
use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::Text;

const HEADER_Y: i32 = 4;
const HEADER_H: i32 = 26;
const FREQ_Y: i32 = 40;
const FREQ_H: i32 = 32;

fn clear_scan<D>(lcd: &mut D, y: i32, height: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    lcd.fill_solid(
        &Rectangle::new(Point::new(0, y), Size::new(SCREEN_W as u32, height as u32)),
        theme::t().bg,
    )
    .ok();
}

#[derive(Clone, Copy, PartialEq)]
struct Snapshot {
    dir_up: bool,
    channel_mode: bool,
    channel_num: u16,
    freq_hz: u32,
}

pub(crate) struct Cache {
    snap: Option<Snapshot>,
}

impl Cache {
    pub fn new() -> Self {
        Cache { snap: None }
    }
}

pub fn draw_scan<D>(lcd: &mut D, app: &app::App, cache: &mut Cache)
where
    D: DrawTarget<Color = Rgb565>,
{
    let snap = Snapshot {
        dir_up: app.scan_direction_up(),
        channel_mode: app.watching_is_channel_mode(),
        channel_num: app.watching_channel_num(),
        freq_hz: app.watching_freq_hz(),
    };

    match cache.snap {
        None => full_redraw(lcd, app),
        Some(prev) => {
            if (prev.dir_up, prev.channel_mode, prev.channel_num)
                != (snap.dir_up, snap.channel_mode, snap.channel_num)
            {
                draw_header(lcd, app);
            }
            if prev.freq_hz != snap.freq_hz {
                clear_scan(lcd, FREQ_Y, FREQ_H);
                draw_big_freq(lcd, snap.freq_hz, FREQ_Y);
            }
        }
    }

    cache.snap = Some(snap);
}

fn full_redraw<D>(lcd: &mut D, app: &app::App)
where
    D: DrawTarget<Color = Rgb565>,
{
    clear_scan(lcd, 0, SCREEN_H);

    draw_header(lcd, app);
    draw_big_freq(lcd, app.watching_freq_hz(), FREQ_Y);

    Text::new(
        "UP/DN STEP  #/EXIT STOP",
        Point::new(4, 120),
        MonoTextStyle::new(&FONT_6X10, theme::t().text),
    )
    .draw(lcd)
    .ok();
}

fn draw_header<D>(lcd: &mut D, app: &app::App)
where
    D: DrawTarget<Color = Rgb565>,
{
    clear_scan(lcd, HEADER_Y, HEADER_H);
    let dir = if app.scan_direction_up() { "UP" } else { "DN" };
    let mut header: TextBuf<27> = TextBuf::new();
    header.write_str(app.chan_mode_name()).ok();
    header.write_str(" ").ok();
    header.write_str(dir).ok();
    if app.watching_is_channel_mode() {
        header.write_str(" M").ok();
        super::num(&mut header, app.watching_channel_num() as u32 + 1, 3);
    }
    Text::new(
        header.as_str(),
        Point::new(4, 14),
        MonoTextStyle::new(&FONT_6X10, theme::t().text),
    )
    .draw(lcd)
    .ok();
    if app.watching_is_channel_mode() {
        let hz = app.watching_freq_hz();
        let mut line: TextBuf<26> = TextBuf::new();
        line.write_str(app.side_name_str(app.watching_index())).ok();
        line.write_str("  ").ok();
        super::freq_text(&mut line, hz, 4);
        Text::new(
            line.as_str(),
            Point::new(4, 27),
            MonoTextStyle::new(&FONT_6X10, theme::t().text),
        )
        .draw(lcd)
        .ok();
    }
}
