//! The emulator window's radio body: the LCD in a bezel, a clickable keypad,
//! side keys, TX/RX lights and a small toolbar. Pure drawing and hit-testing;
//! main.rs decides what a click does.

use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10, FONT_7X13_BOLD, FONT_9X15_BOLD};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::{Rgb565, Rgb888};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{
    Circle, CornerRadii, PrimitiveStyle, Rectangle, RoundedRectangle, Triangle,
};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{K_AB, K_BAND, K_DOWN, K_EXIT, K_MENU, K_POUND, K_SIDE1, K_SIDE2, K_STAR, K_UP, K_VM};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Key(u8),
    Ptt,
    Signal,
    Rebuild,
    Shot,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Key,
    Accent,
    Side,
    Tool,
}

pub struct Button {
    pub action: Action,
    rect: Rectangle,
    label: &'static str,
    sub: &'static str,
    look: Look,
    vertical: bool,
}

pub struct Skin {
    pub width: usize,
    pub height: usize,
    pub screen: Rectangle,
    pub scale: usize,
    body: Rectangle,
    pub buttons: Vec<Button>,
    background: Vec<u32>,
}

/// What lights up this frame.
pub struct State<'a> {
    pub hover: Option<Action>,
    pub held: &'a dyn Fn(Action) -> bool,
    pub tx: bool,
    pub rx: bool,
    pub signal_latched: bool,
    pub status: &'a str,
}

const BG: Rgb888 = Rgb888::new(0x14, 0x15, 0x17);
const BODY: Rgb888 = Rgb888::new(0x24, 0x27, 0x2b);
const BODY_EDGE: Rgb888 = Rgb888::new(0x3a, 0x3f, 0x46);
const BEZEL: Rgb888 = Rgb888::new(0x08, 0x09, 0x0a);
const KEY: Rgb888 = Rgb888::new(0x33, 0x37, 0x3d);
const KEY_HOVER: Rgb888 = Rgb888::new(0x40, 0x45, 0x4c);
const KEY_TOP: Rgb888 = Rgb888::new(0x4a, 0x50, 0x58);
const KEY_DOWN: Rgb888 = Rgb888::new(0x8e, 0x1b, 0x1b);
const ACCENT: Rgb888 = Rgb888::new(0x5a, 0x16, 0x16);
const ACCENT_HOVER: Rgb888 = Rgb888::new(0x70, 0x1c, 0x1c);
const TEXT: Rgb888 = Rgb888::new(0xee, 0xee, 0xee);
const SUBTEXT: Rgb888 = Rgb888::new(0xe0, 0x8a, 0x3c);
const DIM: Rgb888 = Rgb888::new(0x8a, 0x90, 0x98);
const GRILLE: Rgb888 = Rgb888::new(0x16, 0x18, 0x1a);
const LED_OFF: Rgb888 = Rgb888::new(0x30, 0x30, 0x30);
const LED_TX: Rgb888 = Rgb888::new(0xff, 0x30, 0x30);
const LED_RX: Rgb888 = Rgb888::new(0x30, 0xe0, 0x60);

/// A u32 0RGB framebuffer that embedded-graphics can draw on.
pub struct Canvas<'a> {
    pub buf: &'a mut [u32],
    pub w: usize,
    pub h: usize,
}

impl OriginDimensions for Canvas<'_> {
    fn size(&self) -> Size {
        Size::new(self.w as u32, self.h as u32)
    }
}

impl DrawTarget for Canvas<'_> {
    type Color = Rgb888;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < self.w && (p.y as usize) < self.h {
                self.buf[p.y as usize * self.w + p.x as usize] =
                    (c.r() as u32) << 16 | (c.g() as u32) << 8 | c.b() as u32;
            }
        }
        Ok(())
    }
}

fn fill(c: Rgb888) -> PrimitiveStyle<Rgb888> {
    PrimitiveStyle::with_fill(c)
}

fn rounded(r: Rectangle, radius: u32) -> RoundedRectangle {
    RoundedRectangle::new(r, CornerRadii::new(Size::new(radius, radius)))
}

fn text_at(c: &mut Canvas, s: &str, at: Point, font: &MonoFont<'static>, color: Rgb888, align: Alignment) {
    let style = MonoTextStyle::new(font, color);
    let ts = TextStyleBuilder::new().alignment(align).baseline(Baseline::Middle).build();
    Text::with_text_style(s, at, style, ts).draw(c).ok();
}

fn rect(x: i32, y: i32, w: u32, h: u32) -> Rectangle {
    Rectangle::new(Point::new(x, y), Size::new(w, h))
}

impl Skin {
    pub fn new(scale: usize) -> Skin {
        let s = scale as i32;
        let side = 52; // room on the left for the side keys
        let pad = 34;
        let screen_w = 160 * s;
        let screen_h = 128 * s;
        let body_x = side;
        let body_y = 14;
        let top = 62; // speaker grille, name, lights
        let screen = rect(body_x + pad, body_y + top, screen_w as u32, screen_h as u32);

        let key_h = if s >= 3 { 46 } else { 38 };
        let gap = if s >= 3 { 10 } else { 8 };
        let grid_x = screen.top_left.x;
        let mut y = screen.top_left.y + screen_h + 28;
        let mut buttons = Vec::new();

        let row = |buttons: &mut Vec<Button>, y: i32, items: &[(Action, &'static str, &'static str, Look)]| {
            let n = items.len() as i32;
            let w = (screen_w - gap * (n - 1)) / n;
            for (i, &(action, label, sub, look)) in items.iter().enumerate() {
                let x = grid_x + i as i32 * (w + gap);
                buttons.push(Button { action, rect: rect(x, y, w as u32, key_h as u32), label, sub, look, vertical: false });
            }
        };

        use Action::Key as K;
        row(&mut buttons, y, &[
            (K(K_MENU), "MENU", "", Look::Accent),
            (K(K_UP), "^", "", Look::Key),
            (K(K_DOWN), "v", "", Look::Key),
            (K(K_EXIT), "EXIT", "", Look::Accent),
        ]);
        y += key_h + gap;
        row(&mut buttons, y, &[
            (K(K_AB), "A/B", "", Look::Key),
            (K(K_VM), "V/M", "", Look::Key),
            (K(K_BAND), "BAND", "", Look::Key),
        ]);
        y += key_h + gap;
        // Letters as the TEXT app's multi-tap uses them.
        let digits: [[(u8, &'static str, &'static str); 3]; 4] = [
            [(1, "1", ".,?"), (2, "2", "ABC"), (3, "3", "DEF")],
            [(4, "4", "GHI"), (5, "5", "JKL"), (6, "6", "MNO")],
            [(7, "7", "PQRS"), (8, "8", "TUV"), (9, "9", "WXYZ")],
            [(K_STAR, "*", "LOCK"), (0, "0", "SPACE"), (K_POUND, "#", "SCAN")],
        ];
        for r in digits {
            let items: Vec<_> = r.iter().map(|&(id, l, sub)| (K(id), l, sub, Look::Key)).collect();
            row(&mut buttons, y, &items);
            y += key_h + gap;
        }

        y += 6;
        let tool_h = 30;
        row(&mut buttons, y, &[
            (Action::Signal, "SIGNAL", "S", Look::Tool),
            (Action::Rebuild, "REBUILD", "F5", Look::Tool),
            (Action::Shot, "SCREENSHOT", "F12", Look::Tool),
        ]);
        for b in buttons.iter_mut().rev().take(3) {
            b.rect.size.height = tool_h as u32;
        }
        y += tool_h + 26; // room for the status line

        let body_w = (screen_w + 2 * pad) as u32;
        let body_h = (y - body_y) as u32;
        let body = rect(body_x, body_y, body_w, body_h);

        // Side keys stick out of the body's left edge, under the screen's top.
        let sx = body_x - 30;
        let sy = screen.top_left.y + 10;
        let side_key = |action, label, y, h| Button {
            action,
            rect: rect(sx, y, 34, h),
            label,
            sub: "",
            look: Look::Side,
            vertical: true,
        };
        buttons.push(side_key(Action::Ptt, "PTT", sy, 130));
        buttons.push(side_key(Action::Key(K_SIDE1), "FM", sy + 146, 58));
        buttons.push(side_key(Action::Key(K_SIDE2), "LIGHT", sy + 216, 86));

        let width = (body_x + body_w as i32 + 18) as usize;
        let height = (body_y + body_h as i32 + 14) as usize;
        let mut skin = Skin { width, height, screen, scale, body, buttons, background: Vec::new() };
        skin.background = skin.paint_background();
        skin
    }

    fn paint_background(&self) -> Vec<u32> {
        let mut buf = vec![0u32; self.width * self.height];
        let mut c = Canvas { buf: &mut buf, w: self.width, h: self.height };
        c.clear(BG).ok();

        // Body with a lighter rim.
        let rim = self.body.offset(2);
        rounded(rim, 26).into_styled(fill(BODY_EDGE)).draw(&mut c).ok();
        rounded(self.body, 24).into_styled(fill(BODY)).draw(&mut c).ok();

        // Speaker grille.
        let gx = self.body.top_left.x + 30;
        let gy = self.body.top_left.y + 16;
        for i in 0..4 {
            rounded(rect(gx, gy + i * 8, 120, 4), 2).into_styled(fill(GRILLE)).draw(&mut c).ok();
        }
        text_at(&mut c, "UV-5RM", Point::new(gx + 136, gy + 9), &FONT_9X15_BOLD, TEXT, Alignment::Left);
        text_at(&mut c, "SCREEN EMULATOR", Point::new(gx + 136, gy + 25), &FONT_6X10, DIM, Alignment::Left);

        // LCD bezel.
        let bezel = self.screen.offset(10);
        rounded(bezel, 10).into_styled(fill(BEZEL)).draw(&mut c).ok();
        buf
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Action> {
        let p = Point::new(x, y);
        self.buttons.iter().find(|b| b.rect.contains(p)).map(|b| b.action)
    }

    pub fn render(&self, buf: &mut [u32], lcd: &[u16], st: &State) {
        buf.copy_from_slice(&self.background);

        // LCD, scaled up with square pixels.
        let s = self.scale;
        let (ox, oy) = (self.screen.top_left.x as usize, self.screen.top_left.y as usize);
        for y in 0..128 {
            for x in 0..160 {
                let p = lcd[y * 160 + x];
                let c: Rgb888 = Rgb565::from(embedded_graphics::pixelcolor::raw::RawU16::new(p)).into();
                let v = (c.r() as u32) << 16 | (c.g() as u32) << 8 | c.b() as u32;
                for dy in 0..s {
                    let row = (oy + y * s + dy) * self.width + ox + x * s;
                    buf[row..row + s].fill(v);
                }
            }
        }

        let mut c = Canvas { buf, w: self.width, h: self.height };

        // TX / RX lights, top right.
        let lx = self.body.top_left.x + self.body.size.width as i32 - 70;
        let ly = self.body.top_left.y + 26;
        for (i, (label, on, color)) in [("TX", st.tx, LED_TX), ("RX", st.rx, LED_RX)].into_iter().enumerate() {
            let x = lx + i as i32 * 32;
            if on {
                Circle::new(Point::new(x - 4, ly - 4), 18)
                    .into_styled(fill(Rgb888::new(color.r() / 4, color.g() / 4, color.b() / 4)))
                    .draw(&mut c)
                    .ok();
            }
            Circle::new(Point::new(x, ly), 10).into_styled(fill(if on { color } else { LED_OFF })).draw(&mut c).ok();
            text_at(&mut c, label, Point::new(x + 5, ly + 22), &FONT_6X10, DIM, Alignment::Center);
        }

        for b in &self.buttons {
            let held = (st.held)(b.action) || (b.action == Action::Signal && st.signal_latched);
            let hover = st.hover == Some(b.action);
            let base = match (b.look, held, hover) {
                (_, true, _) => KEY_DOWN,
                (Look::Accent, _, true) => ACCENT_HOVER,
                (Look::Accent, _, false) => ACCENT,
                (_, _, true) => KEY_HOVER,
                _ => KEY,
            };
            let r = if held { b.rect.translate(Point::new(0, 2)) } else { b.rect };
            let radius = if b.look == Look::Side { 8 } else { 9 };
            if !held {
                // A thin lighter top edge reads as a raised key.
                rounded(r, radius).into_styled(fill(KEY_TOP)).draw(&mut c).ok();
                rounded(rect(r.top_left.x, r.top_left.y + 2, r.size.width, r.size.height - 2), radius)
                    .into_styled(fill(base))
                    .draw(&mut c)
                    .ok();
            } else {
                rounded(r, radius).into_styled(fill(base)).draw(&mut c).ok();
            }
            let center = r.center();

            if b.vertical {
                let n = b.label.len() as i32;
                let step = 14;
                let y0 = center.y - (n - 1) * step / 2;
                for (i, ch) in b.label.chars().enumerate() {
                    let mut s = [0u8; 4];
                    text_at(&mut c, ch.encode_utf8(&mut s), Point::new(center.x, y0 + i as i32 * step), &FONT_7X13_BOLD, TEXT, Alignment::Center);
                }
                continue;
            }

            match b.label {
                "^" | "v" => {
                    let (w, h) = (16, 10);
                    let (a, bb, cc) = if b.label == "^" {
                        (Point::new(center.x, center.y - h / 2), Point::new(center.x - w / 2, center.y + h / 2), Point::new(center.x + w / 2, center.y + h / 2))
                    } else {
                        (Point::new(center.x, center.y + h / 2), Point::new(center.x - w / 2, center.y - h / 2), Point::new(center.x + w / 2, center.y - h / 2))
                    };
                    Triangle::new(a, bb, cc).into_styled(fill(TEXT)).draw(&mut c).ok();
                }
                _ if b.look == Look::Tool => {
                    text_at(&mut c, b.label, Point::new(center.x - 8, center.y), &FONT_6X10, TEXT, Alignment::Center);
                    text_at(&mut c, b.sub, Point::new(r.top_left.x + r.size.width as i32 - 8, center.y), &FONT_6X10, DIM, Alignment::Right);
                }
                _ if !b.sub.is_empty() => {
                    text_at(&mut c, b.label, Point::new(center.x - 18, center.y), &FONT_10X20, TEXT, Alignment::Center);
                    text_at(&mut c, b.sub, Point::new(center.x + 4, center.y + 1), &FONT_6X10, SUBTEXT, Alignment::Left);
                }
                _ => {
                    let font = if b.label.len() <= 3 { &FONT_9X15_BOLD } else { &FONT_7X13_BOLD };
                    text_at(&mut c, b.label, center, font, TEXT, Alignment::Center);
                }
            }
        }

        // Status line along the bottom of the body.
        let sy = self.body.top_left.y + self.body.size.height as i32 - 16;
        text_at(&mut c, st.status, Point::new(self.screen.top_left.x, sy), &FONT_6X10, DIM, Alignment::Left);
    }
}
