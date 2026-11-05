"""Talk to the custom firmware's programming port. Frames match tools/cps_proto.py.

  cps_tool.py COM3 logo FILE.rgb565        write the boot logo, then read it back to verify
  cps_tool.py COM3 app SLOT FILE.app [...] install apps into slots, verifying each one
"""
import struct, sys, time
import serial

HEADER, ACK = 0xA6, 0x06
READ_LOGO, WRITE_LOGO, END, APP_ERASE, APP_WRITE, READ_RAW, ERR = 0x4C, 0x6C, 0x45, 0x41, 0x61, 0x44, 0xEE
HANDSHAKE = b"PROGRAMBF-5RH"
CHUNK, LOGO_SIZE, FRAME_TIMEOUT, ERASE_TIMEOUT = 512, 160 * 128 * 2, 2.0, 20.0
SLOT_SIZE, SLOT_COUNT = 32 * 1024, 9


def crc16(data):
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else crc << 1
        crc &= 0xFFFF
    return crc


class Cps:
    def __init__(self, name):
        self.port = serial.Serial(name, 115200, timeout=1.0)
        for _ in range(5):
            self.port.reset_input_buffer()
            self.port.write(HANDSHAKE)
            if self.port.read(1) == bytes([ACK]):
                self.port.timeout = 0.03
                print("connected to custom firmware")
                return
        self.port.close()
        raise SystemExit("NO RESPONSE: is the radio on, at its normal screen, with the cable seated?")

    def _exact(self, n, deadline):
        buf = bytearray()
        while len(buf) < n:
            if time.monotonic() > deadline:
                return None
            buf += self.port.read(n - len(buf))
        return bytes(buf)

    def _recv(self, deadline):
        while True:
            b = self._exact(1, deadline)
            if b is None:
                return None
            if b[0] == HEADER:
                break
        head = self._exact(5, deadline)
        if head is None:
            return None
        cmd, addr, length = struct.unpack(">BHH", head)
        data = self._exact(length, deadline) if length else b""
        tail = self._exact(2, deadline) if data is not None else None
        if tail is None or struct.unpack(">H", tail)[0] != crc16(head + data):
            return None
        return cmd, addr, data

    def send(self, cmd, addr, data=b"", timeout=FRAME_TIMEOUT):
        body = struct.pack(">BHH", cmd, addr, len(data)) + data
        frame = bytes([HEADER]) + body + struct.pack(">H", crc16(body))
        for _ in range(5):
            self.port.reset_input_buffer()
            self.port.write(frame)
            r = self._recv(time.monotonic() + timeout)
            if r is None:
                continue
            if r[0] == ERR:
                raise SystemExit(f"RADIO REFUSED cmd {cmd:#x} addr {addr:#x}: error {r[2][:1].hex()}")
            return r
        raise SystemExit(f"NO REPLY to cmd {cmd:#x} addr {addr:#x}")

    def end(self):
        self.send(END, 0)
        self.port.flush()
        self.port.close()


def do_logo(cps, path):
    img = open(path, "rb").read()
    assert len(img) == LOGO_SIZE, len(img)
    for off in range(0, LOGO_SIZE, CHUNK):
        cps.send(WRITE_LOGO, off, img[off:off + CHUNK], ERASE_TIMEOUT if off == 0 else FRAME_TIMEOUT)
    back = bytearray()
    for off in range(0, LOGO_SIZE, CHUNK):
        back += cps.send(READ_LOGO, off)[2]
    if bytes(back) != img:
        bad = sum(a != b for a, b in zip(back, img))
        raise SystemExit(f"LOGO VERIFY FAILED: {bad} bytes differ")
    print(f"logo written and verified: {LOGO_SIZE} bytes")


def do_app(cps, slot, path):
    data = open(path, "rb").read()
    assert 0 <= slot < SLOT_COUNT and data[:4] == b"OVL1" and len(data) <= SLOT_SIZE, (slot, len(data))
    cps.send(APP_ERASE, slot, b"", ERASE_TIMEOUT)
    padded = data.ljust(-(-len(data) // CHUNK) * CHUNK, b"\xff")
    for off in range(0, len(padded), CHUNK):
        cps.send(APP_WRITE, off, padded[off:off + CHUNK])
    print(f"slot {slot}: {path.rsplit('/', 1)[-1]} {len(data)} bytes written")


def main():
    port, what = sys.argv[1:3]
    cps = Cps(port)
    if what == "logo":
        do_logo(cps, sys.argv[3])
    elif what == "app":
        rest = sys.argv[3:]
        for slot, path in zip(rest[0::2], rest[1::2]):
            do_app(cps, int(slot), path)
    else:
        raise SystemExit("unknown command")
    cps.end()
    print("done; radio restarting")


main()
