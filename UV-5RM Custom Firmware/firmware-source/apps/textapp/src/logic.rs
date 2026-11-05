//! TEXT: short messages between radios using the RF chip's built-in FSK modem.
//!
//! The firmware gives apps no modem calls, so this app talks to the chip itself
//! over the same three GPIO pins the firmware's driver uses. Apps and the main
//! loop run on one thread and no interrupt touches this bus, so they can't collide.
//! Modem settings follow the UV-K5 community messenger for the related BK4819.
//!
//! Messages are saved in the external flash, in the app data area, and stay until
//! they're overwritten or erased. Sending is call and response: a message repeats until the other radio confirms it, up to eight tries,
//! so a radio with this app open answers messages by transmitting on its own. The
//! radios are assumed to be a pair; with more, the replies would collide.

use bf5rh_abi::{Api, AppEvent, AppResult};

#[cfg(target_os = "none")]
mod bus {
    use core::ptr::{read_volatile, write_volatile};

    const A: usize = 0x4800_0000;
    const C: usize = 0x4800_0800;
    const CFGR: usize = 0x00;
    const PULL: usize = 0x0C;
    const IDT: usize = 0x10;
    const SCR: usize = 0x18;
    const CLR: usize = 0x28;
    const LE: u32 = 1 << 13; // PC13
    const SCK: u32 = 1 << 2; // PA2
    const SDA: u32 = 1 << 3; // PA3
    const SDA_FIELD: u32 = 3 << 6;

    fn set(port: usize, bit: u32, high: bool) {
        let reg = port + if high { SCR } else { CLR };
        unsafe { write_volatile(reg as *mut u32, bit) };
    }

    /// At least the driver's 1 us at 120 MHz.
    fn pause() {
        for _ in 0..40 {
            unsafe { core::arch::asm!("nop") };
        }
    }

    fn bits(byte: u8) {
        let mut mask = 0x80u8;
        while mask != 0 {
            set(A, SCK, false);
            set(A, SDA, byte & mask != 0);
            pause();
            set(A, SCK, true);
            pause();
            mask >>= 1;
        }
    }

    fn sda_output(output: bool) {
        unsafe {
            let cfg = read_volatile((A + CFGR) as *const u32) & !SDA_FIELD;
            write_volatile((A + CFGR) as *mut u32, cfg | if output { 1 << 6 } else { 0 });
            let pull = read_volatile((A + PULL) as *const u32) & !SDA_FIELD;
            write_volatile((A + PULL) as *mut u32, pull | if output { 0 } else { 1 << 6 });
        }
    }

    pub fn write(addr: u8, value: u16) {
        set(C, LE, false);
        pause();
        bits(addr & 0x7F);
        bits((value >> 8) as u8);
        bits(value as u8);
        set(C, LE, true);
        pause();
        set(A, SCK, false);
    }

    pub fn read(addr: u8) -> u16 {
        set(C, LE, false);
        pause();
        bits(addr | 0x80);
        set(A, SCK, false);
        pause();
        sda_output(false);
        pause();
        let mut data = 0u16;
        for _ in 0..16 {
            data <<= 1;
            set(A, SCK, true);
            if unsafe { read_volatile((A + IDT) as *const u32) } & SDA != 0 {
                data |= 1;
            }
            pause();
            set(A, SCK, false);
            pause();
        }
        sda_output(true);
        set(C, LE, true);
        pause();
        set(A, SDA, false);
        set(A, SCK, false);
        data
    }
}

#[cfg(not(target_os = "none"))]
mod bus {
    pub fn write(addr: u8, value: u16) {
        crate::sim_rf_write(addr, value);
    }
    pub fn read(addr: u8) -> u16 {
        crate::sim_rf_read(addr)
    }
}

const KIND_SINGLE: u8 = 2;
const KIND_LONG: u8 = 3;
const KEY_STAR: u8 = 10;
const KEY_POUND: u8 = 11;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_AB: u8 = 17;
const NO_KEY: u8 = 0xFF;

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const GREEN: u16 = 0x07E0;
const HEADER: u16 = 0x6000;
const HILITE: u16 = 0xA000;
const AMBER: u16 = 0xFD20;

const F5: u8 = 0; // 5x8
const F6: u8 = 1; // 6x10
const SCREEN_W: u16 = 160;
const COLS: usize = 26;

const TITLE_Y: i16 = 0;
const KEYS_Y: i16 = 13;
const MSG_Y: i16 = 30;
const MSG_ROWS: usize = 6;
const COMP_Y: i16 = 97;
const HINT_Y: i16 = 119;

// Chip registers.
const R_INT: u8 = 0x02;
const R_STATUS: u8 = 0x0C;
const R_FILTER: u8 = 0x2B;
const R_STATE: u8 = 0x30;
const R_IRQ_EN: u8 = 0x3F;
const R_DEV: u8 = 0x40;
const R_AF: u8 = 0x47;
const R_VOLUME: u8 = 0x48;
const R_FSK: u8 = 0x58;
const R_CTRL: u8 = 0x59;
const R_SYNC1: u8 = 0x5A;
const R_SYNC2: u8 = 0x5B;
const R_CRC: u8 = 0x5C;
const R_SIZE: u8 = 0x5D;
const R_THRESH: u8 = 0x5E;
const R_FIFO: u8 = 0x5F;
const R_GAIN: u8 = 0x70;
const R_TONE: u8 = 0x71;
const R_BAUD: u8 = 0x72;

const IRQ_TX_DONE: u16 = 1 << 15;
const IRQ_RX_DONE: u16 = 1 << 13;
const IRQ_WORD: u16 = 1 << 12;
const IRQ_SYNC: u16 = 1 << 1;

const CTRL_CLEAR: u16 = (1 << 15) | (1 << 14);
const CTRL_RX_ON: u16 = 1 << 12;
const CTRL_TX_ON: u16 = 1 << 11;
const CTRL_RX: u16 = 1 << 3; // 4-byte sync
const CTRL_TX: u16 = (15 << 4) | CTRL_RX; // plus the longest preamble

const SAVED_REGS: [u8; 10] = [
    R_FSK, R_CTRL, R_SYNC1, R_SYNC2, R_CRC, R_SIZE, R_THRESH, R_GAIN, R_BAUD, R_FILTER,
];

/// The setting measured best on the air, a few feet apart: direct FM at 450 baud with a
/// 4-byte sync and 30 ms of carrier first got 9 of 10 sent 1.5 s apart and about half
/// sent 6 s apart, with no bad packets. A 400 ms lead got none, and 1200 baud and FFSK
/// got none. Why the lead and the gap matter is unknown, so misses are expected and
/// the protocol below retries until the other radio confirms.
const FSK: u16 = 0x0301;
const BAUD: u16 = 4646;
const LEAD_MS: u32 = 30;
const NEXT_PACKET_LEAD_MS: u32 = 20;
const TAIL_MS: u32 = 100;

const PACKET: usize = 32;
const RX_SIZE: u16 = 34; // from the K5 messenger; on this chip the done flag never comes
const CHARS: usize = 25;
/// Bytes 30 and 31 are filler, in case the very end of a packet gets clipped.
const CHECK_AT: usize = 4 + CHARS;
const MAGIC: u8 = 0xB5;
const MAX_LEN: usize = 2 * CHARS; // two packets on the air, two lines on screen

const TAP_100US: u32 = 10_000;
const NOTICE_100US: u32 = 25_000;
/// How long to stay with a packet once its sync arrives, in 2 ms steps.
const RX_WAIT_STEPS: u16 = 350;

// Call and response. A message goes out, then the sender listens. The receiver replies
// once it has the whole message, and again to any repeat, since a repeat means its last
// reply was missed. A packet takes about a second on the air at 450 baud.
const MAX_TRIES: u8 = 8;
const PACKET_100US: u32 = 10_000;
/// The least a reply waits after the packet that earned it.
const REPLY_DELAY_100US: u32 = 2_500;
/// A sender stays keyed after its data: 100 ms of tail here, plus 300 ms when the radio's
/// STE setting is on, as it is by default, plus any repeater-tail or roger setting. So
/// nothing goes out until the channel has been quiet this long, which also gives a
/// sender time to start listening. A carrier already up when a receiver starts listening
/// decoded badly in testing.
const QUIET_100US: u32 = 1_500;
/// RSSI reading that counts as someone on the channel. Idle reads about -110 dBm here and
/// the other radio about -70 a few feet away; this is about -98. A weaker signal goes
/// unnoticed, which at worst means the collision this is meant to avoid.
const BUSY_RAW: u16 = 72;
/// If the channel stays busy this long, a reply is dropped, since the sender will ask
/// again, and a retry goes anyway, so a stuck carrier can't hold the tries up forever.
const REPLY_GIVE_UP_100US: u32 = 30_000;
const RETRY_GIVE_UP_100US: u32 = 50_000;
/// After a transmission ends: the reply's delay, a packet on the air and some margin.
const REPLY_WAIT_100US: u32 = 17_000;
/// Up to this much more at random, so two radios can't keep colliding in step.
const REPLY_JITTER_MASK: u32 = 0x0FFF;
/// Parts of a long message wait this long for the rest. Retries can take most of it.
const JOIN_100US: u32 = 400_000;

// Flash can clear bits without erasing, so a record's status changes in place: from
// sending to delivered or failed only ever clears bits.
const STATUS_RECEIVED: u8 = 0;
const STATUS_SENDING: u8 = 3;
const STATUS_DELIVERED: u8 = 1;
const STATUS_FAILED: u8 = 2;

// The message log: a ring of records past the ninth app slot, in the data area apps may
// write. Each sector erases as the ring reaches it, losing its 64 oldest messages.
const LOG_ADDR: u32 = 0x1F_0000;
const LOG_SLOTS: u16 = 256;
const LOG_PER_SECTOR: u16 = 64;
const LOG_SECTORS: u32 = 4;
const SECTOR_BYTES: u32 = 4096;
const REC_BYTES: usize = 64;
const REC_MAGIC: u8 = 0xA7;
const REC_HEAD: usize = 6; // magic, status, id, length, sequence (2 bytes)

// Tones go to the local speaker on the same path as the firmware's own key beep: the
// chip's tone state, which never keys the transmitter. Values from its driver.
const STATE_TONE: u16 = 0x0302;
const AF_BEEP: u16 = 0x6340;
const VOLUME_BEEP: u16 = 0xB8A2;
const TONE_GAIN: u16 = 0x8000 | (60 << 8);
const TONE_MS: u32 = 110;
const CHIME_NEW: u8 = 1;
const CHIME_DELIVERED: u8 = 2;

const TAPS: [&[u8]; 10] = [
    b" 0", b".,?!-/1", b"ABC2", b"DEF3", b"GHI4", b"JKL5", b"MNO6", b"PQRS7", b"TUV8", b"WXYZ9",
];

#[derive(PartialEq)]
enum Tx {
    Sent,
    Blocked,
    Stuck,
}

struct State {
    redraw_all: bool,
    redraw_msgs: bool,
    redraw_comp: bool,
    redraw_hint: bool,
    armed: bool,
    high_power: bool,
    hz: u32,
    wide: bool,
    me: u8,
    next_id: u8,
    pings: u16,
    saved: [u16; SAVED_REGS.len()],
    comp: [u8; MAX_LEN],
    comp_len: u8,
    tap_key: u8,
    tap_index: u8,
    tap_left: u32,
    log_head: u16,
    log_count: u16,
    log_seq: u16,
    scroll: u16,
    out_slot: u16,
    chime_due: u8,
    erase_armed: bool,
    rx_buf: [u8; PACKET],
    rx_i: u8,
    rx_active: bool,
    join: [u8; 2 * CHARS],
    join_have: u8,
    join_total: u8,
    join_from: u8,
    join_id: u8,
    join_age: u32,
    join_active: bool,
    last_from: u8,
    last_id: u8,
    has_last: bool,
    out: [u8; MAX_LEN],
    out_len: u8,
    out_id: u8,
    pending: bool,
    tries: u8,
    retry_at: u32,
    last_busy: u32,
    reply_due: bool,
    reply_at: u32,
    reply_to: u8,
    reply_id: u8,
    #[cfg(not(target_os = "none"))]
    n_sync: u16,
    #[cfg(not(target_os = "none"))]
    n_ok: u16,
    #[cfg(not(target_os = "none"))]
    n_bad: u16,
    #[cfg(not(target_os = "none"))]
    n_tx: u16,
    #[cfg(not(target_os = "none"))]
    n_got: u16,
    notice: Option<&'static [u8]>,
    notice_left: u32,
}

/// For the PC simulator's tests; the radio build doesn't keep these.
macro_rules! tally {
    ($field:expr) => {
        #[cfg(not(target_os = "none"))]
        {
            $field = $field.wrapping_add(1);
        }
    };
}

const ZEROED: State = State {
    redraw_all: false,
    redraw_msgs: false,
    redraw_comp: false,
    redraw_hint: false,
    armed: false,
    high_power: false,
    hz: 0,
    wide: false,
    me: 0,
    next_id: 0,
    pings: 0,
    saved: [0; SAVED_REGS.len()],
    comp: [0; MAX_LEN],
    comp_len: 0,
    tap_key: 0,
    tap_index: 0,
    tap_left: 0,
    log_head: 0,
    log_count: 0,
    log_seq: 0,
    scroll: 0,
    out_slot: 0,
    chime_due: 0,
    erase_armed: false,
    rx_buf: [0; PACKET],
    rx_i: 0,
    rx_active: false,
    join: [0; 2 * CHARS],
    join_have: 0,
    join_total: 0,
    join_from: 0,
    join_id: 0,
    join_age: 0,
    join_active: false,
    last_from: 0,
    last_id: 0,
    has_last: false,
    out: [0; MAX_LEN],
    out_len: 0,
    out_id: 0,
    pending: false,
    tries: 0,
    retry_at: 0,
    last_busy: 0,
    reply_due: false,
    reply_at: 0,
    reply_to: 0,
    reply_id: 0,
    #[cfg(not(target_os = "none"))]
    n_sync: 0,
    #[cfg(not(target_os = "none"))]
    n_ok: 0,
    #[cfg(not(target_os = "none"))]
    n_bad: 0,
    #[cfg(not(target_os = "none"))]
    n_tx: 0,
    #[cfg(not(target_os = "none"))]
    n_got: 0,
    notice: None,
    notice_left: 0,
};

static mut ST: State = ZEROED;

fn st() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(ST) }
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    match ev {
        AppEvent::Enter => enter(api),
        AppEvent::Key { id, kind } => key(api, id, kind),
        AppEvent::Tick { dt_100us } => tick(api, dt_100us),
        AppEvent::Draw => draw(api),
        AppEvent::Leave => {
            modem_off(st());
            AppResult::Continue
        }
    }
}

fn enter(api: &Api) -> AppResult {
    let s = st();
    // The loader zeroes this on every launch; the PC simulator reopens without it.
    #[cfg(not(target_os = "none"))]
    {
        *s = ZEROED;
    }
    s.redraw_all = true;
    s.tap_key = NO_KEY;
    s.hz = (api.get_master_freq)();
    s.wide = (api.get_master_wide)();
    (api.set_tx_enabled)(false);
    (api.set_modulation)(0);
    (api.set_rx_freq)(s.hz);
    (api.set_tx_freq)(s.hz);
    (api.set_bandwidth)(s.wide);
    (api.set_subaudio_rx)(0);
    (api.set_subaudio_tx)(0);
    (api.set_sql_level)((api.settings_get)(0) as u8);
    (api.enter_rx)();
    // The modem works with the speaker muted, so the data never buzzes.
    (api.set_holds_rx)(false);
    (api.set_af_out)(false);
    (api.set_backlight_hold)(true);

    let t = (api.uptime_100us)();
    s.last_busy = t;
    s.me = (t ^ (t >> 8) ^ (t >> 16)) as u8;
    s.next_id = (t >> 3) as u8;
    for (slot, &reg) in s.saved.iter_mut().zip(SAVED_REGS.iter()) {
        *slot = bus::read(reg);
    }
    s.armed = true;
    modem_rx(s);
    log_open(api, s);
    AppResult::Continue
}

/// Puts the chip back as found. The firmware re-enters normal RX after the app exits.
fn modem_off(s: &mut State) {
    if !s.armed {
        return;
    }
    s.armed = false;
    bus::write(R_IRQ_EN, 0);
    bus::write(R_CTRL, CTRL_CLEAR);
    for (&value, &reg) in s.saved.iter().zip(SAVED_REGS.iter()) {
        bus::write(reg, if reg == R_CTRL { value & 0x07FF } else { value });
    }
    bus::write(R_INT, 0);
}

fn modem_common(size: u16) {
    bus::write(R_GAIN, (1 << 7) | 96);
    bus::write(R_BAUD, BAUD);
    bus::write(R_FSK, FSK);
    bus::write(R_SYNC1, 0x3072);
    bus::write(R_SYNC2, 0x576C);
    bus::write(R_CRC, 0x5625); // no chip CRC; packets carry their own checksum
    bus::write(R_SIZE, size << 8);
}

fn rx_restart() {
    bus::write(R_CTRL, CTRL_RX | CTRL_CLEAR);
    bus::write(R_CTRL, CTRL_RX);
    bus::write(R_INT, 0);
    bus::write(R_CTRL, CTRL_RX | CTRL_RX_ON);
}

fn modem_rx(s: &mut State) {
    modem_common(RX_SIZE);
    bus::write(R_THRESH, (64 << 3) | 1);
    bus::write(R_IRQ_EN, IRQ_RX_DONE | IRQ_WORD | IRQ_SYNC);
    s.rx_active = false;
    rx_restart();
}

fn checksum(p: &[u8]) -> u8 {
    p[..CHECK_AT].iter().fold(0x5Au8, |a, &b| a.wrapping_add(b))
}

/// Direct FM has no tones to keep time by, and short messages are mostly zeros: a
/// long run of the same bit loses the receiver's timing. XOR with a fixed
/// pseudo-random sequence on both ends. Applying it twice gets the original back.
fn scramble(p: &mut [u8; PACKET]) {
    let mut lfsr = 0xA5u8;
    for b in p.iter_mut() {
        *b ^= lfsr;
        lfsr = (lfsr << 1) ^ if lfsr & 0x80 != 0 { 0x1D } else { 0 };
    }
}

/// Sends one packet, keyed already. False if the chip never reports it finished.
fn tx_packet(p: &[u8; PACKET], api: &Api, lead_ms: u32) -> bool {
    bus::write(R_CTRL, CTRL_TX | CTRL_CLEAR);
    bus::write(R_CTRL, CTRL_TX);
    bus::write(R_INT, 0);
    bus::write(R_IRQ_EN, IRQ_TX_DONE);
    (api.delay_ms)(lead_ms);
    for pair in p.chunks_exact(2) {
        bus::write(R_FIFO, ((pair[1] as u16) << 8) | pair[0] as u16);
    }
    bus::write(R_CTRL, CTRL_TX | CTRL_TX_ON);
    let mut done = false;
    for _ in 0..300 {
        (api.delay_ms)(5);
        if bus::read(R_STATUS) & 1 != 0 {
            bus::write(R_INT, 0);
            if bus::read(R_INT) & IRQ_TX_DONE != 0 {
                done = true;
                break;
            }
        }
    }
    bus::write(R_CTRL, CTRL_TX);
    bus::write(R_INT, 0);
    done
}

fn sealed(mut p: [u8; PACKET]) -> [u8; PACKET] {
    p[0] = MAGIC;
    p[CHECK_AT] = checksum(&p);
    p[CHECK_AT + 1] = 0x55;
    p[CHECK_AT + 2] = 0x55;
    scramble(&mut p);
    p
}

/// One whole transmission: key up, send `count` packets, unkey, back to listening.
/// A packet that never reports finishing gives up after 1.5 s.
fn transmit(api: &Api, s: &mut State, packets: &[[u8; PACKET]]) -> Tx {
    (api.set_tx_freq)(s.hz);
    (api.set_power)(if s.high_power { 0 } else { 1 });
    (api.set_subaudio_tx)(0);
    (api.set_tx_enabled)(true);
    (api.set_ptt)(true);
    if (api.tx_state)() & 1 == 0 {
        (api.set_ptt)(false);
        (api.set_tx_enabled)(false);
        modem_rx(s);
        return Tx::Blocked;
    }
    let dev = bus::read(R_DEV);
    let filter = bus::read(R_FILTER);
    bus::write(R_DEV, (dev & 0xF000) | if s.wide { 1300 } else { 1200 });
    bus::write(R_FILTER, (1 << 2) | 1);
    modem_common(PACKET as u16);
    let mut result = Tx::Sent;
    for (index, p) in packets.iter().enumerate() {
        let lead = if index == 0 { LEAD_MS } else { NEXT_PACKET_LEAD_MS };
        if !tx_packet(&sealed(*p), api, lead) {
            result = Tx::Stuck;
            break;
        }
    }
    (api.delay_ms)(TAIL_MS);
    bus::write(R_DEV, dev);
    bus::write(R_FILTER, filter);
    (api.set_ptt)(false);
    (api.set_tx_enabled)(false);
    (api.set_af_out)(false);
    modem_rx(s);
    if result == Tx::Sent {
        tally!(s.n_tx);
        }
    result
}

/// One try at the message waiting to go out.
fn send_out(api: &Api, s: &mut State) -> Tx {
    let len = s.out_len as usize;
    let total = len.div_ceil(CHARS).max(1);
    let mut packets = [[0u8; PACKET]; 2];
    for (part, p) in packets[..total].iter_mut().enumerate() {
        p[1] = s.me;
        p[2] = s.out_id;
        p[3] = ((part as u8) << 4) | total as u8;
        let start = part * CHARS;
        let chunk = &s.out[start..len.min(start + CHARS)];
        p[4..4 + chunk.len()].copy_from_slice(chunk);
    }
    s.redraw_hint = true;
    transmit(api, s, &packets[..total])
}

/// When to try again, from now: call this once the transmission has ended. These run
/// off the clock because the tick's `dt` includes however long the last transmission or
/// reception blocked, which would use up a short wait before it began.
fn next_retry_at(api: &Api) -> u32 {
    let now = (api.uptime_100us)();
    now.wrapping_add(REPLY_WAIT_100US + (now & REPLY_JITTER_MASK))
}

fn is_due(api: &Api, at: u32) -> bool {
    (api.uptime_100us)().wrapping_sub(at) as i32 >= 0
}

/// True if the first try went out, so the typed text can be cleared.
fn start_send(api: &Api, s: &mut State, text: &[u8]) -> bool {
    if s.pending {
        set_notice(s, b"WAIT: STILL SENDING");
        return false;
    }
    let n = text.len().min(MAX_LEN);
    s.out[..n].copy_from_slice(&text[..n]);
    s.out_len = n as u8;
    s.out_id = s.next_id;
    s.next_id = s.next_id.wrapping_add(1);
    match send_out(api, s) {
        Tx::Sent => {
            s.pending = true;
            s.tries = 1;
            s.retry_at = next_retry_at(api);
            s.notice = None; // the hint row shows the try count from here
            s.notice_left = 0;
            store(api, s, text, STATUS_SENDING, s.out_id);
            s.out_slot = (s.log_head + LOG_SLOTS - 1) % LOG_SLOTS;
            true
        }
        Tx::Blocked => {
            set_notice(s, b"TX BLOCKED: SEE SETTINGS");
            false
        }
        Tx::Stuck => {
            set_notice(s, b"TX FAILED: NO DONE FLAG");
            false
        }
    }
}

fn end_send(api: &Api, s: &mut State, status: u8, notice: &'static [u8]) {
    s.pending = false;
    let h = read_head(api, s.out_slot);
    if head_valid(&h) && h[1] == STATUS_SENDING && h[2] == s.out_id {
        (api.nor_write)(log_addr(s.out_slot) + 1, &status, 1);
    }
    s.redraw_msgs = true;
    set_notice(s, notice);
}

fn retry(api: &Api, s: &mut State) {
    if s.tries >= MAX_TRIES {
        end_send(api, s, STATUS_FAILED, b"NOT CONFIRMED");
        return;
    }
    s.tries += 1;
    match send_out(api, s) {
        Tx::Sent => s.retry_at = next_retry_at(api),
        Tx::Blocked => end_send(api, s, STATUS_FAILED, b"TX BLOCKED: SEE SETTINGS"),
        Tx::Stuck => end_send(api, s, STATUS_FAILED, b"TX FAILED: NO DONE FLAG"),
    }
}

fn log_addr(slot: u16) -> u32 {
    LOG_ADDR + slot as u32 * REC_BYTES as u32
}

fn read_head(api: &Api, slot: u16) -> [u8; REC_HEAD] {
    let mut h = [0u8; REC_HEAD];
    (api.nor_read)(log_addr(slot), h.as_mut_ptr(), REC_HEAD as u32);
    h
}

fn head_valid(h: &[u8; REC_HEAD]) -> bool {
    h[0] == REC_MAGIC && h[3] as usize <= MAX_LEN
}

/// Finds where the log left off.
fn log_open(api: &Api, s: &mut State) {
    let mut best: Option<u16> = None;
    for slot in 0..LOG_SLOTS {
        let h = read_head(api, slot);
        if !head_valid(&h) {
            continue;
        }
        s.log_count += 1;
        let seq = u16::from_le_bytes([h[4], h[5]]);
        if best.map_or(true, |b| seq.wrapping_sub(b) as i16 > 0) {
            best = Some(seq);
            s.log_head = (slot + 1) % LOG_SLOTS;
        }
    }
    s.log_seq = best.map_or(0, |b| b.wrapping_add(1));
}

fn log_erase(api: &Api, s: &mut State) {
    for sector in 0..LOG_SECTORS {
        (api.nor_erase_sector)(LOG_ADDR + sector * SECTOR_BYTES);
    }
    s.log_head = 0;
    s.log_count = 0;
    s.scroll = 0;
    s.redraw_msgs = true;
}

fn store(api: &Api, s: &mut State, text: &[u8], status: u8, id: u8) {
    if s.log_head % LOG_PER_SECTOR == 0 {
        if head_valid(&read_head(api, s.log_head)) {
            s.log_count = s.log_count.saturating_sub(LOG_PER_SECTOR);
        }
        (api.nor_erase_sector)(log_addr(s.log_head));
    }
    let n = text.len().min(MAX_LEN);
    let mut rec = [0xFFu8; REC_BYTES];
    rec[0] = REC_MAGIC;
    rec[1] = status;
    rec[2] = id;
    rec[3] = n as u8;
    rec[4..REC_HEAD].copy_from_slice(&s.log_seq.to_le_bytes());
    rec[REC_HEAD..REC_HEAD + n].copy_from_slice(&text[..n]);
    (api.nor_write)(log_addr(s.log_head), rec.as_ptr(), REC_BYTES as u32);
    s.log_head = (s.log_head + 1) % LOG_SLOTS;
    s.log_count = (s.log_count + 1).min(LOG_SLOTS);
    s.log_seq = s.log_seq.wrapping_add(1);
    s.scroll = 0;
    s.redraw_msgs = true;
}

/// Plays through the speaker, then puts the receiver back the way the app opens it.
fn chime(api: &Api, s: &mut State, notes: &[u16]) {
    bus::write(R_GAIN, TONE_GAIN);
    bus::write(R_STATE, 0);
    bus::write(R_STATE, STATE_TONE);
    bus::write(R_AF, AF_BEEP);
    bus::write(R_VOLUME, VOLUME_BEEP);
    (api.set_speaker)(true);
    for &hz in notes {
        bus::write(R_TONE, (hz as u32 * 1_032_444 / 100_000) as u16);
        (api.delay_ms)(TONE_MS);
    }
    (api.set_speaker)(false);
    bus::write(R_TONE, 0);
    bus::write(R_GAIN, 0);
    (api.enter_rx)();
    (api.set_holds_rx)(false);
    (api.set_af_out)(false);
    modem_rx(s);
}

/// `parts_after`: packets of this message still on the air, which the reply must not
/// talk over.
fn schedule_reply(api: &Api, s: &mut State, to: u8, id: u8, parts_after: u32) {
    s.reply_due = true;
    s.reply_to = to;
    s.reply_id = id;
    s.reply_at = (api.uptime_100us)().wrapping_add(REPLY_DELAY_100US + parts_after * PACKET_100US);
}

fn send_reply(api: &Api, s: &mut State) {
    let mut p = [0u8; PACKET];
    p[1] = s.me;
    p[4] = s.reply_to;
    p[5] = s.reply_id;
    if transmit(api, s, &[p]) != Tx::Sent {
        set_notice(s, b"CAN'T REPLY: TX BLOCKED");
    }
}

fn finish_join(api: &Api, s: &mut State) {
    s.join_active = false;
    let mut out = [0u8; MAX_LEN];
    let mut n = 0;
    for &c in s.join.iter() {
        if c == 0 {
            break;
        }
        out[n] = c;
        n += 1;
    }
    s.has_last = true;
    s.last_from = s.join_from;
    s.last_id = s.join_id;
    tally!(s.n_got);
    store(api, s, &out[..n], STATUS_RECEIVED, s.join_id);
    s.chime_due = CHIME_NEW;
    set_notice(s, b"NEW MESSAGE");
}

fn take_packet(api: &Api, s: &mut State) {
    let mut p = s.rx_buf;
    scramble(&mut p);
    if p[0] != MAGIC || p[CHECK_AT] != checksum(&p) {
        tally!(s.n_bad);
        return;
    }
    // A reply has no part or total. Only one meant for this radio's waiting message counts.
    if p[3] == 0 {
        tally!(s.n_ok);
        if s.pending && p[4] == s.me && p[5] == s.out_id {
            end_send(api, s, STATUS_DELIVERED, b"DELIVERED");
            s.chime_due = CHIME_DELIVERED;
        }
        return;
    }
    let total = p[3] & 0x0F;
    let part = p[3] >> 4;
    if total == 0 || total > 2 || part >= total {
        tally!(s.n_bad);
        return;
    }
    tally!(s.n_ok);
    let (from, id) = (p[1], p[2]);
    if s.has_last && s.last_from == from && s.last_id == id {
        // A repeat: the sender didn't hear the reply. Answer at its last packet.
        if part + 1 == total {
            schedule_reply(api, s, from, id, 0);
        }
        return;
    }
    if !s.join_active || s.join_from != from || s.join_id != id {
        s.join_active = true;
        s.join_from = from;
        s.join_id = id;
        s.join_total = total;
        s.join_have = 0;
        s.join = [0; 2 * CHARS];
    }
    s.join_age = 0;
    let at = part as usize * CHARS;
    for i in 0..CHARS {
        let c = p[4 + i];
        s.join[at + i] = if c == 0 || (0x20..0x7F).contains(&c) { c } else { b'?' };
    }
    s.join_have |= 1 << part;
    if s.join_have == (1 << s.join_total) - 1 {
        finish_join(api, s);
        schedule_reply(api, s, from, id, (total - 1 - part) as u32);
    }
}

fn rx_word(s: &mut State) {
    let word = bus::read(R_FIFO);
    let i = s.rx_i as usize;
    if i < PACKET {
        s.rx_buf[i] = word as u8;
    }
    if i + 1 < PACKET {
        s.rx_buf[i + 1] = (word >> 8) as u8;
    }
    s.rx_i = (i + 2).min(PACKET) as u8;
}

fn finish_rx(api: &Api, s: &mut State) {
    s.rx_active = false;
    take_packet(api, s);
    rx_restart();
}

fn poll_modem(api: &Api, s: &mut State) {
    // A word arrives about every 36 ms, but ticks can be far apart and the screen is
    // slower still. So from sync to the end of the packet this stays here with the
    // chip, under a second, and nothing else runs.
    let mut wait = 0u16;
    for _ in 0..1000 {
        if bus::read(R_STATUS) & 1 == 0 {
            if !s.rx_active {
                return;
            }
            if wait == 0 {
                s.rx_active = false;
                tally!(s.n_bad);
                    rx_restart();
                return;
            }
            wait -= 1;
            (api.delay_ms)(2);
            continue;
        }
        bus::write(R_INT, 0);
        let flags = bus::read(R_INT);
        if flags & IRQ_SYNC != 0 {
            s.rx_active = true;
            s.rx_i = 0;
            s.rx_buf = [0; PACKET];
            wait = RX_WAIT_STEPS;
            tally!(s.n_sync);
        }
        if flags & IRQ_WORD != 0 && s.rx_active {
            rx_word(s);
            wait = RX_WAIT_STEPS;
            // Packets are one fixed size. On this chip the done flag never comes,
            // and it goes on decoding noise as words, so stop counting here.
            if s.rx_i as usize >= PACKET {
                finish_rx(api, s);
            }
        }
        if flags & IRQ_RX_DONE != 0 {
            if s.rx_active {
                // Anything still waiting in the FIFO.
                while (s.rx_i as usize) < PACKET {
                    rx_word(s);
                }
                finish_rx(api, s);
            } else {
                rx_restart();
            }
        }
    }
}

fn countdown(left: &mut u32, dt: u32) -> bool {
    if *left == 0 {
        return false;
    }
    *left = left.saturating_sub(dt);
    *left == 0
}

fn tick(api: &Api, dt: u32) -> AppResult {
    let s = st();
    if countdown(&mut s.tap_left, dt) {
        s.tap_key = NO_KEY;
        s.redraw_comp = true;
    }
    if countdown(&mut s.notice_left, dt) {
        s.notice = None;
        s.erase_armed = false;
        s.redraw_hint = true;
    }
    if s.join_active {
        s.join_age = s.join_age.saturating_add(dt);
        if s.join_age >= JOIN_100US {
            s.join_active = false;
        }
    }
    if (api.read_rssi)() >= BUSY_RAW {
        s.last_busy = (api.uptime_100us)();
    }
    poll_modem(api, s);
    if s.chime_due != 0 {
        // Its own tick, so the reply's quiet check next tick reads the channel afresh.
        let notes: &[u16] = if s.chime_due == CHIME_NEW { &[880, 1320] } else { &[1320] };
        s.chime_due = 0;
        chime(api, s, notes);
        return AppResult::Continue;
    }
    let quiet = is_due(api, s.last_busy.wrapping_add(QUIET_100US));
    if s.reply_due {
        // A reply goes first, whatever else is waiting.
        if is_due(api, s.reply_at) {
            if quiet {
                s.reply_due = false;
                send_reply(api, s);
            } else if is_due(api, s.reply_at.wrapping_add(REPLY_GIVE_UP_100US)) {
                s.reply_due = false;
            }
        }
    } else if s.pending
        && is_due(api, s.retry_at)
        && (quiet || is_due(api, s.retry_at.wrapping_add(RETRY_GIVE_UP_100US)))
    {
        retry(api, s);
    }
    AppResult::Continue
}

fn set_notice(s: &mut State, text: &'static [u8]) {
    s.notice = Some(text);
    s.notice_left = NOTICE_100US;
    s.redraw_hint = true;
}

fn type_digit(s: &mut State, digit: u8) {
    let set = TAPS[digit as usize];
    if s.tap_key == digit && s.comp_len > 0 {
        s.tap_index = (s.tap_index + 1) % set.len() as u8;
        s.comp[s.comp_len as usize - 1] = set[s.tap_index as usize];
    } else {
        if s.comp_len as usize == MAX_LEN {
            set_notice(s, b"MESSAGE FULL");
            return;
        }
        s.tap_index = 0;
        s.comp[s.comp_len as usize] = set[0];
        s.comp_len += 1;
    }
    s.tap_key = digit;
    s.tap_left = TAP_100US;
    s.redraw_comp = true;
}

fn key(api: &Api, id: u8, kind: u8) -> AppResult {
    let s = st();
    if kind == KIND_LONG {
        if id == KEY_EXIT {
            modem_off(s);
            return AppResult::Exit;
        }
        if id == KEY_POUND {
            if s.erase_armed {
                log_erase(api, s);
                set_notice(s, b"MESSAGES ERASED");
            } else {
                s.erase_armed = true;
                set_notice(s, b"HOLD # AGAIN TO ERASE ALL");
            }
        }
        return AppResult::Continue;
    }
    if kind != KIND_SINGLE {
        return AppResult::Continue;
    }
    s.erase_armed = false;
    match id {
        0..=9 => type_digit(s, id),
        KEY_MENU => {
            if s.comp_len == 0 {
                set_notice(s, b"TYPE A MESSAGE FIRST");
            } else {
                let text = s.comp;
                let n = s.comp_len as usize;
                s.tap_key = NO_KEY;
                s.tap_left = 0;
                if start_send(api, s, &text[..n]) {
                    s.comp_len = 0;
                }
                s.redraw_comp = true;
            }
        }
        KEY_STAR => {
            let mut b = Line::new();
            b.push(b"PING ");
            b.num(s.pings as u32 + 1);
            if start_send(api, s, b.as_slice()) {
                s.pings = s.pings.wrapping_add(1);
            }
        }
        KEY_AB => {
            s.high_power = !s.high_power;
            s.redraw_all = true;
        }
        KEY_UP => {
            if s.scroll + 1 < s.log_count {
                s.scroll += 1;
                s.redraw_msgs = true;
            }
        }
        KEY_DOWN => {
            if s.scroll > 0 {
                s.scroll -= 1;
                s.redraw_msgs = true;
            }
        }
        KEY_EXIT => {
            if s.pending {
                end_send(api, s, STATUS_FAILED, b"STOPPED: NOT CONFIRMED");
                return AppResult::Continue;
            }
            if s.comp_len == 0 {
                modem_off(s);
                return AppResult::Exit;
            }
            s.comp_len -= 1;
            s.tap_key = NO_KEY;
            s.tap_left = 0;
            s.redraw_comp = true;
        }
        _ => {}
    }
    AppResult::Continue
}

// ui

struct Line {
    buf: [u8; 40],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Line { buf: [0; 40], len: 0 }
    }
    fn push(&mut self, s: &[u8]) {
        for &c in s {
            if self.len < self.buf.len() {
                self.buf[self.len] = c;
                self.len += 1;
            }
        }
    }
    fn num(&mut self, mut v: u32) {
        let mut d = [0u8; 10];
        let mut n = 0;
        loop {
            d[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            self.push(&d[n..n + 1]);
        }
    }
    fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16, font: u8) {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, font);
}

fn draw_title(api: &Api, s: &State) {
    (api.fill_rect)(0, TITLE_Y, SCREEN_W, 12, HEADER);
    let mut l = Line::new();
    l.push(b"TEXT ");
    let mut f = [0u8; 12];
    let n = (api.fmt_freq)(s.hz, f.as_mut_ptr()) as usize;
    l.push(&f[..n]);
    l.push(if s.high_power { b" HIGH" } else { b" LOW" });
    text(api, 4, TITLE_Y + 1, l.as_slice(), WHITE, HEADER, F6);
}

fn slot_back(s: &State, back: u16) -> u16 {
    (s.log_head + LOG_SLOTS - 1 - back) % LOG_SLOTS
}

fn rows_for(len: u8) -> usize {
    (len as usize + 1).div_ceil(COLS)
}

fn draw_msgs(api: &Api, s: &State) {
    (api.fill_rect)(0, MSG_Y, SCREEN_W, MSG_ROWS as u16 * 10, BLACK);
    if s.log_count == 0 {
        text(api, 4, MSG_Y, b"NO MESSAGES YET", GRAY, BLACK, F6);
        return;
    }
    // Fit as many messages as possible, ending at the one scrolled to.
    let newest = s.scroll;
    let mut oldest = newest;
    let mut rows = rows_for(read_head(api, slot_back(s, newest))[3]);
    while oldest + 1 < s.log_count {
        let more = rows_for(read_head(api, slot_back(s, oldest + 1))[3]);
        if rows + more > MSG_ROWS {
            break;
        }
        oldest += 1;
        rows += more;
    }
    let mut y = MSG_Y;
    for back in (newest..=oldest).rev() {
        let slot = slot_back(s, back);
        let mut rec = [0u8; REC_BYTES];
        (api.nor_read)(log_addr(slot), rec.as_mut_ptr(), REC_BYTES as u32);
        // Still marked sending, but not the one being sent: the app closed mid-try.
        let live = s.pending && slot == s.out_slot;
        let (mark, color) = match rec[1] {
            STATUS_RECEIVED => (b'<', GREEN),
            STATUS_SENDING if live => (b'>', GRAY),
            STATUS_DELIVERED => (b'+', WHITE),
            _ => (b'!', AMBER),
        };
        let mut row = [0u8; COLS];
        row[0] = mark;
        let mut col = 1;
        let len = (rec[3] as usize).min(MAX_LEN);
        for &c in &rec[REC_HEAD..REC_HEAD + len] {
            if col == COLS {
                text(api, 2, y, &row, color, BLACK, F6);
                y += 10;
                col = 0;
            }
            row[col] = c;
            col += 1;
        }
        text(api, 2, y, &row[..col], color, BLACK, F6);
        y += 10;
    }
}

fn draw_comp(api: &Api, s: &State) {
    (api.fill_rect)(0, COMP_Y, SCREEN_W, 21, BLACK);
    (api.fill_rect)(0, COMP_Y, SCREEN_W, 1, HEADER);
    let n = s.comp_len as usize;
    for (row, chunk) in s.comp[..n].chunks(COLS).enumerate() {
        text(api, 2, COMP_Y + 2 + row as i16 * 10, chunk, WHITE, BLACK, F6);
    }
    let at = |i: usize| (2 + (i % COLS) as i16 * 6, COMP_Y + 2 + (i / COLS) as i16 * 10);
    if s.tap_key != NO_KEY && n > 0 {
        let (x, y) = at(n - 1);
        text(api, x, y, &s.comp[n - 1..n], WHITE, HILITE, F6);
    }
    if n < MAX_LEN {
        let (x, y) = at(n);
        text(api, x, y, b"_", GRAY, BLACK, F6);
    }
}

fn draw_hint(api: &Api, s: &State) {
    (api.fill_rect)(0, HINT_Y, SCREEN_W, 9, BLACK);
    if let Some(n) = s.notice {
        text(api, 2, HINT_Y, n, AMBER, BLACK, F5);
    } else if s.pending {
        let mut l = Line::new();
        l.push(b"TRY ");
        l.num(s.tries as u32);
        l.push(b" OF ");
        l.num(MAX_TRIES as u32);
        l.push(b"  EXIT STOPS");
        text(api, 2, HINT_Y, l.as_slice(), AMBER, BLACK, F5);
    } else {
        text(api, 2, HINT_Y, b"MENU SEND  EXIT DEL  *PING", GRAY, BLACK, F5);
    }
}

fn draw(api: &Api) -> AppResult {
    let s = st();
    if s.redraw_all {
        (api.fill_rect)(0, 0, SCREEN_W, 128, BLACK);
        draw_title(api, s);
        text(api, 2, KEYS_Y, b"1.,? 2ABC 3DEF 4GHI 5JKL", GRAY, BLACK, F5);
        text(api, 2, KEYS_Y + 8, b"6MNO 7PQRS 8TUV 9WXYZ 0SP", GRAY, BLACK, F5);
        s.redraw_msgs = true;
        s.redraw_comp = true;
        s.redraw_hint = true;
        s.redraw_all = false;
    }
    if s.redraw_msgs {
        draw_msgs(api, s);
        s.redraw_msgs = false;
    }
    if s.redraw_comp {
        draw_comp(api, s);
        s.redraw_comp = false;
    }
    if s.redraw_hint {
        draw_hint(api, s);
        s.redraw_hint = false;
    }
    AppResult::Continue
}

// Read-only views for the PC simulator; not part of the radio build.
#[cfg(not(target_os = "none"))]
pub fn debug_counts() -> (u16, u16, u16, u16) {
    let s = st();
    (s.n_sync, s.n_ok, s.n_bad, s.n_tx)
}

#[cfg(not(target_os = "none"))]
pub fn debug_comp() -> String {
    let s = st();
    String::from_utf8_lossy(&s.comp[..s.comp_len as usize]).into_owned()
}

/// Oldest first, with a stale "sending" read as failed, as the screen does.
#[cfg(not(target_os = "none"))]
pub fn debug_msgs() -> Vec<(u8, String)> {
    let s = st();
    let api = &crate::API;
    (0..s.log_count)
        .rev()
        .map(|back| {
            let slot = slot_back(s, back);
            let mut rec = [0u8; REC_BYTES];
            (api.nor_read)(log_addr(slot), rec.as_mut_ptr(), REC_BYTES as u32);
            let live = s.pending && slot == s.out_slot;
            let status = if rec[1] == STATUS_SENDING && !live { STATUS_FAILED } else { rec[1] };
            (status, String::from_utf8_lossy(&rec[REC_HEAD..REC_HEAD + rec[3] as usize]).into_owned())
        })
        .collect()
}

#[cfg(not(target_os = "none"))]
pub fn debug_notice() -> String {
    String::from_utf8_lossy(st().notice.unwrap_or(b"")).into_owned()
}

#[cfg(not(target_os = "none"))]
pub fn debug_got() -> u16 {
    st().n_got
}

#[cfg(not(target_os = "none"))]
pub fn debug_pending() -> (bool, u8) {
    (st().pending, st().tries)
}

#[cfg(not(target_os = "none"))]
pub fn debug_force_redraw() {
    st().redraw_all = true;
}

#[cfg(not(target_os = "none"))]
pub fn debug_sign(p: &mut [u8; PACKET]) {
    p[CHECK_AT] = checksum(p);
}

#[cfg(not(target_os = "none"))]
pub fn debug_scramble(p: &mut [u8; PACKET]) {
    scramble(p);
}
