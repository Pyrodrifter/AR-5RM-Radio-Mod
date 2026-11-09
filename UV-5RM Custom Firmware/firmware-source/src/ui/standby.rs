//! Home screen with a status bar, a framed readout for the active side
//! and a compact standby panel. With dual watch on, both sides get the
//! large readout.
//! Labels are `accent`, readouts `value`. Only what changed is redrawn.

use super::backdrop;
use super::list::fill;
use super::theme::{self, Theme};
use super::{freq_text, icons, num, TextBuf, SCREEN_H, SCREEN_W};
use crate::app::{self, ChannelDisplayMode};
use crate::device::radio::{Modulation, Power, SubAudio};
use core::convert::Infallible;
use core::fmt::Write as _;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use crate::ui::fonts::FONT_6X10;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};

const PANEL_X: i32 = 2;
const PANEL_W: i32 = SCREEN_W - 2 * PANEL_X;
const INNER_X: i32 = PANEL_X + 1;
const INNER_W: i32 = PANEL_W - 2;
const LEFT: i32 = 6;
const RIGHT: i32 = SCREEN_W - 4;
const STATUS_H: i32 = 14;
const BATTERY_DIVIDER: i32 = 105;
const HEADER_TEXT_END: i32 = RIGHT - 22;
const READ_H: i32 = 30;
const SMALL_W: i32 = 6;

// Single layout.
const HEAD_Y: i32 = 20;
const READ_Y: i32 = 38;
const DETAIL_Y: i32 = 76;
const METER_Y: i32 = 92;
const STBY_Y: i32 = 113;

// Dual layout: two blocks of this height, each a header line, the readout
// and a thin bar.
const BLOCK_H: i32 = 56;

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
    peak_dbm: Option<i32>,
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
            is_transmitting: app.is_transmitting(),
            power_save: app.power_save_active(),
            dual_standby: app.dual_standby_enabled(),
            vox: app.vox_enabled(),
            key_locked: app.is_key_locked(),
            master_index: app.master_index(),
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

/// In channel (MR) mode a named channel shows its name large and its
/// frequency small, unless the display mode asks for the frequency.
fn name_big(app: &app::App, i: usize) -> bool {
    app.side_is_channel_mode(i)
        && !app.side_name_str(i).is_empty()
        && app.channel_display_mode() != ChannelDisplayMode::Frequency
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

/// Clears a rectangle to the background (or the uploaded picture).
#[inline(never)]
fn paint_backdrop<D>(lcd: &mut D, app: &mut app::App, y: i32, h: i32, readout: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    backdrop::paint_background(
        lcd,
        &mut |a, b| app.storage_mut().read_raw(a, b),
        unsafe { BG_PRESENT },
        if readout { INNER_X } else { 0 },
        y,
        if readout { INNER_W } else { SCREEN_W },
        h,
        readout,
        theme::t().bg,
    );
}

#[inline(never)]
fn panel<D>(lcd: &mut D, y: i32, h: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    fill(lcd, PANEL_X, y, PANEL_W, h, t.card);
    icons::draw_outline(lcd, Point::new(PANEL_X, y), Size::new(PANEL_W as u32, h as u32), t.card_hi);
}

/// Text with its top at `y`; returns the x after it.
#[inline(never)]
fn text<D>(lcd: &mut D, s: &str, font: &MonoFont, x: i32, y: i32, color: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    Text::with_baseline(s, Point::new(x, y), MonoTextStyle::new(font, color), Baseline::Top)
        .draw(lcd)
        .ok();
    x + s.len() as i32 * font.character_size.width as i32
}

fn text_right<D>(lcd: &mut D, s: &str, font: &MonoFont, right: i32, y: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    text(lcd, s, font, right - s.len() as i32 * font.character_size.width as i32, y, color);
}

/// Draws the channel name three times taller, with a width that fits the readout.
struct Scaled<'a, D> {
    lcd: &'a mut D,
    x: i32,
    y: i32,
    scale_x: i32,
}

impl<D> OriginDimensions for Scaled<'_, D> {
    fn size(&self) -> Size {
        Size::new(SCREEN_W as u32, SCREEN_H as u32)
    }
}

impl<D> DrawTarget for Scaled<'_, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(p, c) in pixels {
            fill(self.lcd, self.x + p.x * self.scale_x, self.y + p.y * 3, self.scale_x, 3, c);
        }
        Ok(())
    }
}

/// Large digits: the FM app's 5x7 dot font drawn in 4-pixel blocks, with a
/// dash (index 10) for digits still to be typed.
const DIGITS: [[u8; 7]; 11] = [
    [14, 17, 17, 17, 17, 17, 14],
    [4, 12, 4, 4, 4, 4, 14],
    [14, 17, 1, 2, 4, 8, 31],
    [30, 1, 1, 14, 1, 1, 30],
    [2, 6, 10, 18, 31, 2, 2],
    [31, 16, 16, 30, 1, 1, 30],
    [14, 16, 16, 30, 17, 17, 14],
    [31, 1, 2, 4, 8, 8, 8],
    [14, 17, 17, 14, 17, 17, 14],
    [14, 17, 17, 15, 1, 1, 14],
    [0, 0, 0, 31, 0, 0, 0],
];
const DIGIT_PITCH: i32 = 22;

/// `MMM.kkk` in the large digits with `kk` small after it, right-aligned to
/// `right`. With `typed`, shows the frequency being keyed in (dashes after).
fn big_freq<D>(lcd: &mut D, t: &Theme, hz: u32, right: i32, y: i32, color: Rgb565, typed: Option<&app::App>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mhz = hz / 1_000_000;
    let khz = hz / 1000 % 1000;
    let digits = [mhz / 100, mhz / 10 % 10, mhz % 10, khz / 100, khz / 10 % 10, khz % 10];
    let color = if typed.is_some() { t.warn } else { color };
    // Six digits, the point and the small tail fill 151 pixels.
    let mut x = right - 151;
    for (pos, &d) in digits.iter().enumerate() {
        let glyph = match typed {
            Some(app) if pos < app.freq_input_len() => app.freq_input_digit(pos) as usize,
            Some(_) => 10,
            None if pos == 0 && d == 0 => 11,
            None => d as usize,
        };
        if let Some(rows) = DIGITS.get(glyph) {
            for (r, &bits) in rows.iter().enumerate() {
                for col in 0..5 {
                    if bits & (16 >> col) != 0 {
                        fill(lcd, x + col * 4, y + 1 + r as i32 * 4, 4, 4, color);
                    }
                }
            }
        }
        x += DIGIT_PITCH;
        if pos == 2 {
            fill(lcd, x + 1, y + 25, 4, 4, color);
            x += 8;
        }
    }
    if typed.is_none() {
        let mut tail: TextBuf<4> = TextBuf::new();
        num(&mut tail, hz / 10 % 100, 2);
        text(lcd, tail.as_str(), &FONT_6X10, x, y + 21, t.dim);
    }
}

// ---- layout ----

/// Top of side `i`'s large readout.
fn box_top(snap: &Snapshot, i: usize) -> i32 {
    if snap.dual_standby {
        header_y(snap, i) + 13
    } else {
        READ_Y
    }
}

/// Whether side `i` gets a large readout: both with dual watch, else only the master.
fn has_box(snap: &Snapshot, i: usize) -> bool {
    snap.dual_standby || i == snap.master_index
}

fn header_y(snap: &Snapshot, i: usize) -> i32 {
    if snap.dual_standby {
        STATUS_H + 5 + i as i32 * BLOCK_H
    } else {
        HEAD_Y
    }
}

fn full_redraw<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot)
where
    D: DrawTarget<Color = Rgb565>,
{
    unsafe {
        BG_PRESENT = backdrop::background_present(&mut |a, b| app.storage_mut().read_raw(a, b));
    }
    let t = theme::t();
    paint_backdrop(lcd, app, 0, SCREEN_H, false);
    panel(lcd, 0, STATUS_H);
    fill(lcd, BATTERY_DIVIDER, 1, 1, STATUS_H - 2, t.card_hi);
    draw_flags(lcd, app);

    for i in 0..2 {
        if has_box(snap, i) {
            let y = header_y(snap, i) - 3;
            panel(lcd, y, if snap.dual_standby { BLOCK_H - 2 } else { 90 });
            if i == snap.master_index {
                fill(lcd, PANEL_X, y + 1, 2, 13, t.accent);
            }
            draw_header(lcd, app, snap, i);
            draw_box(lcd, app, snap, i);
        } else {
            panel(lcd, STBY_Y - 2, 15);
            draw_standby_line(lcd, app, i);
        }
    }
    if !snap.dual_standby {
        draw_details(lcd, app, snap.master_index);
        fill(lcd, INNER_X, METER_Y - 4, INNER_W, 1, t.card_hi);
    }

    if app.tx_prohibited() {
        draw_overlay(lcd, "TX PROHIBITED", t.alert);
    } else if app.no_channels_notice() {
        draw_overlay(lcd, "NO CHANNELS", t.warn);
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
                || p.is_transmitting != snap.is_transmitting
        }
    };

    if full {
        full_redraw(lcd, app, &snap);
        cache.tx_elapsed = None;
    } else if !overlay_active {
        let prev = cache.structural.unwrap();

        if prev.flags() != snap.flags() {
            draw_flags(lcd, app);
        }
        if snap.is_transmitting {
            let secs = app.tx_elapsed_seconds();
            if cache.tx_elapsed != Some(secs) {
                cache.tx_elapsed = Some(secs);
                draw_flags(lcd, app);
            }
        }

        for i in 0..2usize {
            let p = &prev.sides[i];
            let s = &snap.sides[i];
            let is_master = i == snap.master_index;
            let channel_changed = (p.is_channel_mode, p.channel_num) != (s.is_channel_mode, s.channel_num);
            let dtmf_changed = prev.dtmf_dial_active != snap.dtmf_dial_active
                || (snap.dtmf_dial_active && prev.dtmf_dial_len != snap.dtmf_dial_len);

            if !has_box(&snap, i) {
                if channel_changed || p.freq_hz != s.freq_hz || dtmf_changed {
                    draw_standby_line(lcd, app, i);
                }
                continue;
            }
            let input_changed = if is_master {
                (prev.channel_input_len, prev.freq_input_len) != (snap.channel_input_len, snap.freq_input_len)
            } else {
                dtmf_changed
            };
            if input_changed || channel_changed || p.freq_hz != s.freq_hz {
                draw_header(lcd, app, &snap, i);
                draw_box(lcd, app, &snap, i);
            }
            if !snap.dual_standby
                && is_master
                && (p.modulation, p.power, p.subaudio_tx, p.subaudio_rx, p.offset_hz, p.freq_dir, p.wide_band, p.reversed)
                    != (s.modulation, s.power, s.subaudio_tx, s.subaudio_rx, s.offset_hz, s.freq_dir, s.wide_band, s.reversed)
            {
                draw_details(lcd, app, i);
            }
        }
    }
    // else: an unchanged overlay, already painted.

    cache.structural = Some(snap);

    draw_battery(lcd, app, &mut cache.battery, full);
    for i in 0..2usize {
        if has_box(&snap, i) {
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
    fill(lcd, x, y, w, 24, theme::t().bg);
    icons::draw_outline(lcd, Point::new(x, y), Size::new(w as u32, 24), color);
    text(lcd, label, &FONT_6X10, x + 8, y + 7, color);
}

// ---- status bar ----

fn draw_flags<D>(lcd: &mut D, app: &mut app::App)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    fill(lcd, INNER_X, 1, BATTERY_DIVIDER - INNER_X, STATUS_H - 2, t.card);
    let y = 2;
    let side = match (app.dual_standby_enabled(), app.master_index()) {
        (true, 0) => "DW A",
        (true, _) => "DW B",
        (false, 0) => "A",
        (false, _) => "B",
    };
    let mut x = text(lcd, side, &FONT_6X10, 5, y, t.accent) + 2;

    if app.is_transmitting() {
        let secs = app.tx_elapsed_seconds();
        let mut buf: TextBuf<8> = TextBuf::new();
        buf.write_str("TX ").ok();
        num(&mut buf, secs / 60, 1);
        buf.write_str(":").ok();
        num(&mut buf, secs % 60, 2);
        text(lcd, buf.as_str(), &FONT_6X10, x, y, t.alert);
        return;
    }
    x = text(lcd, app.chan_mode_short(), &FONT_6X10, x, y, t.dim) + 2;
    for (on, label, color) in [(app.power_save_active(), "PS", t.dim), (app.vox_enabled(), "VOX", t.ok)] {
        if on {
            x = text(lcd, label, &FONT_6X10, x, y, color) + 2;
        }
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
    fill(lcd, BATTERY_DIVIDER + 1, 1, PANEL_X + PANEL_W - BATTERY_DIVIDER - 2, STATUS_H - 2, t.card);
    let icon_x = RIGHT - icons::BATTERY_W;
    let mut volts: TextBuf<8> = TextBuf::new();
    num(&mut volts, cv as u32 / 100, 1);
    volts.write_str(".").ok();
    num(&mut volts, cv as u32 / 10 % 10, 1);
    volts.write_str("V").ok();
    let charge = match bars {
        0 | 1 => t.alert,
        2 => t.warn,
        _ => t.ok,
    };
    text_right(lcd, volts.as_str(), &FONT_6X10, icon_x - 3, 2, charge);
    icons::draw_battery(lcd, icon_x, 2, bars, t.dim, charge);
}

// ---- the active side ----

/// Channel number and name (or the frequency, in Name display mode).
fn draw_header<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    let dual = snap.dual_standby;
    let y = header_y(snap, i);
    let is_master = i == snap.master_index;
    fill(lcd, LEFT, y, HEADER_TEXT_END - LEFT, 10, t.card);

    let mut x = LEFT;
    if dual {
        x = text(lcd, if i == 0 { "A" } else { "B" }, &FONT_6X10, x, y, if is_master { t.accent } else { t.dim }) + 6;
    }
    let mut label: TextBuf<8> = TextBuf::new();
    let mut label_color = t.accent;
    if is_master && app.channel_input_len() > 0 {
        let len = app.channel_input_len();
        label.write_str("CH ").ok();
        for pos in 0..3usize {
            if pos + len < 3 {
                label.write_str("-").ok();
            } else {
                num(&mut label, app.freq_input_digit(pos + len - 3) as u32, 1);
            }
        }
        label_color = t.warn;
    } else if app.side_is_channel_mode(i) {
        label.write_str("CH ").ok();
        num(&mut label, app.side_channel_num(i) as u32 + 1, 3);
    } else {
        label.write_str("VFO").ok();
    }
    x = text(lcd, label.as_str(), &FONT_6X10, x, y, label_color) + 7;

    if !is_master && app.dtmf_dial_active() {
        return;
    }
    let name = app.side_name_str(i);
    let named = app.side_is_channel_mode(i) && !name.is_empty();
    if name_big(app, i) {
        let mut freq: TextBuf<12> = TextBuf::new();
        freq_text(&mut freq, shown_freq(app, i), 5);
        text(lcd, freq.as_str(), &FONT_6X10, x, y, t.text);
    } else if named {
        let room = ((HEADER_TEXT_END - x) / SMALL_W).max(0) as usize;
        text(lcd, &name[..name.len().min(room)], &FONT_6X10, x, y, t.text);
    }
}

/// The large readout: digits, the name in channel mode, typing, or DTMF.
fn draw_box<D>(lcd: &mut D, app: &mut app::App, snap: &Snapshot, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    let y = box_top(snap, i);
    let is_master = i == snap.master_index;
    paint_backdrop(lcd, app, y - 2, READ_H + 4, true);
    let color = if is_master { t.value } else { t.dim };

    if is_master && app.freq_input_len() > 0 {
        big_freq(lcd, t, 0, RIGHT - 1, y, color, Some(app));
        return;
    }
    if !is_master && app.dtmf_dial_active() {
        draw_dtmf(lcd, app, 8, y + 9);
        return;
    }
    if name_big(app, i) {
        let name = app.side_name_str(i);
        let count = name.chars().count() as i32;
        let scale_x = if count <= INNER_W / (SMALL_W * 3) { 3 } else { 2 };
        let x = (SCREEN_W - count * SMALL_W * scale_x) / 2;
        let mut big = Scaled { lcd, x, y, scale_x };
        text(&mut big, name, &FONT_6X10, 0, 0, color);
        return;
    }
    big_freq(lcd, t, shown_freq(app, i), RIGHT - 1, y, color, None);
}

/// Mode, power, tone, shift, bandwidth and reverse, as one line.
fn draw_details<D>(lcd: &mut D, app: &mut app::App, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    fill(lcd, INNER_X, DETAIL_Y, INNER_W, 10, t.card);
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
    let fields = [
        (modulation, t.ok),
        (power, t.ok),
        (tone_mode_label(app.side_subaudio_tx(i), app.side_subaudio_rx(i)), t.accent),
        (shift.as_str(), t.accent),
        (if app.side_wide_band(i) { "" } else { "NAR" }, t.ok),
        (if app.side_reversed(i) { "REV" } else { "" }, t.alert),
    ];
    let mut x = LEFT;
    for (label, color) in fields {
        if !label.is_empty() && x + label.len() as i32 * SMALL_W <= RIGHT {
            x = text(lcd, label, &FONT_6X10, x, DETAIL_Y, color) + 9;
        }
    }
}

/// Single layout: the other side, in the compact standby panel.
fn draw_standby_line<D>(lcd: &mut D, app: &mut app::App, i: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    fill(lcd, INNER_X, STBY_Y, INNER_W, 10, t.card);
    let y = STBY_Y;
    let mut x = text(lcd, "STBY", &FONT_6X10, LEFT, y, t.accent) + 6;
    x = text(lcd, if i == 0 { "A" } else { "B" }, &FONT_6X10, x, y, t.accent) + 6;
    if app.dtmf_dial_active() {
        draw_dtmf(lcd, app, x, y);
        return;
    }
    let hz = app.side_freq_hz(i);
    let mut f: TextBuf<12> = TextBuf::new();
    freq_text(&mut f, hz, 5);
    text_right(lcd, f.as_str(), &FONT_6X10, RIGHT, y, t.text);
    let name = app.side_name_str(i);
    let room = ((RIGHT - 9 * SMALL_W - 6 - x) / SMALL_W).max(0) as usize;
    if app.side_is_channel_mode(i) && !name.is_empty() {
        text(lcd, &name[..name.len().min(room)], &FONT_6X10, x, y, t.dim);
    } else {
        text(lcd, "VFO", &FONT_6X10, x, y, t.dim);
    }
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
    text(lcd, buf.as_str(), &FONT_6X10, x, y, theme::t().warn);
}

/// RX / TX / last-signal marker at the right end of a side's header.
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
    let y = header_y(snap, i);
    fill(lcd, RIGHT - 20, y, 20, 10, t.card);
    let (label, color) = match kind {
        RxStatusText::Tx => ("TX", t.alert),
        RxStatusText::Rx => ("RX", t.ok),
        RxStatusText::LastSignal => (">>", t.warn),
        RxStatusText::None => return,
    };
    text_right(lcd, label, &FONT_6X10, RIGHT, y, color);
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

/// The frequency in the large digits across the screen, for the scan screen.
pub(super) fn draw_big_freq<D>(lcd: &mut D, hz: u32, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let t = theme::t();
    big_freq(lcd, t, hz, RIGHT - 2, y, t.value, None);
}

// ---- meter ----

const S9_SEGMENTS: u8 = 9;
const METER_SEGMENTS: u8 = S9_SEGMENTS + 4;

fn meter_state(app: &app::App) -> MeterState {
    if app.is_transmitting() {
        return MeterState {
            tx: true,
            level: ((app.mic_level() as u16 * METER_SEGMENTS as u16 + 127) / 255) as u8,
            live: true,
            peak_dbm: None,
            over_s9: 0,
            side: app.master_index() as u8,
        };
    }
    if app.audio_open() {
        MeterState {
            tx: false,
            level: app.s_meter_level(),
            live: true,
            peak_dbm: None,
            over_s9: app.s_meter_over_s9_dbm(),
            side: app.watching_index() as u8,
        }
    } else {
        match app.last_signal_peak() {
            Some((side, dbm)) => MeterState {
                tx: false,
                level: app::s_meter_level_of(dbm),
                live: false,
                peak_dbm: Some(dbm),
                over_s9: 0,
                side: side as u8,
            },
            None => MeterState {
                tx: false,
                level: 0,
                live: false,
                peak_dbm: None,
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
    let side_changed = last.map(|prev| prev.side) != Some(state.side);
    let redraw_bar = force || side_changed || last.is_some_and(|prev| {
        (prev.level, prev.live, prev.tx) != (state.level, state.live, state.tx)
    });
    *last = Some(state);
    let t = theme::t();
    let colors = if !state.live {
        (t.dim, t.dim)
    } else if state.tx {
        (t.alert, t.alert)
    } else {
        (t.ok, t.warn)
    };

    if snap.dual_standby {
        if redraw_bar {
            for side in 0..2usize {
                let active = side == state.side as usize;
                if active || force || side_changed {
                    let level = if active { state.level } else { 0 };
                    bar(lcd, box_top(snap, side) + READ_H + 6, 11, 4, level, colors);
                }
            }
        }
        return;
    }

    if redraw_bar {
        bar(lcd, METER_Y + 9, 8, 8, state.level, colors);
    }

    let mut line: TextBuf<12> = TextBuf::new();
    match state.peak_dbm {
        Some(dbm) => {
            line.write_str("-").ok();
            num(&mut line, dbm.unsigned_abs(), 1);
            line.write_str("dBm").ok();
        }
        None if state.over_s9 > 0 => {
            line.write_str("S9+").ok();
            num(&mut line, state.over_s9 as u32, 1);
        }
        None if state.live && !state.tx => {
            line.write_str("S").ok();
            num(&mut line, app.s_meter_s_number() as u32, 1);
        }
        None => {
            line.write_str(if state.tx { "MIC" } else { "S--" }).ok();
        }
    }
    fill(lcd, 111, METER_Y - 1, 46, 12, t.card_hi);
    text_right(lcd, line.as_str(), &FONT_6X10, RIGHT - 2, METER_Y, if state.live { t.text } else { t.dim });
}

/// Stepped segments with a wider gap at S9.
#[inline(never)]
fn bar<D>(lcd: &mut D, bottom: i32, pitch: i32, height: i32, level: u8, colors: (Rgb565, Rgb565))
where
    D: DrawTarget<Color = Rgb565>,
{
    let off = theme::t().card_hi;
    let mut x = LEFT;
    for segment in 0..METER_SEGMENTS {
        if segment == S9_SEGMENTS {
            x += 2;
        }
        let h = height - i32::from(segment < 3) - i32::from(segment < 6);
        let color = if segment >= level {
            off
        } else if segment < S9_SEGMENTS {
            colors.0
        } else {
            colors.1
        };
        fill(lcd, x, bottom - h, pitch - 2, h, color);
        x += pitch;
    }
}
