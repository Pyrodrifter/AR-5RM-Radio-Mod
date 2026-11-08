#![no_std]
#![no_main]

use bf5rh_abi::{Api, AppEvent, AppResult};
use app_theme::pal;

const KIND_SINGLE: u8 = 2;
const KIND_LONG: u8 = 3;
const KIND_REPEAT: u8 = 4;
const KEY_POUND: u8 = 11;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_VM: u8 = 16;

const FREQ_LO_DECI_MHZ: u16 = 875;
const FREQ_HI_DECI_MHZ: u16 = 1080;
const FM_DEFAULT_DECI_MHZ: u16 = 1000;

const FREQ_INPUT_DIGITS: usize = 4;
const CHANNEL_INPUT_DIGITS: usize = 2;
const CHANNEL_COUNT: usize = 30;
const CHANNEL_EMPTY: u16 = 0xFFFF;

const SEEK_TIMEOUT_100US: u32 = 30_000;
const SCAN_TUNE_SETTLE_100US: u32 = 3_000;
const SCAN_SEEK_TIMEOUT_100US: u32 = 100_000;
const SCAN_SEEK_GRACE_100US: u32 = 2_000;
const NOTICE_DURATION_100US: u32 = 40_000;
const METER_WINDOW_100US: u32 = 3_000;
const METER_HYSTERESIS: u8 = 2;
const METER_THRESHOLDS: [u8; 10] = [8, 14, 20, 27, 35, 44, 54, 65, 78, 92];

const F6: u8 = 1; // 6x10
const F9: u8 = 2; // 9x18
const FW6: i16 = 6;
const FW9: i16 = 9;

const SCREEN_W: i16 = 160;
const SCREEN_H: i16 = 128;
const RIGHT_MARGIN: i16 = 4;

const HEADER_HEIGHT: i16 = 22;
const FREQ_CLEAR_Y: i16 = 26;
const FREQ_CLEAR_H: i16 = 46;
const FREQ_TOP: i16 = 29;
const STATUS_CLEAR_Y: i16 = 73;
const STATUS_CLEAR_H: i16 = 16;
const STATUS_TOP: i16 = 76;
const FOOTER_Y: i16 = 91;
const PICKER_FOOTER_Y: i16 = 110;
const ROW_HEIGHT: i16 = 22;
const VISIBLE_ROWS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Vfo,
    Channel,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Tuning,
    Seeking { remaining_100us: u32 },
    SavePicker { selected: u8 },
    AutoScan { last: u16, remaining_100us: u32, preparing: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Notice {
    None,
    ScanDone { added: u8, stopped: bool },
    PresetsFull,
    NoPresets,
    Saved(u8),
    SaveFailed,
}

struct SignalMeter {
    weighted_sum: u32,
    elapsed_100us: u32,
    level: u8,
}

impl SignalMeter {
    const EMPTY: Self = Self {
        weighted_sum: 0,
        elapsed_100us: 0,
        level: 0,
    };

    fn update(&mut self, rssi: u8, dt_100us: u32) {
        let elapsed = dt_100us.min(METER_WINDOW_100US - self.elapsed_100us);
        self.weighted_sum += u32::from(rssi.min(127)) * elapsed;
        self.elapsed_100us += elapsed;
        if self.elapsed_100us < METER_WINDOW_100US {
            return;
        }

        // Average over a fixed interval and require a margin on either side of
        // each threshold so a reading near a boundary doesn't blink a bar.
        let average = (self.weighted_sum / METER_WINDOW_100US) as u8;
        let mut level = self.level as usize;
        while level < METER_THRESHOLDS.len()
            && average >= METER_THRESHOLDS[level] + METER_HYSTERESIS
        {
            level += 1;
        }
        while level > 0 && average < METER_THRESHOLDS[level - 1] - METER_HYSTERESIS {
            level -= 1;
        }
        self.level = level as u8;
        self.weighted_sum = 0;
        self.elapsed_100us = 0;
    }
}

struct State {
    mode: Mode,
    deci_mhz: u16,
    channel_index: u8,
    phase: Phase,
    meter: SignalMeter,
    scan_added: u8,
    notice: Notice,
    notice_remaining_100us: u32,
    channels: [u16; CHANNEL_COUNT],
    input: [u8; FREQ_INPUT_DIGITS],
    input_len: usize,
}

const EMPTY_CHANNELS: [u16; CHANNEL_COUNT] = [CHANNEL_EMPTY; CHANNEL_COUNT];

static mut STATE: State = State {
    mode: Mode::Vfo,
    deci_mhz: FM_DEFAULT_DECI_MHZ,
    channel_index: 0,
    phase: Phase::Tuning,
    meter: SignalMeter::EMPTY,
    scan_added: 0,
    notice: Notice::None,
    notice_remaining_100us: 0,
    channels: EMPTY_CHANNELS,
    input: [0u8; FREQ_INPUT_DIGITS],
    input_len: 0,
};

static mut API_PTR: *const Api = core::ptr::null();

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Tuning,
    Picker,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Snap {
    view: View,
    channel_mode: bool,
    channel_index: u8,
    input_len: usize,
    input_digits: [u8; FREQ_INPUT_DIGITS],
    deci_mhz: u16,
    status: Status,
    picker_selected: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Signal(u8),
    Seeking,
    Scanning(u8),
    Notice(Notice),
}

static mut LAST_SNAP: Option<Snap> = None;

#[no_mangle]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    app_theme::load(api);
    unsafe {
        API_PTR = api as *const Api;
    }
    match ev {
        AppEvent::Enter => enter(api),
        AppEvent::Key { id, kind } => key(api, id, kind),
        AppEvent::Tick { dt_100us } => tick(api, dt_100us),
        AppEvent::Draw => draw(api),
        AppEvent::Leave => AppResult::Continue,
    }
}

fn enter(api: &Api) -> AppResult {
    // park for fm
    (api.fm_tune_khz)(FM_DEFAULT_DECI_MHZ as u32 * 100);

    // read channels
    let mut ch = EMPTY_CHANNELS;
    let _ok = (api.fm_channels_load)(
        ch.as_mut_ptr() as *mut u8,
        (CHANNEL_COUNT * 2) as u16,
    );

    // init state
    unsafe {
        STATE = State {
            mode: Mode::Vfo,
            deci_mhz: FM_DEFAULT_DECI_MHZ,
            channel_index: 0,
            phase: Phase::Tuning,
            meter: SignalMeter::EMPTY,
            scan_added: 0,
            notice: Notice::None,
            notice_remaining_100us: 0,
            channels: ch,
            input: [0u8; FREQ_INPUT_DIGITS],
            input_len: 0,
        };
        LAST_SNAP = None;
    }

    // enable speaker
    (api.set_speaker)(true);
    AppResult::Continue
}

fn exit(api: &Api) {
    (api.fm_power_off)();
}

fn tune(api: &Api, deci_mhz: u16) {
    unsafe {
        STATE.deci_mhz = deci_mhz;
        STATE.meter = SignalMeter::EMPTY;
    }
    (api.fm_tune_khz)(deci_mhz as u32 * 100);
}

fn channels() -> [u16; CHANNEL_COUNT] {
    unsafe { STATE.channels }
}

fn show_notice(notice: Notice) {
    unsafe {
        STATE.notice = notice;
        STATE.notice_remaining_100us = NOTICE_DURATION_100US;
    }
}

fn input_value() -> u16 {
    let len = unsafe { STATE.input_len };
    let mut v = 0u16;
    for i in 0..len {
        v = v * 10 + unsafe { STATE.input[i] as u16 };
    }
    v
}

fn find_next_channel(
    channels: &[u16; CHANNEL_COUNT],
    from: u8,
    forward: bool,
) -> Option<u8> {
    let len = CHANNEL_COUNT as u8;
    let mut i = from;
    for _ in 0..len {
        i = if forward {
            if i + 1 >= len {
                0
            } else {
                i + 1
            }
        } else if i == 0 {
            len - 1
        } else {
            i - 1
        };
        if channels[i as usize] != CHANNEL_EMPTY {
            return Some(i);
        }
    }
    None
}

fn find_matching_or_first(
    channels: &[u16; CHANNEL_COUNT],
    deci_mhz: u16,
) -> Option<u8> {
    channels
        .iter()
        .position(|&f| f == deci_mhz)
        .or_else(|| channels.iter().position(|&f| f != CHANNEL_EMPTY))
        .map(|i| i as u8)
}

fn step(api: &Api, up: bool) {
    let mode = unsafe { STATE.mode };
    match mode {
        Mode::Vfo => {
            let cur = unsafe { STATE.deci_mhz };
            let next = if up {
                if cur >= FREQ_HI_DECI_MHZ {
                    FREQ_LO_DECI_MHZ
                } else {
                    cur + 1
                }
            } else if cur <= FREQ_LO_DECI_MHZ {
                FREQ_HI_DECI_MHZ
            } else {
                cur - 1
            };
            tune(api, next);
        }
        Mode::Channel => {
            let ch = channels();
            if let Some(idx) =
                find_next_channel(&ch, unsafe { STATE.channel_index }, up)
            {
                unsafe {
                    STATE.channel_index = idx;
                }
                tune(api, ch[idx as usize]);
            }
        }
    }
}

fn start_seek(api: &Api, up: bool) {
    (api.set_speaker)(false);
    (api.fm_seek)(up);
    unsafe {
        STATE.phase = Phase::Seeking {
            remaining_100us: SEEK_TIMEOUT_100US,
        };
    }
}

// Preserve existing presets and fill empty slots during one band pass.
fn start_auto_scan(api: &Api) {
    if !channels().contains(&CHANNEL_EMPTY) {
        show_notice(Notice::PresetsFull);
        return;
    }
    unsafe {
        STATE.input_len = 0;
        STATE.mode = Mode::Vfo;
        STATE.scan_added = 0;
    }
    tune(api, FREQ_LO_DECI_MHZ);
    (api.set_speaker)(false);
    unsafe { STATE.phase = Phase::AutoScan { last: FREQ_LO_DECI_MHZ, remaining_100us: SCAN_TUNE_SETTLE_100US, preparing: true }; }
}
fn finish_auto_scan(api: &Api, stopped: bool) {
    let ch = channels();
    let added = unsafe { STATE.scan_added };
    let saved = added == 0 || (api.fm_channels_save)(ch.as_ptr().cast(), (CHANNEL_COUNT * 2) as u16);
    tune(api, unsafe { STATE.deci_mhz });
    unsafe {
        STATE.phase = Phase::Tuning;
    }
    show_notice(if saved { Notice::ScanDone { added, stopped } } else { Notice::SaveFailed });
    (api.set_speaker)(true);
}

fn toggle_mode(api: &Api) {
    let mode = unsafe { STATE.mode };
    match mode {
        Mode::Vfo => {
            match find_matching_or_first(&channels(), unsafe { STATE.deci_mhz })
            {
                Some(idx) => {
                    unsafe {
                        STATE.mode = Mode::Channel;
                        STATE.channel_index = idx;
                    }
                    tune(api, channels()[idx as usize]);
                }
                None => {
                    show_notice(Notice::NoPresets);
                }
            }
        }
        Mode::Channel => unsafe {
            STATE.mode = Mode::Vfo;
        },
    }
    unsafe {
        STATE.input_len = 0;
    }
}

fn handle_digit(api: &Api, d: u8) {
    let mode = unsafe { STATE.mode };
    let max_len = if mode == Mode::Vfo {
        FREQ_INPUT_DIGITS
    } else {
        CHANNEL_INPUT_DIGITS
    };
    if unsafe { STATE.input_len } >= max_len {
        unsafe {
            STATE.input_len = 0;
        }
    }
    let len = unsafe { STATE.input_len };
    unsafe {
        STATE.input[len] = d;
        STATE.input_len = len + 1;
    }
    if unsafe { STATE.input_len } == max_len {
        commit_input(api);
    }
}

fn commit_input(api: &Api) {
    let value = input_value();
    unsafe {
        STATE.input_len = 0;
    }
    let mode = unsafe { STATE.mode };
    match mode {
        Mode::Vfo => {
            let deci = value;
            if (FREQ_LO_DECI_MHZ..=FREQ_HI_DECI_MHZ).contains(&deci) {
                tune(api, deci);
            }
            // out of bounds
            // TODO: play beep
        }
        Mode::Channel => {
            let slot = value as usize;
            let ch = channels();
            if (1..=CHANNEL_COUNT).contains(&slot)
                && ch[slot - 1] != CHANNEL_EMPTY
            {
                unsafe {
                    STATE.channel_index = (slot - 1) as u8;
                }
                tune(api, ch[slot - 1]);
            }
        }
    }
}

fn open_save_picker() {
    let selected = match unsafe { STATE.mode } {
        Mode::Channel => unsafe { STATE.channel_index },
        Mode::Vfo => channels()
            .iter()
            .position(|&f| f == unsafe { STATE.deci_mhz })
            .map(|i| i as u8)
            .unwrap_or(0),
    };
    unsafe {
        STATE.phase = Phase::SavePicker { selected };
    }
}

fn dispatch_save_picker(api: &Api, id: u8, kind: u8) {
    let selected = match unsafe { STATE.phase } {
        Phase::SavePicker { selected } => selected,
        _ => return,
    };
    let len = CHANNEL_COUNT as u8;
    match (kind, id) {
        (KIND_SINGLE, KEY_UP) | (KIND_REPEAT, KEY_UP) => {
            let next = if selected == 0 { len - 1 } else { selected - 1 };
            unsafe {
                STATE.phase = Phase::SavePicker { selected: next };
            }
        }
        (KIND_SINGLE, KEY_DOWN) | (KIND_REPEAT, KEY_DOWN) => {
            let next = if selected + 1 >= len { 0 } else { selected + 1 };
            unsafe {
                STATE.phase = Phase::SavePicker { selected: next };
            }
        }
        (KIND_SINGLE, KEY_MENU) => {
            let mut ch = channels();
            ch[selected as usize] = unsafe { STATE.deci_mhz };
            let saved = (api.fm_channels_save)(
                ch.as_ptr() as *const u8,
                (CHANNEL_COUNT * 2) as u16,
            );
            unsafe {
                if saved {
                    STATE.channels = ch;
                }
                STATE.phase = Phase::Tuning;
            }
            show_notice(if saved { Notice::Saved(selected + 1) } else { Notice::SaveFailed });
        }
        (KIND_SINGLE, KEY_EXIT) | (KIND_LONG, KEY_EXIT) => unsafe {
            STATE.phase = Phase::Tuning;
        },
        _ => {}
    }
}

fn key(api: &Api, id: u8, kind: u8) -> AppResult {
    let phase = unsafe { STATE.phase };
    match phase {
        Phase::SavePicker { .. } => {
            dispatch_save_picker(api, id, kind);
            AppResult::Continue
        }
        Phase::AutoScan { .. } => {
            if (kind == KIND_SINGLE || kind == KIND_LONG) && id == KEY_EXIT {
                finish_auto_scan(api, true);
            }
            AppResult::Continue
        }
        _ => {
            if (kind == KIND_SINGLE || kind == KIND_LONG) && id == KEY_EXIT {
                if unsafe { STATE.input_len } > 0 {
                    unsafe {
                        STATE.input_len = 0;
                    }
                } else {
                    exit(api);
                    return AppResult::Exit;
                }
                return AppResult::Continue;
            }
            if phase == Phase::Tuning {
                if matches!(kind, KIND_SINGLE | KIND_LONG | KIND_REPEAT) {
                    unsafe { STATE.notice = Notice::None; }
                }
                if kind == KIND_SINGLE && id <= 9 {
                    handle_digit(api, id);
                } else {
                    match (kind, id) {
                        (KIND_SINGLE, KEY_VM) => toggle_mode(api),
                        (KIND_SINGLE, KEY_UP) | (KIND_REPEAT, KEY_UP) => {
                            step(api, true)
                        }
                        (KIND_SINGLE, KEY_DOWN) | (KIND_REPEAT, KEY_DOWN) => {
                            step(api, false)
                        }
                        (KIND_LONG, KEY_UP) => start_seek(api, true),
                        (KIND_LONG, KEY_DOWN) => start_seek(api, false),
                        (KIND_SINGLE, KEY_MENU) => open_save_picker(),
                        (KIND_LONG, KEY_POUND) => start_auto_scan(api),
                        _ => {}
                    }
                }
            }
            if unsafe { STATE.phase } == Phase::Tuning {
                (api.set_speaker)(true);
            }
            AppResult::Continue
        }
    }
}

fn tick(api: &Api, dt_100us: u32) -> AppResult {
    let status = (api.fm_status)();
    unsafe {
        if STATE.phase == Phase::Tuning {
            (*core::ptr::addr_of_mut!(STATE.meter)).update((status >> 8) as u8, dt_100us);
        }
        STATE.notice_remaining_100us = STATE.notice_remaining_100us.saturating_sub(dt_100us);
        if STATE.notice_remaining_100us == 0 {
            STATE.notice = Notice::None;
        }
    }
    if let Phase::AutoScan { last, remaining_100us, preparing } = unsafe { STATE.phase } {
        let remaining = remaining_100us.saturating_sub(dt_100us);
        if preparing {
            if remaining == 0 {
                (api.fm_seek)(true);
                unsafe { STATE.phase = Phase::AutoScan { last, remaining_100us: SCAN_SEEK_TIMEOUT_100US, preparing: false }; }
            } else {
                unsafe { STATE.phase = Phase::AutoScan { last, remaining_100us: remaining, preparing: true }; }
            }
        } else if remaining == 0 {
            finish_auto_scan(api, false);
        } else if remaining <= SCAN_SEEK_TIMEOUT_100US - SCAN_SEEK_GRACE_100US && status & 1 != 0 {
            if status & 2 != 0 {
                finish_auto_scan(api, false);
            } else {
                let found = ((api.fm_tuned_freq_khz)() / 100) as u16;
                if found <= last || found > FREQ_HI_DECI_MHZ {
                    finish_auto_scan(api, false);
                } else {
                    unsafe { STATE.deci_mhz = found; }
                    let mut ch = channels();
                    if !ch.contains(&found) {
                        if let Some(slot) = ch.iter().position(|&f| f == CHANNEL_EMPTY) {
                            ch[slot] = found;
                            unsafe {
                                STATE.channels = ch;
                                STATE.scan_added += 1;
                            }
                        }
                    }
                    if ch.iter().all(|&f| f != CHANNEL_EMPTY) {
                        finish_auto_scan(api, false);
                    } else {
                        // Retuning clears the previous seek; wait for tuning to settle.
                        tune(api, found);
                        unsafe { STATE.phase = Phase::AutoScan { last: found, remaining_100us: SCAN_TUNE_SETTLE_100US, preparing: true }; }
                    }
                }
            }
        } else {
            unsafe { STATE.phase = Phase::AutoScan { last, remaining_100us: remaining, preparing: false }; }
        }
        return AppResult::Continue;
    }
    if let Phase::Seeking { remaining_100us } = unsafe { STATE.phase } {
        let complete = status & 1 != 0;
        let seek_failed = status & 2 != 0;
        let remaining_100us = remaining_100us.saturating_sub(dt_100us);
        if !complete && !seek_failed && remaining_100us > 0 {
            unsafe {
                STATE.phase = Phase::Seeking { remaining_100us };
            }
        } else {
            if !seek_failed {
                let khz = (api.fm_tuned_freq_khz)();
                let deci = ((khz / 100) as u16)
                    .clamp(FREQ_LO_DECI_MHZ, FREQ_HI_DECI_MHZ);
                unsafe {
                    STATE.deci_mhz = deci;
                }
            }
            unsafe {
                STATE.phase = Phase::Tuning;
                STATE.meter = SignalMeter::EMPTY;
            }
            (api.set_speaker)(true);
        }
    }
    AppResult::Continue
}

// UI

fn clear_strip(api: &Api, y: i16, h: i16) {
    (api.fill_rect)(0, y, SCREEN_W as u16, h as u16, pal().bg);
}

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16, font: u8) {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, font);
}

fn fmt_deci(api: &Api, deci: u16, out: &mut [u8; 8]) -> usize {
    let mhz = (deci / 10) as u32;
    let tenth = (deci % 10) as u32;
    let n1 = (api.fmt_u32)(mhz, out.as_mut_ptr()) as usize;
    out[n1] = b'.';
    let n2 =
        (api.fmt_u32)(tenth, unsafe { out.as_mut_ptr().add(n1 + 1) }) as usize;
    n1 + 1 + n2
}

fn capture() -> Snap {
    unsafe {
        let (view, picker_selected) = match STATE.phase {
            Phase::SavePicker { selected } => (View::Picker, selected),
            _ => (View::Tuning, 0),
        };
        Snap {
            view,
            channel_mode: STATE.mode == Mode::Channel,
            channel_index: STATE.channel_index,
            input_len: STATE.input_len,
            input_digits: STATE.input,
            deci_mhz: STATE.deci_mhz,
            status: match STATE.phase {
                Phase::Seeking { .. } => Status::Seeking,
                Phase::AutoScan { .. } => Status::Scanning(STATE.scan_added),
                _ if STATE.notice != Notice::None => Status::Notice(STATE.notice),
                _ => Status::Signal(STATE.meter.level),
            },
            picker_selected,
        }
    }
}

fn draw(api: &Api) -> AppResult {
    let snap = capture();
    let prev = unsafe { LAST_SNAP };
    let first = prev.is_none();

    match snap.view {
        View::Tuning => {
            if first || prev.unwrap().view != View::Tuning {
                full_redraw_tuning(api, &snap);
            } else {
                let p = prev.unwrap();
                if (p.channel_mode, p.channel_index)
                    != (snap.channel_mode, snap.channel_index)
                {
                    draw_header(api, &snap);
                }
                if p.channel_mode != snap.channel_mode
                    || p.input_len != snap.input_len
                    || p.input_digits != snap.input_digits
                    || p.deci_mhz != snap.deci_mhz
                {
                    clear_strip(api, FREQ_CLEAR_Y, FREQ_CLEAR_H);
                    draw_freq(api, &snap);
                }
                if p.status != snap.status {
                    match (p.status, snap.status) {
                        (Status::Signal(old), Status::Signal(new)) => {
                            draw_signal_bars(api, new, old.min(new)..old.max(new));
                        }
                        _ => draw_status(api, &snap),
                    }
                }
                if p.channel_mode != snap.channel_mode
                    || matches!(p.status, Status::Scanning(_)) != matches!(snap.status, Status::Scanning(_))
                {
                    draw_footer(api, &snap);
                }
            }
        }
        View::Picker => {
            if first || prev.unwrap().view != View::Picker {
                full_redraw_picker(api, &snap);
            } else if prev.unwrap().picker_selected != snap.picker_selected {
                draw_picker(api, &snap);
            }
        }
    }

    unsafe {
        LAST_SNAP = Some(snap);
    }
    AppResult::Continue
}

fn full_redraw_tuning(api: &Api, snap: &Snap) {
    clear_strip(api, 0, SCREEN_H);
    draw_header(api, snap);
    draw_freq(api, snap);
    draw_status(api, snap);
    draw_footer(api, snap);
}

fn draw_header(api: &Api, snap: &Snap) {
    (api.fill_rect)(0, 0, SCREEN_W as u16, HEADER_HEIGHT as u16, pal().card);
    text(api, 5, 2, b"FM RADIO", pal().text, pal().card, F9);
    if snap.channel_mode {
        let mut label = *b"PRESET 00";
        label[7..].copy_from_slice(&two_digits(snap.channel_index + 1));
        text(api, 101, 6, &label, pal().accent, pal().card, F6);
    } else {
        text(api, 119, 6, b"MANUAL", pal().accent, pal().card, F6);
    }
}

fn two_digits(value: u8) -> [u8; 2] {
    [b'0' + value / 10, b'0' + value % 10]
}

fn draw_footer(api: &Api, snap: &Snap) {
    (api.fill_rect)(0, FOOTER_Y, SCREEN_W as u16, (SCREEN_H - FOOTER_Y) as u16, pal().card);
    if matches!(snap.status, Status::Scanning(_)) {
        text(api, 5, FOOTER_Y + 7, b"SCAN ADDS TO EMPTY SLOTS", pal().dim, pal().card, F6);
        text(api, 5, FOOTER_Y + 21, b"EXIT  STOP AND SAVE", pal().text, pal().card, F6);
    } else {
        let arrows = if snap.channel_mode { b"UP/DN PRESET" } else { b"UP/DN TUNE  " };
        let mode = if snap.channel_mode { b"V/M MANUAL " } else { b"V/M PRESETS" };
        text(api, 5, FOOTER_Y + 2, arrows, pal().text, pal().card, F6);
        text(api, 89, FOOTER_Y + 2, b"HOLD SEEK", pal().dim, pal().card, F6);
        text(api, 5, FOOTER_Y + 14, b"MENU SAVE", pal().text, pal().card, F6);
        text(api, 89, FOOTER_Y + 14, b"HOLD # SCAN", pal().dim, pal().card, F6);
        text(api, 5, FOOTER_Y + 26, mode, pal().text, pal().card, F6);
        text(api, 89, FOOTER_Y + 26, b"EXIT CLOSE", pal().dim, pal().card, F6);
    }
}

// Large digits drawn at four times their 5x7 pixel size.
fn draw_big_frequency(api: &Api, value: &[u8]) {
    const DIGITS: [[u8; 7]; 10] = [
        [14,17,17,17,17,17,14], [4,12,4,4,4,4,14],
        [14,17,1,2,4,8,31], [30,1,1,14,1,1,30],
        [2,6,10,18,31,2,2], [31,16,16,30,1,1,30],
        [14,16,16,30,17,17,14], [31,1,2,4,8,8,8],
        [14,17,17,14,17,17,14], [14,17,17,15,1,1,14],
    ];
    let width: i16 = value.iter().map(|&c| if c == b'.' { 8 } else { 23 }).sum();
    let mut x = (SCREEN_W - width - 18) / 2;
    for &c in value {
        if c == b'.' {
            (api.fill_rect)(x + 2, FREQ_TOP + 24, 4, 4, pal().accent);
            x += 8;
            continue;
        }
        let blank = [0u8; 7];
        let dash = [0,0,0,31,0,0,0];
        let rows = if c.is_ascii_digit() { &DIGITS[(c - b'0') as usize] }
            else if c == b'-' { &dash } else { &blank };
        for (row, &bits) in rows.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    (api.fill_rect)(x + col * 4, FREQ_TOP + row as i16 * 4, 4, 4, pal().text);
                }
            }
        }
        x += 23;
    }
}

fn draw_freq(api: &Api, snap: &Snap) {
    if snap.input_len > 0 {
        draw_freq_input(api, snap);
    } else {
        let mut buf = [0u8; 8];
        let n = fmt_deci(api, snap.deci_mhz, &mut buf);
        draw_big_frequency(api, &buf[..n]);
    }
    let unit = if snap.channel_mode && snap.input_len > 0 { b"CH " } else { b"MHz" };
    text(api, 135, 47, unit, pal().dim, pal().bg, F6);
    // A dial marker shows the tuned frequency within the broadcast band.
    let offset = snap.deci_mhz.clamp(FREQ_LO_DECI_MHZ, FREQ_HI_DECI_MHZ) - FREQ_LO_DECI_MHZ;
    let marker = 8 + (offset * 143 / (FREQ_HI_DECI_MHZ - FREQ_LO_DECI_MHZ)) as i16;
    (api.fill_rect)(8, 65, 144, 2, pal().card_hi);
    for x in [8, 44, 80, 116, 151] {
        (api.fill_rect)(x, 67, 1, 3, pal().card_hi);
    }
    (api.fill_rect)(marker - 1, 62, 3, 8, pal().accent);
}

fn draw_freq_input(api: &Api, snap: &Snap) {
    let mut buf = [0u8; 8];
    let mut n = 0usize;
    let len = snap.input_len;
    if snap.channel_mode {
        for pos in 0..CHANNEL_INPUT_DIGITS {
            buf[n] = if pos < len {
                b'0' + snap.input_digits[pos]
            } else {
                b'-'
            };
            n += 1;
        }
    } else {
        for pos in 0..FREQ_INPUT_DIGITS {
            if pos == 3 {
                buf[n] = b'.';
                n += 1;
            }
            buf[n] = if pos < len {
                b'0' + snap.input_digits[pos]
            } else {
                b'-'
            };
            n += 1;
        }
    }
    draw_big_frequency(api, &buf[..n]);
}

fn draw_status(api: &Api, snap: &Snap) {
    clear_strip(api, STATUS_CLEAR_Y, STATUS_CLEAR_H);
    let mut label = *b"+00 ADDED";
    let message: &[u8] = match snap.status {
        Status::Signal(level) => {
            text(api, 5, STATUS_TOP, b"SIGNAL", pal().dim, pal().bg, F6);
            draw_signal_bars(api, level, 0..METER_THRESHOLDS.len() as u8);
            return;
        }
        Status::Seeking => b"SEEKING...",
        Status::Scanning(added) => {
            label[1..3].copy_from_slice(&two_digits(added));
            text(api, 101, STATUS_TOP, &label, pal().text, pal().bg, F6);
            b"SCANNING..."
        }
        Status::Notice(Notice::ScanDone { added, stopped }) => {
            label[1..3].copy_from_slice(&two_digits(added));
            text(api, 101, STATUS_TOP, &label, pal().text, pal().bg, F6);
            if stopped { b"SCAN STOPPED" } else { b"SCAN DONE" }
        }
        Status::Notice(Notice::PresetsFull) => b"PRESETS FULL (30/30)",
        Status::Notice(Notice::NoPresets) => b"NO PRESETS. HOLD # TO SCAN",
        Status::Notice(Notice::Saved(slot)) => {
            let mut saved = *b"SAVED TO PRESET 00";
            saved[16..].copy_from_slice(&two_digits(slot));
            text(api, 5, STATUS_TOP, &saved, pal().accent, pal().bg, F6);
            return;
        }
        Status::Notice(Notice::SaveFailed) => b"COULD NOT SAVE PRESETS",
        Status::Notice(Notice::None) => return,
    };
    text(api, 5, STATUS_TOP, message, pal().accent, pal().bg, F6);
}

fn draw_signal_bars(api: &Api, level: u8, bars: core::ops::Range<u8>) {
    for i in bars {
        let color = if i < level { pal().accent } else { pal().card_hi };
        (api.fill_rect)(56 + i16::from(i) * 10, STATUS_TOP + 2, 8, 6, color);
    }
}

fn full_redraw_picker(api: &Api, snap: &Snap) {
    clear_strip(api, 0, SCREEN_H);
    (api.fill_rect)(0, 0, SCREEN_W as u16, HEADER_HEIGHT as u16, pal().card);
    text(api, 5, 2, b"SAVE PRESET", pal().text, pal().card, F9);
    let mut freq = [0u8; 8];
    let len = fmt_deci(api, snap.deci_mhz, &mut freq);
    text(api, SCREEN_W - RIGHT_MARGIN - len as i16 * FW6, 6, &freq[..len], pal().accent, pal().card, F6);
    (api.fill_rect)(0, PICKER_FOOTER_Y, SCREEN_W as u16, (SCREEN_H - PICKER_FOOTER_Y) as u16, pal().card);
    text(api, 5, PICKER_FOOTER_Y + 4, b"MENU SAVE     EXIT BACK", pal().text, pal().card, F6);
    draw_picker(api, snap);
}

fn draw_picker(api: &Api, snap: &Snap) {
    let selected = snap.picker_selected as usize;
    let top = scroll_top(selected);
    for slot in 0..VISIBLE_ROWS {
        let index = top + slot;
        let row_top = HEADER_HEIGHT + slot as i16 * ROW_HEIGHT;
        draw_picker_row(api, index, selected, row_top);
    }
}

fn scroll_top(selected: usize) -> usize {
    if CHANNEL_COUNT <= VISIBLE_ROWS {
        return 0;
    }
    let half = VISIBLE_ROWS / 2;
    selected
        .saturating_sub(half)
        .min(CHANNEL_COUNT - VISIBLE_ROWS)
}

fn draw_picker_row(api: &Api, index: usize, selected: usize, row_top: i16) {
    let is_sel = index == selected;
    let bg = if is_sel { pal().card } else { pal().bg };
    let fg = if is_sel { pal().accent } else { pal().text };
    (api.fill_rect)(0, row_top, SCREEN_W as u16, ROW_HEIGHT as u16, bg);
    if is_sel {
        (api.fill_rect)(0, row_top, 2, ROW_HEIGHT as u16, pal().accent);
    }
    let text_top = row_top + 2;

    let mut label = *b"PRESET 00";
    label[7..].copy_from_slice(&two_digits(index as u8 + 1));
    text(api, 6, text_top, &label, fg, bg, F9);

    let deci = channels()[index];
    if deci == CHANNEL_EMPTY {
        text(
            api,
            SCREEN_W - RIGHT_MARGIN - 5 * FW9,
            text_top,
            b"EMPTY",
            if is_sel { pal().accent } else { pal().dim },
            bg,
            F9,
        );
    } else {
        let mut buf = [0u8; 8];
        let m = fmt_deci(api, deci, &mut buf);
        let w = m as i16 * FW9;
        text(
            api,
            SCREEN_W - RIGHT_MARGIN - w,
            text_top,
            &buf[..m],
            fg,
            bg,
            F9,
        );
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        let ptr = API_PTR;
        if !ptr.is_null() {
            ((*ptr).app_fault)(0);
        }
    }
    loop {}
}
