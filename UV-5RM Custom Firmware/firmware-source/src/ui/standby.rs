//! Home screen. One big card for the side you transmit on (seven-segment
//! frequency, channel box, name, tags) with the other side on one line, or two
//! equal cards with dual watch on. Colours come from `theme`; cards are the
//! uploaded background at a quarter brightness, or plain when there is none.
//! Only what changed is redrawn each tick.

use crate::ui::rounded_font::FONT_9X18;
use super::backdrop;
use super::theme::{self, Theme};
use super::{freq_text, icons, num, TextBuf, SCREEN_H, SCREEN_W};
use crate::app::{self, ChannelDisplayMode};
use crate::device::radio::{Modulation, Power, SubAudio};
use core::fmt::Write as _;
use embedded_graphics::mono_font::{
    ascii::{FONT_5X8, FONT_6X10},
    MonoFont, MonoTextStyle,
};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};

const STATUS_H: i32 = 14;
const CARD_X: i32 = 2;
const CARD_W: i32 = SCREEN_W - 4;
const EDGE_W: i32 = 3;

// Single layout: the master's card, the meter, the other side's line.
const MAIN_TOP: i32 = 16;
const MAIN_H: i32 = 80;
const METER_Y: i32 = 98;
const METER_H: i32 = 14;
const OTHER_Y: i32 = 114;
const OTHER_H: i32 = SCREEN_H - OTHER_Y;

// Dual layout: two equal cards.
const DUAL_H: i32 = 55;
const DUAL_TOP1: i32 = SCREEN_H - DUAL_H;

const BADGE_X: i32 = SCREEN_W - 28;
const BADGE_W: i32 = 24;
const FLAGS_W: i32 = 96;

const SMALL_W: i32 = FONT_6X10.character_size.width as i32;

static mut BG_PRESENT: bool = false;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Snapshot {
    is_transmitting: bool,
    power_save: bool,
    dual_standby: bool,
    vox: bool,
    key_locked: bool,
    master_index: usize,
    dtmf_dial_active: bool,
    dtmf_dial_len: usize,
    channel_input_len: usize,
    freq_input_len: usize,
    tx_prohibited: bool,
    no_channels_notice: bool,
    sides: [SideSnapshot; 2],
}

pub(super) struct Cache {
    structural: Option<Snapshot>,
    battery: Option<(u8, u16)>,
    rx_status: [Option<RxStatusText>; 2],
    tx_elapsed: Option<u32>,
    meter: Option<MeterState>,
}

impl Cache {
    pub fn new() -> Self {
        Cache {
            structural: None,
            battery: None,
            rx_status: [None, None],
            tx_elapsed: None,
            meter: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RxStatusText {
    None,
    Rx,
    Tx,
    LastSignal,
}

#[derive(Clone, Copy, PartialEq)]
struct MeterState {
    tx: bool,
    level: u8,
    live: bool,
    dbm: Option<i32>,
    s_number: u8,
    over_s9: i32,
    side: u8,
}

#[derive(Clone, Copy, PartialEq)]
struct SideSnapshot {
    freq_hz: u32,
    is_channel_mode: bool,
    channel_num: u16,
    modulation: Modulation,
    power: Power,
    subaudio_tx: SubAudio,
    subaudio_rx: SubAudio,
    offset_hz: u32,
    freq_dir: u8,
    wide_band: bool,
    reversed: bool,
}

impl Snapshot {
    fn overlay_active(&self) -> bool {
        self.tx_prohibited || self.no_channels_notice
    }

    fn flags(&self) -> (bool, bool, bool, bool) {
        (self.is_transmitting, self.power_save, self.vox, self.key_locked)
    }

    fn capture(app: &app::App) -> Self {
        let is_transmitting = app.is_transmitting();
        let master_index = app.master_index();
        let side = |i: usize| -> SideSnapshot {
            SideSnapshot {
                freq_hz: shown_freq(app, i),
                is_channel_mode: app.side_is_channel_mode(i),
                channel_num: app.side_channel_num(i),
                modulation: app.side_modulation(i),
                power: app.side_power(i),
                subaudio_tx: app.side_subaudio_tx(i),
                subaudio_rx: app.side_subaudio_rx(i),
                offset_hz: app.side_offset_hz(i),
                freq_dir: app.side_freq_dir(i),
                wide_band: app.side_wide_band(i),
                reversed: app.side_reversed(i),
            }
        };
        Snapshot {
            is_transmitting,
            power_save: app.power_save_active(),
            dual_standby: app.dual_standby_enabled(),
            vox: app.vox_enabled(),
            key_locked: app.is_key_locked(),
            master_index,
            dtmf_dial_active: app.dtmf_dial_active(),
            dtmf_dial_len: app.dtmf_dial_len(),
            channel_input_len: app.channel_input_len(),
            freq_input_len: app.freq_input_len(),
            tx_prohibited: app.tx_prohibited(),
            no_channels_notice: app.no_channels_notice(),
            sides: [side(0), side(1)],
        }
    }
}

/// The frequency a side shows: its TX frequency while transmitting on it.
fn shown_freq(app: &app::App, i: usize) -> u32 {
    if app.is_transmitting() && i == app.master_index() {
        app.side_tx_freq_hz(i)
    } else {
        app.side_freq_hz(i)
    }
}

// ---- drawing primitives ----

fn fill<D>(lcd: &mut D, x: i32, y: i32, w: i32, h: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    if w <= 0 || h <= 0 {
        return;
    }
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(lcd)
        .ok();
}

/// Card (`card` true) or open-screen backing for a rectangle.
#[inline(never)]
fn paint<D>(lcd: &mut D, app: &mut app::App, x: i32, y: i32, w: i32, h: i32, card: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    backdrop::paint_background(
        lcd,
        &mut |a, b| app.storage_mut().read_raw(a, b),
        unsafe { BG_PRESENT },
        x,
        y,
        w,
        h,
        card,
        if card { t.card } else { t.bg },
    );
}

/// Keep small text sharp and give large text extra weight.
#[inline(never)]
fn bold<D>(lcd: &mut D, s: &str, font: &MonoFont, x: i32, y: i32, color: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let style = MonoTextStyle::new(font, color);
    if font.character_size.width >= 9 {
        Text::with_baseline(s, Point::new(x + 1, y), style, Baseline::Top)
            .draw(lcd)
            .ok();
    }
    Text::with_baseline(s, Point::new(x, y), style, Baseline::Top)
        .draw(lcd)
        .ok();
    x + s.len() as i32 * font.character_size.width as i32
}

fn bold_right<D>(lcd: &mut D, s: &str, font: &MonoFont, right: i32, y: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    bold(lcd, s, font, right - s.len() as i32 * font.character_size.width as i32, y, color);
}

/// A filled box with clipped corners and `s` inside; returns the x after it.
#[inline(never)]
fn pill<D>(lcd: &mut D, s: &str, font: &MonoFont, x: i32, y: i32, h: i32, bg: Rgb565, fg: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = s.len() as i32 * font.character_size.width as i32 + 7;
    fill(lcd, x + 1, y, w - 2, h, bg);
    fill(lcd, x, y + 1, w, h - 2, bg);
    bold(lcd, s, font, x + 3, y + (h - font.character_size.height as i32) / 2 + 1, fg);
    x + w + 3
}


/// Draws the radio's existing font at twice its width and one-and-a-half
/// times its height. This retains the letter shapes while using the space
/// available in the frequency row.
struct ScaledFrequency<'a, D> {
    lcd: &'a mut D,
    x: i32,
    y: i32,
}

impl<D: DrawTarget<Color = Rgb565>> OriginDimensions for ScaledFrequency<'_, D> {
    fn size(&self) -> Size {
        Size::new(80, 21)
    }
}

impl<D: DrawTarget<Color = Rgb565>> DrawTarget for ScaledFrequency<'_, D> {
    type Color = Rgb565;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(point, color) in pixels {
            let y = point.y * 3 / 2;
            let next_y = (point.y + 1) * 3 / 2;
            self.lcd.fill_solid(
                &Rectangle::new(
                    Point::new(self.x + point.x * 2, self.y + y),
                    Size::new(3, (next_y - y) as u32),
                ),
                color,
            )?;
        }
        Ok(())
    }
}

#[inline(never)]
fn draw_scaled_text<D: DrawTarget<Color = Rgb565>>(lcd: &mut D, text: &str, x: i32, y: i32, color: Rgb565) {
    let mut scaled = ScaledFrequency { lcd, x, y };
    Text::with_baseline(text, Point::zero(), MonoTextStyle::new(&FONT_9X18, color), Baseline::Top)
        .draw(&mut scaled).ok();
}


/// Draw the six main frequency digits and the two smaller precision digits.
fn seg_freq<D>(
    lcd: &mut D,
    t: &Theme,
    hz: u32,
    y: i32,
    _big: bool,
    color: Rgb565,
    typed: Option<&app::App>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let mhz = hz / 1_000_000;
    let khz = hz / 1000 % 1000;
    let digits = [mhz / 100, mhz / 10 % 10, mhz % 10, khz / 100, khz / 10 % 10, khz % 10];
    let mut shown = [b' '; 7];
    shown[3] = b'.';
    for (pos, &digit) in digits.iter().enumerate() {
        let at = pos + usize::from(pos >= 3);
        shown[at] = match typed {
            Some(app) if pos < app.freq_input_len() => b'0' + app.freq_input_digit(pos),
            Some(_) => b'-',
            None if pos == 0 && digit == 0 => b' ',
            None => b'0' + digit as u8,
        };
    }
    let text = core::str::from_utf8(&shown).unwrap_or("");
    let x = CARD_X + CARD_W - 3 - 7 * 18 - 2 * SMALL_W;
    draw_scaled_text(lcd, text, x, y, if typed.is_some() { t.warn } else { color });
    if typed.is_none() {
        let mut tail: TextBuf<4> = TextBuf::new();
        num(&mut tail, hz / 10 % 100, 2);
        bold(lcd, tail.as_str(), &FONT_6X10, x + 7 * 18, y + 17, t.text);
    }
}

// ---- layout ----

/// Top and height of side `i`'s card.
fn card_area(snap: &Snapshot, i: usize) -> (i32, i32) {
    if snap.dual_standby {
        (if i == 0 { MAIN_TOP } else { DUAL_TOP1 }, DUAL_H - 2)
    } else {
        (MAIN_TOP, MAIN_H)
    }
}

/// Whether side `i` gets a card (always with dual watch; else only the master).
fn has_card(snap: &Snapshot, i: usize) -> bool {
    snap.dual_standby || i == snap.master_index
}

fn full_redraw<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot)
where
    D: DrawTarget<Color = Rgb565>,
{
    unsafe {
        BG_PRESENT = backdrop::background_present(&mut |a, b| app.storage_mut().read_raw(a, b));
    }
    paint(lcd, app, 0, 0, SCREEN_W, SCREEN_H, false);
    paint(lcd, app, 0, 0, SCREEN_W, STATUS_H, true);
    draw_flags(lcd, app);

    for i in 0..2 {
        if has_card(snap, i) {
            let (top, h) = card_area(snap, i);
            paint(lcd, app, CARD_X, top, CARD_W, h, true);
            draw_edge(lcd, app, snap, i);
            draw_row1(lcd, app, snap, i);
            draw_row2(lcd, app, snap, i);
            if !snap.dual_standby {
                draw_row3(lcd, app, i);
            }
        } else {
            draw_other(lcd, app, i);
        }
    }

    if app.tx_prohibited() {
        draw_overlay(lcd, "TX PROHIBITED", theme::t().alert);
    } else if app.no_channels_notice() {
        draw_overlay(lcd, "NO CHANNELS", theme::t().warn);
    }
}

pub fn draw_standby<D>(lcd: &mut D, app: &mut app::App, cache: &mut Cache)
where
    D: DrawTarget<Color = Rgb565>,
{
    let snap = Snapshot::capture(app);

    let overlay_active = snap.overlay_active();
    let full = match cache.structural.as_ref() {
        None => true,
        Some(p) => {
            p.overlay_active() != overlay_active
                || (overlay_active
                    && (p.tx_prohibited, p.no_channels_notice)
                        != (snap.tx_prohibited, snap.no_channels_notice))
                || p.dual_standby != snap.dual_standby
                || p.master_index != snap.master_index
        }
    };

    if full {
        full_redraw(lcd, app, &snap);
        cache.tx_elapsed = None;
    } else if !overlay_active {
        let prev = cache.structural.unwrap();
        let tx_changed = prev.is_transmitting != snap.is_transmitting;

        if prev.flags() != snap.flags() {
            draw_flags(lcd, app);
        }
        if snap.is_transmitting {
            let secs = app.tx_elapsed_seconds();
            if cache.tx_elapsed != Some(secs) {
                cache.tx_elapsed = Some(secs);
                draw_flags(lcd, app);
            }
        } else {
            cache.tx_elapsed = None;
        }

        for i in 0..2usize {
            let p = &prev.sides[i];
            let s = &snap.sides[i];
            let is_master = i == snap.master_index;
            let channel_changed = (p.is_channel_mode, p.channel_num) != (s.is_channel_mode, s.channel_num);
            let dtmf_changed = prev.dtmf_dial_active != snap.dtmf_dial_active
                || (snap.dtmf_dial_active && prev.dtmf_dial_len != snap.dtmf_dial_len);

            if !has_card(&snap, i) {
                if channel_changed || p.freq_hz != s.freq_hz || dtmf_changed {
                    draw_other(lcd, app, i);
                }
                continue;
            }
            if is_master && tx_changed {
                draw_edge(lcd, app, &snap, i);
            }
            let input_changed = if is_master {
                (prev.channel_input_len, prev.freq_input_len) != (snap.channel_input_len, snap.freq_input_len)
            } else {
                dtmf_changed
            };
            if input_changed || channel_changed || p.freq_hz != s.freq_hz {
                draw_row1(lcd, app, &snap, i);
                draw_row2(lcd, app, &snap, i);
            }
            if !snap.dual_standby
                && (p.modulation, p.power, p.subaudio_tx, p.subaudio_rx, p.offset_hz, p.freq_dir, p.wide_band, p.reversed)
                    != (s.modulation, s.power, s.subaudio_tx, s.subaudio_rx, s.offset_hz, s.freq_dir, s.wide_band, s.reversed)
            {
                draw_row3(lcd, app, i);
            }
        }
    }
    // else: an unchanged overlay, already painted.

    cache.structural = Some(snap);

    draw_battery(lcd, app, &mut cache.battery, full);
    for i in 0..2usize {
        if has_card(&snap, i) {
            draw_rx_status(lcd, app, &snap, i, &mut cache.rx_status[i], full);
        }
    }
    if !overlay_active {
        draw_meter(lcd, app, &snap, &mut cache.meter, full);
    }
}

fn draw_overlay<D>(lcd: &mut D, label: &str, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = label.len() as i32 * SMALL_W + 16;
    let x = (SCREEN_W - w) / 2;
    let y = SCREEN_H / 2 - 12;
    fill(lcd, x, y, w, 24, theme::t().card);
    icons::draw_outline(lcd, Point::new(x, y), Size::new(w as u32, 24), color);
    bold(lcd, label, &FONT_6X10, x + 8, y + 7, color);
}

// ---- status bar ----

fn draw_flags<D>(lcd: &mut D, app: &mut app::App)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    paint(lcd, app, 0, 0, FLAGS_W, STATUS_H, true);
    let mut x = pill(lcd, app.chan_mode_short(), &FONT_6X10, 2, 1, 12, t.accent, t.on_accent);

    if app.is_transmitting() {
        let secs = app.tx_elapsed_seconds();
        let mut buf: TextBuf<8> = TextBuf::new();
        num(&mut buf, secs / 60, 1);
        buf.write_str(":").ok();
        num(&mut buf, secs % 60, 2);
        x = bold(lcd, buf.as_str(), &FONT_6X10, x, 2, t.alert) + 4;
    } else {
        for (on, label, color) in [(app.power_save_active(), "PS", t.dim), (app.dual_standby_enabled(), "DW", t.accent)] {
            if on {
                x = pill(lcd, label, &FONT_5X8, x, 2, 10, color, t.card);
            }
        }
    }
    if app.vox_enabled() {
        x = pill(lcd, "VOX", &FONT_5X8, x, 2, 10, t.ok, t.card);
    }
    if app.is_key_locked() {
        icons::draw_padlock(lcd, x, 2, t.warn);
    }
}

fn draw_battery<D>(lcd: &mut D, app: &mut app::App, last: &mut Option<(u8, u16)>, force: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let state = (app.battery_bars(), app.battery_voltage_cv());
    if !force && *last == Some(state) {
        return;
    }
    *last = Some(state);
    let (bars, cv) = state;
    let t = theme::t();
    paint(lcd, app, FLAGS_W, 0, SCREEN_W - FLAGS_W, STATUS_H, true);
    let icon_x = SCREEN_W - 3 - icons::BATTERY_W;
    let mut volts: TextBuf<8> = TextBuf::new();
    num(&mut volts, cv as u32 / 100, 1);
    volts.write_str(".").ok();
    num(&mut volts, cv as u32 / 10 % 10, 1);
    volts.write_str("V").ok();
    bold_right(lcd, volts.as_str(), &FONT_6X10, icon_x - 3, 2, t.text);
    let charge = match bars {
        0 | 1 => t.alert,
        2 => t.warn,
        _ => t.ok,
    };
    icons::draw_battery(lcd, icon_x, 2, bars, t.dim, charge);
}

// ---- cards ----

fn draw_edge<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (top, h) = card_area(snap, i);
    let t = theme::t();
    if i != snap.master_index {
        paint(lcd, app, CARD_X, top, EDGE_W, h, true);
    } else {
        fill(lcd, CARD_X, top, EDGE_W, h, if snap.is_transmitting { t.alert } else { t.accent });
    }
}

/// Channel box, then the name (or the frequency in Name display mode).
fn draw_row1<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    let (top, _) = card_area(snap, i);
    let big = !snap.dual_standby;
    let is_master = i == snap.master_index;
    let (y, h, font) = if big { (top + 3, 20, &FONT_9X18) } else { (top + 2, 13, &FONT_6X10) };
    let mut x = CARD_X + EDGE_W + 3;
    paint(lcd, app, x, y, BADGE_X - x, h, true);

    if !big {
        x = bold(lcd, if i == 0 { "A" } else { "B" }, &FONT_6X10, x, y + 2, if is_master { t.accent } else { t.dim }) + 2;
    }

    // The channel box: memory number, input in progress, or VFO.
    let mut label: TextBuf<4> = TextBuf::new();
    let mut label_color = if is_master { t.accent } else { t.dim };
    if is_master && app.channel_input_len() > 0 {
        let len = app.channel_input_len();
        for pos in 0..3usize {
            if pos + len < 3 {
                label.write_str("-").ok();
            } else {
                num(&mut label, app.freq_input_digit(pos + len - 3) as u32, 1);
            }
        }
        label_color = t.warn;
    } else if app.side_is_channel_mode(i) {
        num(&mut label, app.side_channel_num(i) as u32 + 1, 3);
    } else {
        label.write_str("VFO").ok();
    }
    x = bold(lcd, label.as_str(), &FONT_6X10, x, y + if big { 5 } else { 2 }, label_color) + 8;

    if !is_master && app.dtmf_dial_active() {
        return;
    }
    let name = app.side_name_str(i);
    if app.channel_display_mode() == ChannelDisplayMode::Name && app.side_is_channel_mode(i) && !name.is_empty() {
        let mut freq: TextBuf<12> = TextBuf::new();
        freq_text(&mut freq, shown_freq(app, i), 5);
        bold(lcd, freq.as_str(), &FONT_6X10, x, y + if big { 5 } else { 2 }, t.text);
    } else if app.channel_display_mode() != ChannelDisplayMode::Name && app.side_is_channel_mode(i) && !name.is_empty() {
        let room = ((BADGE_X - x - 2) / font.character_size.width as i32).max(0) as usize;
        let shown = &name[..name.len().min(room)];
        bold(lcd, shown, font, x, y + if big { 1 } else { 2 }, t.text);
    }
}

/// The frequency in segments, DTMF dialling, or a channel name in Name mode.
fn draw_row2<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    let (top, _) = card_area(snap, i);
    let big = !snap.dual_standby;
    let (y, h) = if big { (top + 26, 32) } else { (top + 18, 28) };
    paint(lcd, app, CARD_X + EDGE_W, y, CARD_W - EDGE_W, h, true);
    let is_master = i == snap.master_index;
    let color = if is_master || snap.dual_standby { t.text } else { t.dim };

    if is_master && app.freq_input_len() > 0 {
        seg_freq(lcd, t, 0, y + 1, big, color, Some(app));
        return;
    }
    if !is_master && app.dtmf_dial_active() {
        draw_dtmf(lcd, app, CARD_X + EDGE_W + 4, y + 8);
        return;
    }
    let name = app.side_name_str(i);
    if app.channel_display_mode() == ChannelDisplayMode::Name && app.side_is_channel_mode(i) && !name.is_empty() {
        if name.len() <= 8 {
            draw_scaled_text(lcd, name, CARD_X + EDGE_W + 4, y + 1, color);
        } else {
            let shown = &name[..name.len().min(16)];
            bold(lcd, shown, &FONT_9X18, CARD_X + EDGE_W + 4, y + 6, color);
        }
        return;
    }
    seg_freq(lcd, t, shown_freq(app, i), y + 1, big, color, None);
}

/// Tags: mode, power, tone, shift, bandwidth, reverse.
fn draw_row3<D>(lcd: &mut D, app: &mut app::App, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    let y = MAIN_TOP + 62;
    let mut x = CARD_X + EDGE_W + 4;
    paint(lcd, app, x, y, CARD_W - EDGE_W - 4, 14, true);

    let modulation = match app.side_modulation(i) {
        Modulation::Fm => "FM",
        Modulation::Am => "AM",
        Modulation::Usb => "USB",
        Modulation::Cw => "CW",
        Modulation::Cwf => "CWF",
    };
    let power = match app.side_power(i) {
        Power::Low => "LOW",
        Power::Mid => "MID",
        Power::High => "HIGH",
    };
    let mut shift: TextBuf<8> = TextBuf::new();
    let offset = app.side_offset_hz(i);
    if offset != 0 {
        let sign = match app.side_freq_dir(i) {
            1 => "+",
            2 => "-",
            _ => "",
        };
        if !sign.is_empty() {
            shift.write_str(sign).ok();
            num(&mut shift, offset / 1000, 1);
            shift.write_str("K").ok();
        }
    }
    let tags = [
        (modulation, t.text),
        (power, t.text),
        (tone_mode_label(app.side_subaudio_tx(i), app.side_subaudio_rx(i)), t.accent),
        (shift.as_str(), t.accent),
        (if app.side_wide_band(i) { "" } else { "NAR" }, t.text),
        (if app.side_reversed(i) { "R" } else { "" }, t.alert),
    ];
    for (label, color) in tags {
        if label.is_empty() || x + label.len() as i32 * SMALL_W + 7 > CARD_X + CARD_W {
            continue;
        }
        x = bold(lcd, label, &FONT_6X10, x, y + 2, color) + 6;
    }
}

/// Single layout: the side you're not on, as one line under the meter.
fn draw_other<D>(lcd: &mut D, app: &mut app::App, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    paint(lcd, app, CARD_X, OTHER_Y, CARD_W, OTHER_H, true);
    let y = OTHER_Y + 2;
    let mut x = bold(lcd, if i == 0 { "A" } else { "B" }, &FONT_6X10, CARD_X + 4, y, t.dim) + 5;
    if app.dtmf_dial_active() {
        draw_dtmf(lcd, app, x, y);
        return;
    }
    let mut label: TextBuf<6> = TextBuf::new();
    if app.side_is_channel_mode(i) {
        label.write_str("M").ok();
        num(&mut label, app.side_channel_num(i) as u32 + 1, 3);
    } else {
        label.write_str("VFO").ok();
    }
    x = bold(lcd, label.as_str(), &FONT_6X10, x, y, t.dim) + 5;
    let name = app.side_name_str(i);
    if app.side_is_channel_mode(i) && !name.is_empty() {
        bold(lcd, &name[..name.len().min(7)], &FONT_6X10, x, y, t.dim);
    }
    let hz = app.side_freq_hz(i);
    let mut f: TextBuf<12> = TextBuf::new();
    freq_text(&mut f, hz, 5);
    bold_right(lcd, f.as_str(), &FONT_6X10, CARD_X + CARD_W - 3, y, t.dim);
}

fn draw_dtmf<D>(lcd: &mut D, app: &app::App, x: i32, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut buf: TextBuf<17> = TextBuf::new();
    buf.write_str(">").ok();
    for pos in 0..app.dtmf_dial_capacity() {
        if pos < app.dtmf_dial_len() {
            buf.write_char(dtmf_char(app.dtmf_dial_digit(pos))).ok();
        } else {
            buf.write_str("-").ok();
        }
    }
    bold(lcd, buf.as_str(), &FONT_6X10, x, y, theme::t().warn);
}

fn draw_rx_status<D>(
    lcd: &mut D,
    app: &mut app::App,
    snap: &Snapshot,
    i: usize,
    last: &mut Option<RxStatusText>,
    force: bool,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let is_master = i == app.master_index();
    let kind = if app.is_transmitting() && is_master {
        RxStatusText::Tx
    } else if i == app.watching_index() && (app.is_monitor() || app.rssi_open()) {
        if app.rx_blink_on() {
            RxStatusText::Rx
        } else {
            RxStatusText::None
        }
    } else if app.last_signal_side() == Some(i) {
        RxStatusText::LastSignal
    } else {
        RxStatusText::None
    };
    if !force && *last == Some(kind) {
        return;
    }
    *last = Some(kind);

    let t = theme::t();
    let (top, _) = card_area(snap, i);
    let (y, h) = if snap.dual_standby { (top + 2, 13) } else { (top + 6, 14) };
    paint(lcd, app, BADGE_X, y, BADGE_W, h, true);
    let (label, bg, fg) = match kind {
        RxStatusText::Tx => ("TX", t.alert, t.text),
        RxStatusText::Rx => ("RX", t.ok, t.card),
        RxStatusText::LastSignal => (">>", t.card_hi, t.warn),
        RxStatusText::None => return,
    };
    pill(lcd, label, &FONT_6X10, BADGE_X + 2, y, h, bg, fg);
}

fn tone_mode_label(tx: SubAudio, rx: SubAudio) -> &'static str {
    match (tx, rx) {
        (SubAudio::None, SubAudio::None) => "",
        (SubAudio::Ctcss(_), SubAudio::None) => "TONE",
        (SubAudio::Ctcss(t), SubAudio::Ctcss(r)) if t == r => "TSQL",
        (SubAudio::Dcs { code: tc, inverted: ti }, SubAudio::Dcs { code: rc, inverted: ri })
            if tc == rc && ti == ri =>
        {
            "DCS"
        }
        (SubAudio::None, SubAudio::Dcs { .. }) => "DCS-R",
        (SubAudio::None, SubAudio::Ctcss(_)) => "TSQL-R",
        _ => "CROSS",
    }
}

fn dtmf_char(v: u8) -> char {
    match v {
        0..=9 => (b'0' + v) as char,
        10..=13 => (b'A' + (v - 10)) as char,
        14 => '*',
        15 => '#',
        _ => '?',
    }
}

/// The frequency in big segments across the screen, for the scan screen.
pub(super) fn draw_big_freq<D>(lcd: &mut D, hz: u32, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    seg_freq(lcd, t, hz, y, true, t.text, None);
}

// ---- meter ----

const S_SEGS: u8 = 14;

fn meter_state(app: &app::App) -> MeterState {
    if app.is_transmitting() {
        return MeterState {
            tx: true,
            level: ((app.mic_level() as u16 * S_SEGS as u16 + 127) / 255).min(S_SEGS as u16) as u8,
            live: true,
            dbm: None,
            s_number: 0,
            over_s9: 0,
            side: app.master_index() as u8,
        };
    }
    if app.audio_open() {
        MeterState {
            tx: false,
            level: app.s_meter_level(),
            live: true,
            dbm: Some(app.rssi_dbm()),
            s_number: app.s_meter_s_number(),
            over_s9: app.s_meter_over_s9_dbm(),
            side: app.watching_index() as u8,
        }
    } else {
        match app.last_signal_peak() {
            Some((side, dbm)) => MeterState {
                tx: false,
                level: app::s_meter_level_of(dbm),
                live: false,
                dbm: Some(dbm),
                s_number: 0,
                over_s9: 0,
                side: side as u8,
            },
            None => MeterState {
                tx: false,
                level: 0,
                live: false,
                dbm: None,
                s_number: 0,
                over_s9: 0,
                side: app.master_index() as u8,
            },
        }
    }
}

fn draw_meter<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot, last: &mut Option<MeterState>, force: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let state = meter_state(app);
    if !force && *last == Some(state) {
        return;
    }
    *last = Some(state);
    let t = theme::t();
    let seg_color = |i: u8| {
        if state.tx || i >= 11 {
            t.alert
        } else if i >= 8 {
            t.warn
        } else {
            t.ok
        }
    };

    if snap.dual_standby {
        // A thin bar along the bottom of each card, lit on the active side.
        for side in 0..2usize {
            let (top, h) = card_area(snap, side);
            let lit = if side == state.side as usize && (state.live || state.tx) { state.level } else { 0 };
            for i in 0..S_SEGS {
                fill(lcd, CARD_X + 6 + i as i32 * 10, top + h - 5, 8, 3, if i < lit { seg_color(i) } else { t.card_hi });
            }
        }
        return;
    }

    paint(lcd, app, CARD_X, METER_Y, CARD_W, METER_H, true);
    let mut x = CARD_X + 4;
    if state.tx {
        x = bold(lcd, "MIC", &FONT_6X10, x, METER_Y + 2, t.alert) + 4;
    }
    let lit = if state.live { state.level } else { 0 };
    for i in 0..S_SEGS {
        let h = 4 + i as i32 * 6 / 13;
        let color = if i < lit || (!state.live && i + 1 == state.level) { seg_color(i) } else { t.card_hi };
        fill(lcd, x + i as i32 * 7, METER_Y + 12 - h, 5, h, color);
    }
    let mut line: TextBuf<12> = TextBuf::new();
    match state.dbm {
        Some(_) if state.live && state.over_s9 > 0 => {
            line.write_str("S9+").ok();
            num(&mut line, state.over_s9 as u32, 1);
        }
        Some(_) if state.live => {
            line.write_str("S").ok();
            num(&mut line, state.s_number as u32, 1);
        }
        Some(dbm) => {
            line.write_str("-").ok();
            num(&mut line, dbm.unsigned_abs(), 1);
            line.write_str("dBm").ok();
        }
        None => {}
    }
    bold_right(lcd, line.as_str(), &FONT_6X10, CARD_X + CARD_W - 3, METER_Y + 2, if state.live { t.text } else { t.dim });
}
