use bf5rh_abi::{Api, AppEvent, AppResult};
use app_theme::pal;

const MENU: u8 = 12;
const EXIT: u8 = 13;
const UP: u8 = 14;
const DOWN: u8 = 15;
const POUND: u8 = 11;
const SINGLE: u8 = 2;
const REPEAT: u8 = 4;

// The CPS writes this block at the end of the app's own slot.
const SLOT_BASE: u32 = 0x14_C000;
const SLOT_BYTES: u32 = 0x8000;
const SLOT_COUNT: u32 = 9;
const CONFIG_BYTES: usize = 512;
const LIST_START: usize = 8;
const LIST_BYTES: usize = 78;
const LIST_MAX: usize = 6;
const MEMBER_MAX: usize = 32;
const NAME_BYTES: usize = 12;

const SETTLE: u32 = 2_000; // 200 ms for the receiver after each hop.
const OPEN_CONFIRM: u32 = 800;
const CLOSE_HANG: u32 = 5_000;
const LOG_CAPACITY: usize = 24;
const LOG_ROWS: usize = 6;
const F6: u8 = 1;
const F9: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Lists,
    Scan,
    Activity,
    Listen,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanPhase {
    Settle,
    Probe,
    Active,
}

#[derive(Clone, Copy)]
struct Hit {
    channel: u16,
    seconds: u16,
}

struct State {
    config: [u8; CONFIG_BYTES],
    list_count: usize,
    list: usize,
    member: usize,
    channel: u16,
    hz: u32,
    channel_name: [u8; NAME_BYTES],
    page: Page,
    phase: ScanPhase,
    timer: u32,
    active: u32,
    hits: [Hit; LOG_CAPACITY],
    head: usize,
    used: usize,
    selected: usize,
    empty_list: bool,
    dirty: bool,
    chrome: bool,
}

impl State {
    fn list_at(&self, list: usize) -> &[u8] {
        let at = LIST_START + list * LIST_BYTES;
        &self.config[at..at + LIST_BYTES]
    }

    fn hit_at(&self, back: usize) -> Hit {
        self.hits[(self.head + LOG_CAPACITY - 1 - back) % LOG_CAPACITY]
    }

    fn show(&mut self, page: Page) {
        self.page = page;
        self.chrome = true;
        self.dirty = true;
    }
}

static mut STATE: State = State {
    config: [0; CONFIG_BYTES],
    list_count: 0,
    list: 0,
    member: 0,
    channel: 0,
    hz: 0,
    channel_name: [0; NAME_BYTES],
    page: Page::Lists,
    phase: ScanPhase::Settle,
    timer: 0,
    active: 0,
    hits: [Hit { channel: 0, seconds: 0 }; LOG_CAPACITY],
    head: 0,
    used: 0,
    selected: 0,
    empty_list: false,
    dirty: true,
    chrome: true,
};

pub fn entry(api: &Api, event: AppEvent) -> AppResult {
    let s = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match event {
        AppEvent::Enter => {
            s.list_count = load_config(api, &mut s.config);
            s.list = 0;
            s.head = 0;
            s.used = 0;
            s.selected = 0;
            s.empty_list = false;
            s.show(Page::Lists);
        }
        AppEvent::Key { id, kind } => return key(api, s, id, kind),
        AppEvent::Tick { dt_100us } if s.page == Page::Scan => tick(api, s, dt_100us),
        AppEvent::Draw if s.dirty => draw(api, s),
        _ => {}
    }
    AppResult::Continue
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x1021 } else { 0 };
        }
    }
    crc
}

fn load_config(api: &Api, bytes: &mut [u8; CONFIG_BYTES]) -> usize {
    for slot in 0..SLOT_COUNT {
        let base = SLOT_BASE + slot * SLOT_BYTES;
        let mut header = [0u8; 24];
        if !(api.nor_read)(base, header.as_mut_ptr(), header.len() as u32)
            || &header[..4] != b"OVL1"
            || &header[12..] != b"RADIO WATCH\0"
        {
            continue;
        }
        if !(api.nor_read)(base + SLOT_BYTES - CONFIG_BYTES as u32, bytes.as_mut_ptr(), CONFIG_BYTES as u32) {
            return 0;
        }
        let count = usize::from(bytes[4]);
        let checksum = u16::from_be_bytes([bytes[510], bytes[511]]);
        if &bytes[..4] != b"RW01" || count > LIST_MAX || crc16(&bytes[..510]) != checksum {
            return 0;
        }
        for list in bytes[LIST_START..LIST_START + count * LIST_BYTES].chunks_exact(LIST_BYTES) {
            if !(1..=MEMBER_MAX).contains(&usize::from(list[NAME_BYTES])) {
                return 0;
            }
        }
        return count;
    }
    0
}

fn channel_record(api: &Api, channel: u16, record: &mut [u8; 32]) -> bool {
    (1..=1000).contains(&channel) && (api.chan_read)(channel - 1, record.as_mut_ptr(), 32)
}

fn tune(api: &Api, s: &mut State, channel: u16) -> bool {
    let mut record = [0; 32];
    if !channel_record(api, channel, &mut record) {
        return false;
    }
    let mut hz = 0u32;
    for &byte in record[..4].iter().rev() {
        let (high, low) = (byte >> 4, byte & 15);
        if high > 9 || low > 9 {
            return false;
        }
        hz = hz * 100 + u32::from(high * 10 + low);
    }
    hz *= 10;
    if !matches!(hz, 16_000_000..=560_000_000 | 740_000_000..=1_120_000_000) {
        return false;
    }
    s.channel = channel;
    s.hz = hz;
    s.channel_name.copy_from_slice(&record[20..32]);
    let am = (108_000_000..=136_000_000).contains(&hz);
    (api.set_monitor)(false);
    (api.set_modulation)(u8::from(am));
    (api.set_bandwidth)(record[15] & 0x40 == 0);
    (api.set_subaudio_rx)(if am { 0 } else { u16::from_le_bytes([record[8], record[9]]) });
    (api.set_sql_level)((api.settings_get)(0).clamp(1, 9) as u8);
    (api.set_rx_freq)(hz);
    (api.enter_rx)();
    s.dirty = true;
    true
}

fn mute(api: &Api) {
    (api.set_holds_rx)(false);
    (api.set_af_out)(false);
    (api.set_speaker)(false);
}

fn hop(api: &Api, s: &mut State) {
    let count = usize::from(s.list_at(s.list)[NAME_BYTES]);
    for _ in 0..count {
        let at = 14 + s.member * 2;
        let list = s.list_at(s.list);
        let channel = u16::from_le_bytes([list[at], list[at + 1]]);
        s.member = (s.member + 1) % count;
        if tune(api, s, channel) {
            s.phase = ScanPhase::Settle;
            s.timer = SETTLE;
            s.active = 0;
            return;
        }
    }
    s.empty_list = true;
    s.show(Page::Lists);
    mute(api);
}

fn save_hit(s: &mut State) {
    if s.phase != ScanPhase::Active {
        return;
    }
    let seconds = s.active.div_ceil(10_000).min(u32::from(u16::MAX)) as u16;
    s.hits[s.head] = Hit { channel: s.channel, seconds };
    s.head = (s.head + 1) % LOG_CAPACITY;
    s.used = (s.used + 1).min(LOG_CAPACITY);
    s.phase = ScanPhase::Settle;
}

fn tick(api: &Api, s: &mut State, dt: u32) {
    if s.phase == ScanPhase::Settle {
        s.timer = s.timer.saturating_sub(dt);
        if s.timer == 0 {
            s.phase = ScanPhase::Probe;
        }
        return;
    }
    let open = (api.squelch_open)();
    if open {
        s.active = s.active.saturating_add(dt);
        s.timer = 0;
        if s.phase == ScanPhase::Probe && s.active >= OPEN_CONFIRM {
            s.phase = ScanPhase::Active;
            s.dirty = true;
        }
    } else if s.phase == ScanPhase::Active {
        s.timer = s.timer.saturating_add(dt);
        if s.timer >= CLOSE_HANG {
            save_hit(s);
            hop(api, s);
        }
    } else {
        hop(api, s);
    }
}

fn key(api: &Api, s: &mut State, id: u8, kind: u8) -> AppResult {
    if kind != SINGLE && !(kind == REPEAT && matches!(id, UP | DOWN)) {
        return AppResult::Continue;
    }
    match (s.page, id) {
        (Page::Lists, MENU) if s.list_count > 0 => {
            s.member = 0;
            s.empty_list = false;
            s.show(Page::Scan);
            hop(api, s);
        }
        (Page::Lists, UP | DOWN) if s.list_count > 0 => {
            s.list = step(s.list, s.list_count, id);
            s.empty_list = false;
            s.dirty = true;
        }
        (Page::Lists | Page::Scan, POUND) => {
            if s.page == Page::Scan {
                save_hit(s);
                mute(api);
            }
            s.selected = 0;
            s.show(Page::Activity);
        }
        (Page::Scan, DOWN) => {
            save_hit(s);
            hop(api, s);
        }
        (Page::Scan, EXIT) => {
            save_hit(s);
            mute(api);
            s.show(Page::Lists);
        }
        (Page::Activity, UP | DOWN) if s.used > 0 => {
            s.selected = step(s.selected, s.used, id);
            s.dirty = true;
        }
        (Page::Activity, MENU) if s.used > 0 => {
            let hit = s.hit_at(s.selected);
            if tune(api, s, hit.channel) {
                s.show(Page::Listen);
            }
        }
        (Page::Listen, EXIT) => {
            mute(api);
            s.show(Page::Activity);
        }
        (Page::Activity, EXIT) => s.show(Page::Lists),
        (Page::Lists, EXIT) => return AppResult::Exit,
        _ => {}
    }
    AppResult::Continue
}

fn step(index: usize, count: usize, key: u8) -> usize {
    if key == UP { index.saturating_sub(1) } else { (index + 1).min(count - 1) }
}

fn text(api: &Api, x: i16, y: i16, value: &[u8], fg: u16, bg: u16, font: u8) {
    (api.draw_text)(x, y, value.as_ptr(), value.len() as u16, fg, bg, font);
}

fn name_or<'a>(name: &'a [u8], fallback: &'a [u8]) -> &'a [u8] {
    let end = name.iter().position(|&b| !(32..=126).contains(&b)).unwrap_or(name.len());
    if end == 0 { fallback } else { &name[..end] }
}

fn number<'a>(api: &Api, value: u32, buffer: &'a mut [u8; 10]) -> &'a [u8] {
    let len = (api.fmt_u32)(value, buffer.as_mut_ptr()) as usize;
    &buffer[..len]
}

fn draw(api: &Api, s: &mut State) {
    if s.chrome {
        (api.fill_rect)(0, 0, 160, 128, pal().bg);
        (api.fill_rect)(0, 0, 160, 22, pal().card);
        let title: &[u8] = match s.page {
            Page::Activity => b"RECENT ACTIVITY",
            Page::Listen => b"LISTENING",
            _ => b"RADIO WATCH",
        };
        text(api, 5, 2, title, pal().text, pal().card, F9);
        (api.fill_rect)(0, 109, 160, 19, pal().card);
        let hint: &[u8] = match s.page {
            Page::Lists => b"MENU SCAN  # LOG  EXIT",
            Page::Scan => b"DN SKIP  # LOG  EXIT STOP",
            Page::Activity => b"MENU LISTEN    EXIT BACK",
            Page::Listen => b"EXIT BACK TO ACTIVITY",
        };
        text(api, 4, 113, hint, pal().dim, pal().card, F6);
        s.chrome = false;
    }
    (api.fill_rect)(0, 22, 160, 87, pal().bg);
    let mut buffer = [0u8; 10];
    match s.page {
        Page::Lists if s.list_count == 0 => text(api, 5, 43, b"ADD SCAN LISTS IN THE CPS", pal().dim, pal().bg, F6),
        Page::Lists => {
            for i in 0..s.list_count {
                let y = 23 + i as i16 * 12;
                let bg = if i == s.list { pal().card } else { pal().bg };
                (api.fill_rect)(0, y, 160, 12, bg);
                let list = s.list_at(i);
                text(api, 5, y + 1, name_or(&list[..NAME_BYTES], b"LIST"), pal().text, bg, F6);
                text(api, 123, y + 1, number(api, u32::from(list[NAME_BYTES]), &mut buffer), pal().accent, bg, F6);
                text(api, 141, y + 1, b"CH", pal().dim, bg, F6);
            }
            if s.empty_list {
                text(api, 5, 97, b"NO SAVED CHANNELS IN LIST", pal().accent, pal().bg, F6);
            }
        }
        Page::Scan | Page::Listen => {
            let status: &[u8] = if s.page == Page::Listen {
                b"MONITORING"
            } else if s.phase == ScanPhase::Active {
                b"SIGNAL OPEN"
            } else {
                b"SCANNING"
            };
            text(api, 5, 29, status, pal().accent, pal().bg, F6);
            if s.page == Page::Scan {
                text(api, 81, 29, name_or(&s.list_at(s.list)[..NAME_BYTES], b"LIST"), pal().dim, pal().bg, F6);
            }
            text(api, 5, 47, name_or(&s.channel_name, b"CHANNEL"), pal().text, pal().bg, F9);
            let mut frequency = [0; 16];
            let len = (api.fmt_freq)(s.hz, frequency.as_mut_ptr()) as usize;
            text(api, 5, 74, &frequency[..len], pal().text, pal().bg, F9);
            text(api, 112, 78, b"CH", pal().dim, pal().bg, F6);
            text(api, 127, 78, number(api, u32::from(s.channel), &mut buffer), pal().accent, pal().bg, F6);
        }
        Page::Activity if s.used == 0 => text(api, 5, 43, b"NO ACTIVITY YET", pal().dim, pal().bg, F6),
        Page::Activity => {
            let start = s.selected.saturating_sub(LOG_ROWS - 1);
            for (row, back) in (start..s.used.min(start + LOG_ROWS)).enumerate() {
                let hit = s.hit_at(back);
                let y = 23 + row as i16 * 14;
                let bg = if back == s.selected { pal().card } else { pal().bg };
                (api.fill_rect)(0, y, 160, 14, bg);
                let mut record = [0; 32];
                if channel_record(api, hit.channel, &mut record) {
                    text(api, 4, y + 2, name_or(&record[20..32], b"CHANNEL"), pal().text, bg, F6);
                }
                text(api, 80, y + 2, number(api, u32::from(hit.channel), &mut buffer), pal().dim, bg, F6);
                text(api, 112, y + 2, number(api, u32::from(hit.seconds), &mut buffer), pal().accent, bg, F6);
                text(api, 148, y + 2, b"s", pal().dim, bg, F6);
            }
        }
    }
    s.dirty = false;
}
