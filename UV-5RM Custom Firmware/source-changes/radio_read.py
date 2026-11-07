"""Read a stock Baofeng 5RM using the CHIRP BF5RM programming handshake.

Only the handshake below and 'R' read frames can leave this script: send() refuses
anything else, so there is no path that writes to the radio.

With --calibration, read unencrypted blocks at F000, F040, F080, F0C0,
F100, F200 and F240 in that order. Verified on 5RHNRF v0.21.
"""
import argparse
from pathlib import Path
import struct
import time
import serial

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("port")
parser.add_argument("output", type=Path)
parser.add_argument("--calibration", action="store_true", help="read factory calibration from stock 5RHNRF v0.21")
args = parser.parse_args()
IDENT = b"PROGRAMBFNORMALU"
MAGICS = [(b"\x46", 16), (b"\x4d", 15),
          (b"\x53\x45\x4E\x44\x21\x05\x0D\x01\x01\x01\x04\x11\x08\x05"
           b"\x0D\x0D\x01\x11\x0F\x09\x12\x09\x10\x04\x00", 1)]
STARTS, SIZES = [0x0000, 0x9000, 0xA000, 0xD000], [0x8040, 0x0040, 0x02C0, 0x0040]
BLOCK = 0x40
CALIBRATION_STARTS = (0xF000, 0xF040, 0xF080, 0xF0C0, 0xF100, 0xF200, 0xF240)
KEY = b"CO 7"  # _encrsym = 1
HANDSHAKE = {IDENT} | {m for m, _ in MAGICS}


def crypt(buf):
    out = bytearray()
    for i, b in enumerate(buf):
        k = KEY[i % 4]
        if k != 32 and b not in (0, 255) and b != k and b != k ^ 255:
            out.append(b ^ k)
        else:
            out.append(b)
    return bytes(out)


s = serial.Serial(args.port, 115200, timeout=0.25, rtscts=False, xonxoff=False)


def send(frame):
    is_read = len(frame) == 4 and frame[0] == 0x52
    if frame not in HANDSHAKE and not is_read:
        raise SystemExit(f"REFUSED to send {frame[:8].hex()}")
    s.write(frame)


try:
    s.reset_input_buffer()
    time.sleep(0.01)
    send(IDENT)
    ack = s.read(1)
    print("ident ack:", ack.hex() or "(no response)")
    if ack != b"\x06":
        raise SystemExit("Radio did not acknowledge. It may be off, or the cable isn't fully seated.")
    for magic, n in MAGICS:
        send(magic)
        r = s.read(n)
        print(f"magic {magic[:1].hex()} -> {r.hex()}  ascii={''.join(chr(c) if 32 <= c < 127 else '.' for c in r)!r}")
        if args.calibration and (len(r) != n or (magic == b"M" and not r.startswith(b"5RH "))):
            raise SystemExit("Calibration read requires the stock 5RH programming response")

    data = bytearray()
    ranges = [(addr, BLOCK) for addr in CALIBRATION_STARTS] if args.calibration else zip(STARTS, SIZES)
    for start, size in ranges:
        for addr in range(start, start + size, BLOCK):
            frame = struct.pack(">BHB", 0x52, addr, BLOCK)
            send(frame)
            d = s.read(BLOCK + 4)
            if len(d) != BLOCK + 4:
                raise SystemExit(f"Short read at {addr:#06x}: got {len(d)} bytes after {len(data)} bytes total")
            if args.calibration and d[:4] != frame:
                raise SystemExit(f"Unexpected read header at {addr:#06x}: {d[:4].hex()}")
            data += d[4:] if args.calibration else crypt(d[4:])
    if args.calibration and all(value in (0, 0xFF) for value in data):
        raise SystemExit("The calibration response contains only blank values")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
    print("read", len(data), "bytes ->", args.output)
finally:
    s.close()
