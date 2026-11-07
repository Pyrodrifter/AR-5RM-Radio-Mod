use crate::hal::wear_leveled::crc16_xmodem;

pub const HEADER: u8 = 0xA6;

pub const CMD_READ: u8 = 0x52; // 'R'
pub const CMD_WRITE: u8 = 0x57; // 'W'
pub const CMD_END: u8 = 0x45; // 'E'

pub const CMD_READ_LOGO: u8 = 0x4C; // 'L'
pub const CMD_WRITE_LOGO: u8 = 0x6C; // 'l'

/// Home-screen background, same chunking as the logo. A successful read at
/// offset 0 tells the CPS tool this firmware has themes and backgrounds;
/// older firmware answers ERR_BAD_LEN (unknown command).
pub const CMD_READ_BG: u8 = 0x42; // 'B'
pub const CMD_WRITE_BG: u8 = 0x62; // 'b'

pub const CMD_APP_ERASE: u8 = 0x41; // 'A'
pub const CMD_APP_WRITE: u8 = 0x61; // 'a'

pub const CMD_SAT_WRITE: u8 = 0x53; // 'S'

pub const CMD_READ_FLASH_RAW: u8 = 0x44; // 'D'

pub const CMD_ERROR: u8 = 0xEE;

pub const ERR_BAD_CRC: u8 = 0x01;
pub const ERR_BAD_ADDR: u8 = 0x02;
pub const ERR_BAD_LEN: u8 = 0x03;

pub const MAX_DATA: usize = 512;

pub const BOOT_LOGO_CHUNK: u32 = 512;
pub const APP_CHUNK: u32 = 512;
pub const SAT_CHUNK: u32 = 512;

pub struct Frame<'a> {
    pub cmd: u8,
    pub addr: u16,
    pub data: &'a [u8],
}

pub enum FeedResult<'a> {
    /// Frame not yet complete; keep feeding bytes.
    Pending,
    /// A complete, CRC-valid frame.
    Frame(Frame<'a>),
    /// A frame completed but its CRC didn't match.
    CrcError { addr: u16 },
}

enum State {
    Header,
    Body,
    CrcHi,
    CrcLo,
}

/// Byte-at-a-time frame decoder fed from the USART1 RX ring buffer.
pub struct FrameReader {
    state: State,
    body_len: usize,
    body: [u8; 5 + MAX_DATA],
    idx: usize,
    crc_hi: u8,
}

impl FrameReader {
    pub const fn new() -> Self {
        FrameReader {
            state: State::Header,
            body_len: 5,
            body: [0; 5 + MAX_DATA],
            idx: 0,
            crc_hi: 0,
        }
    }

    fn reset(&mut self) {
        self.state = State::Header;
    }

    /// Feed one received byte. Returns `Frame` once a CRC-valid frame is
    /// complete, `CrcError` if a frame completed but failed its CRC check,
    /// or `Pending` otherwise. Oversized frames are silently dropped and
    /// the reader resyncs on the next header byte.
    pub fn feed(&mut self, byte: u8) -> FeedResult<'_> {
        match self.state {
            State::Header => {
                if byte == HEADER {
                    self.idx = 0;
                    self.body_len = 5;
                    self.state = State::Body;
                }
            }
            State::Body => {
                self.body[self.idx] = byte;
                self.idx += 1;
                if self.idx == 5 {
                    let len = u16::from_be_bytes([self.body[3], byte]);
                    if usize::from(len) > MAX_DATA {
                        self.reset();
                        return FeedResult::Pending;
                    }
                    self.body_len += usize::from(len);
                }
                if self.idx == self.body_len {
                    self.state = State::CrcHi;
                }
            }
            State::CrcHi => {
                self.crc_hi = byte;
                self.state = State::CrcLo;
            }
            State::CrcLo => {
                let crc_rx = ((self.crc_hi as u16) << 8) | byte as u16;
                let addr = u16::from_be_bytes([self.body[1], self.body[2]]);
                self.reset();

                let end = self.body_len;
                let crc_calc = crc16_xmodem(&self.body[..end]);
                if crc_rx == crc_calc {
                    return FeedResult::Frame(Frame {
                        cmd: self.body[0],
                        addr,
                        data: &self.body[5..end],
                    });
                }
                return FeedResult::CrcError { addr };
            }
        }
        FeedResult::Pending
    }
}
