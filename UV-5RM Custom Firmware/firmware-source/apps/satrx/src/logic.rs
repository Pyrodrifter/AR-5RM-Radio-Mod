use bf5rh_abi::{Api, AppEvent, AppResult};
use sgp4mini::{civil_from_days, days_from_civil};

const SLOT_BASE: u32 = 0x14_C000;
const SLOT_SIZE: u32 = 0x8000;
const SLOT_COUNT: u32 = 9;
const PLAN_SIZE: u32 = 4096;
const PLAN_HEADER: u32 = 32;
const RECORD: u32 = 260;
const POINTS: u32 = 41;
const MAX_PASSES: usize = 12;
const VISIBLE: usize = 5;
const DAY: u32 = 86_400;
const BLUE: u16 = 0x1033;
const PANEL: u16 = 0x08A5;
const WHITE: u16 = 0xFFFF;
const CYAN: u16 = 0x4EFD;
const GRAY: u16 = 0xBDF7;
const BLACK: u16 = 0;

#[derive(Clone, Copy)]
struct Pass {
    start: u32,
    end: u32,
    peak: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Passes,
    Clock,
    Track,
}

struct State {
    base: u32,
    freq: u32,
    wide: bool,
    name: [u8; 12],
    count: usize,
    passes: [Pass; MAX_PASSES],
    selected: usize,
    page: Page,
    digits: [u8; 12],
    len: usize,
    bad_clock: bool,
    now: u32,
    hz: u32,
    az: u16,
    el: i16,
    listening: bool,
    dirty: bool,
}

const EMPTY: State = State {
    base: 0,
    freq: 0,
    wide: true,
    name: [0; 12],
    count: 0,
    passes: [Pass { start: 0, end: 0, peak: 0 }; MAX_PASSES],
    selected: 0,
    page: Page::Passes,
    digits: [0; 12],
    len: 0,
    bad_clock: false,
    now: 0,
    hz: 0,
    az: 0,
    el: 0,
    listening: false,
    dirty: true,
};

static mut STATE: State = EMPTY;

pub fn entry(api: &Api, event: AppEvent) -> AppResult {
    let s = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match event {
        AppEvent::Enter => {
            *s = EMPTY;
            load_plan(api, s);
            s.now = (api.utc_get)();
            s.selected = s.passes[..s.count].iter().position(|p| p.end > s.now).unwrap_or(0);
        }
        AppEvent::Key { id, kind: 2 } => return key(api, s, id),
        AppEvent::Tick { .. } if s.page != Page::Clock => {
            let now = (api.utc_get)();
            if now != s.now {
                let previous = s.now;
                s.now = now;
                if s.page == Page::Track {
                    update_track(api, s);
                } else if s.passes[..s.count].iter().any(|pass| (previous >= pass.end) != (now >= pass.end)) {
                    s.dirty = true;
                }
            }
        }
        AppEvent::Draw if s.dirty => draw(api, s),
        _ => {}
    }
    AppResult::Continue
}

fn read(api: &Api, addr: u32, bytes: &mut [u8]) -> bool {
    (api.nor_read)(addr, bytes.as_mut_ptr(), bytes.len() as u32)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn load_plan(api: &Api, s: &mut State) {
    let mut header = [0; 32];
    for slot in 0..SLOT_COUNT {
        let base = SLOT_BASE + slot * SLOT_SIZE;
        if !read(api, base, &mut header)
            || &header[..4] != b"OVL1"
            || &header[12..25] != b"SATELLITE RX\0"
        {
            continue;
        }
        s.base = base + SLOT_SIZE - PLAN_SIZE;
        break;
    }
    if s.base == 0 || !read(api, s.base, &mut header) || &header[..4] != b"SRX1" {
        return;
    }
    let count = usize::from(header[4]);
    let hz = u32_at(&header, 20);
    if !(1..=MAX_PASSES).contains(&count)
        || !matches!(hz, 136_000_000..=174_000_000 | 400_000_000..=520_000_000)
        || !valid_checksum(api, s.base)
    {
        return;
    }
    s.name.copy_from_slice(&header[8..20]);
    s.freq = hz;
    s.wide = header[5] != 0;
    let mut record = [0; 10];
    for index in 0..count {
        if !read(api, s.base + PLAN_HEADER + index as u32 * RECORD, &mut record) {
            return;
        }
        let start = u32_at(&record, 0);
        let end = u32_at(&record, 4);
        if end <= start || end - start > 3600 {
            return;
        }
        s.passes[index] = Pass { start, end, peak: u16_at(&record, 8) };
    }
    s.count = count;
}

fn valid_checksum(api: &Api, base: u32) -> bool {
    let mut chunk = [0; 64];
    let mut crc = 0u16;
    for offset in (0..PLAN_SIZE).step_by(chunk.len()) {
        if !read(api, base + offset, &mut chunk) {
            return false;
        }
        let last = offset + 64 == PLAN_SIZE;
        for &byte in &chunk[..if last { 62 } else { 64 }] {
            crc ^= u16::from(byte) << 8;
            for _ in 0..8 {
                crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x1021 } else { 0 };
            }
        }
    }
    crc == u16::from_be_bytes([chunk[62], chunk[63]])
}

fn mute(api: &Api, s: &mut State) {
    if s.listening {
        (api.set_holds_rx)(false);
        (api.set_af_out)(false);
        (api.set_speaker)(false);
        s.listening = false;
    }
}

fn update_track(api: &Api, s: &mut State) {
    s.dirty = true;
    let pass = s.passes[s.selected];
    if s.now < pass.start || s.now >= pass.end {
        mute(api, s);
    }
    let duration = pass.end - pass.start;
    let position = s.now.saturating_sub(pass.start).min(duration) * (POINTS - 1);
    let index = (position / duration).min(POINTS - 2);
    let numerator = (position - index * duration) as i32;
    let mut bytes = [0; 12];
    let addr = s.base + PLAN_HEADER + s.selected as u32 * RECORD + 12 + index * 6;
    if !read(api, addr, &mut bytes) {
        mute(api, s);
        return;
    }
    let scale = |delta: i32| delta * numerator / duration as i32;
    let az1 = i32::from(u16_at(&bytes, 0));
    let az2 = i32::from(u16_at(&bytes, 6));
    let az_delta = (az2 - az1 + 5400) % 3600 - 1800;
    s.az = (az1 + scale(az_delta)).rem_euclid(3600) as u16;
    let interpolate = |at| {
        let a = i32::from(u16_at(&bytes, at) as i16);
        let b = i32::from(u16_at(&bytes, at + 6) as i16);
        a + scale(b - a)
    };
    s.el = interpolate(2) as i16;
    let hz = s.freq.saturating_add_signed(interpolate(4)) / 10 * 10;
    if (pass.start..pass.end).contains(&s.now) {
        if !s.listening {
            (api.set_monitor)(false);
            (api.set_modulation)(0);
            (api.set_bandwidth)(s.wide);
            (api.set_subaudio_rx)(0);
            (api.set_sql_level)((api.settings_get)(0).min(9) as u8);
            (api.set_rx_freq)(hz);
            (api.enter_rx)();
            s.listening = true;
        } else if hz != s.hz {
            (api.retune_rx)(hz, s.wide);
        }
    }
    s.hz = hz;
}

fn commit_clock(api: &Api, s: &mut State) {
    if s.len != 12 {
        return;
    }
    let pair = |at: usize| i32::from(s.digits[at]) * 10 + i32::from(s.digits[at + 1]);
    let (y, m, d) = (2000 + pair(0), pair(2), pair(4));
    let (h, min, sec) = (pair(6), pair(8), pair(10));
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h >= 24 || min >= 60 || sec >= 60 {
        s.bad_clock = true;
        return;
    }
    let days = days_from_civil(y, m, d);
    if civil_from_days(days) != (y, m, d) {
        s.bad_clock = true;
        return;
    }
    s.now = days as u32 * DAY + (h * 3600 + min * 60 + sec) as u32;
    (api.utc_set)(s.now);
    s.selected = s.passes[..s.count].iter().position(|p| p.end > s.now).unwrap_or(0);
    s.page = Page::Passes;
}

fn key(api: &Api, s: &mut State, id: u8) -> AppResult {
    s.dirty = true;
    match (s.page, id) {
        (Page::Passes, 13) => return AppResult::Exit,
        (Page::Passes, 14) => s.selected = s.selected.saturating_sub(1),
        (Page::Passes, 15) if s.count > 0 => s.selected = (s.selected + 1).min(s.count - 1),
        (Page::Passes, 10) | (Page::Passes, 12) if s.now == 0 || id == 10 => {
            s.page = Page::Clock;
            s.len = 0;
            s.bad_clock = false;
        }
        (Page::Passes, 12) if s.count > 0 => {
            s.page = Page::Track;
            update_track(api, s);
        }
        (Page::Clock, 0..=9) if s.len < 12 => {
            s.digits[s.len] = id;
            s.len += 1;
            s.bad_clock = false;
        }
        (Page::Clock, 12) => commit_clock(api, s),
        (Page::Clock, 13) if s.len > 0 => {
            s.len -= 1;
            s.bad_clock = false;
        }
        (Page::Clock, 13) | (Page::Track, 13) => {
            mute(api, s);
            s.page = Page::Passes;
        }
        _ => {}
    }
    AppResult::Continue
}

fn txt(api: &Api, x: i16, y: i16, value: &[u8], color: u16, bg: u16, font: u8) {
    (api.draw_text)(x, y, value.as_ptr(), value.len() as u16, color, bg, font);
}

fn number(api: &Api, x: i16, y: i16, value: u32, color: u16, bg: u16) {
    let mut bytes = [0; 10];
    let len = (api.fmt_u32)(value, bytes.as_mut_ptr()) as usize;
    txt(api, x, y, &bytes[..len], color, bg, 1);
}

fn two(bytes: &mut [u8], value: u32) {
    bytes[0] = b'0' + (value / 10) as u8;
    bytes[1] = b'0' + (value % 10) as u8;
}

fn date_label(time: u32) -> [u8; 11] {
    let (_, month, day) = civil_from_days((time / DAY) as i32);
    let mut b = *b"00/00 00:00";
    two(&mut b[0..2], month as u32);
    two(&mut b[3..5], day as u32);
    two(&mut b[6..8], time % DAY / 3600);
    two(&mut b[9..11], time % 3600 / 60);
    b
}

fn draw(api: &Api, s: &mut State) {
    (api.fill_rect)(0, 0, 160, 128, BLACK);
    (api.fill_rect)(0, 0, 160, 20, BLUE);
    (api.fill_rect)(0, 110, 160, 18, PANEL);
    let title: &[u8] = match s.page {
        Page::Passes => b"SATELLITE RX",
        Page::Clock => b"SET UTC",
        Page::Track => {
            let len = s.name.iter().position(|&byte| byte == 0).unwrap_or(s.name.len());
            &s.name[..len]
        }
    };
    txt(api, 4, 1, title, WHITE, BLUE, 2);
    let hint: &[u8] = match s.page {
        Page::Passes => b"MENU TRACK  * UTC  EXIT",
        Page::Clock => b"MENU SAVE     EXIT DELETE",
        Page::Track => b"EXIT BACK TO PASSES",
    };
    txt(api, 4, 114, hint, GRAY, PANEL, 1);
    match s.page {
        Page::Clock => {
            txt(api, 5, 35, b"YYMMDD HHMMSS (UTC)", GRAY, BLACK, 1);
            let mut line = *b"------ ------";
            for i in 0..s.len {
                line[i + usize::from(i >= 6)] = b'0' + s.digits[i];
            }
            txt(api, 5, 54, &line, CYAN, BLACK, 2);
            if s.bad_clock {
                txt(api, 5, 86, b"CHECK DATE AND TIME", CYAN, BLACK, 1);
            }
        }
        Page::Passes if s.count == 0 => txt(api, 5, 45, b"UPLOAD PASSES FROM CPS", GRAY, BLACK, 1),
        Page::Passes => {
            let top = s.selected.saturating_sub(VISIBLE - 1);
            for (row, index) in (top..s.count.min(top + VISIBLE)).enumerate() {
                let y = 23 + row as i16 * 14;
                let pass = s.passes[index];
                let bg = if index == s.selected { BLUE } else { BLACK };
                (api.fill_rect)(0, y, 160, 14, bg);
                let color = if s.now >= pass.end { GRAY } else { WHITE };
                txt(api, 5, y + 2, &date_label(pass.start), color, bg, 1);
                txt(api, 82, y + 2, b"UTC", GRAY, bg, 1);
                number(api, 113, y + 2, u32::from(pass.peak), CYAN, bg);
                txt(api, 132, y + 2, b"DEG", GRAY, bg, 1);
            }
            let footer: &[u8] = if s.now == 0 {
                b"SET UTC BEFORE TRACKING"
            } else if s.now >= s.passes[s.count - 1].end {
                b"PASS PLAN EXPIRED"
            } else {
                b"START TIME / PEAK ELEV"
            };
            txt(api, 5, 97, footer, CYAN, BLACK, 1);
        }
        Page::Track => {
            let pass = s.passes[s.selected];
            let label: &[u8] = if s.now < pass.start { b"STARTS IN" }
                else if s.now < pass.end { b"ENDS IN" }
                else { b"PASS ENDED" };
            txt(api, 5, 26, label, CYAN, BLACK, 1);
            if s.now < pass.end {
                let seconds = if s.now < pass.start { pass.start - s.now } else { pass.end - s.now };
                if seconds < DAY {
                    let mut b = *b"00:00:00";
                    two(&mut b[0..2], seconds / 3600);
                    two(&mut b[3..5], seconds % 3600 / 60);
                    two(&mut b[6..8], seconds % 60);
                    txt(api, 77, 26, &b, WHITE, BLACK, 1);
                } else {
                    txt(api, 68, 26, &date_label(pass.start), WHITE, BLACK, 1);
                    txt(api, 138, 26, b"UTC", GRAY, BLACK, 1);
                }
            }
            txt(api, 5, 47, b"AZ DEG", GRAY, BLACK, 1);
            number(api, 50, 47, u32::from(s.az / 10), WHITE, BLACK);
            txt(api, 82, 47, b"EL DEG", GRAY, BLACK, 1);
            number(api, 127, 47, s.el.max(0) as u32 / 10, WHITE, BLACK);
            let mut freq = [0; 16];
            let len = (api.fmt_freq)(s.hz, freq.as_mut_ptr()) as usize;
            txt(api, 5, 67, &freq[..len], WHITE, BLACK, 2);
            txt(api, 5, 95, if s.listening { b"FM RX  DOPPLER ON" } else { b"FM DOWNLINK" }, CYAN, BLACK, 1);
        }
    }
    s.dirty = false;
}
