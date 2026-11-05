"""Read-only dump of a Baofeng 5RM, following CHIRP's BF5RM download sequence.

Only the handshake below and 'R' read frames can leave this script: send() refuses
anything else, so there is no path that writes to the radio.
"""
import os, struct, sys, time
import serial

PORT, OUT = sys.argv[1], sys.argv[2]
IDENT = b"PROGRAMBFNORMALU"
MAGICS = [(b"\x46", 16), (b"\x4d", 15),
          (b"\x53\x45\x4E\x44\x21\x05\x0D\x01\x01\x01\x04\x11\x08\x05"
           b"\x0D\x0D\x01\x11\x0F\x09\x12\x09\x10\x04\x00", 1)]
STARTS, SIZES = [0x0000, 0x9000, 0xA000, 0xD000], [0x8040, 0x0040, 0x02C0, 0x0040]
BLOCK = 0x40
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


s = serial.Serial(PORT, 115200, timeout=0.25, rtscts=False, xonxoff=False)


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

    data = bytearray()
    for start, size in zip(STARTS, SIZES):
        for addr in range(start, start + size, BLOCK):
            send(struct.pack(">BHB", 0x52, addr, BLOCK))
            d = s.read(BLOCK + 4)
            if len(d) != BLOCK + 4:
                raise SystemExit(f"Short read at {addr:#06x}: got {len(d)} bytes after {len(data)} bytes total")
            data += crypt(d[4:])
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "wb") as f:
        f.write(data)
    print("read", len(data), "bytes ->", OUT)
finally:
    s.close()
