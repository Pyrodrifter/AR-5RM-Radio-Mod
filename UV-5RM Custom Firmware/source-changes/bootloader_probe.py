"""Check that the bootloader answers. Sends only the HANDSHAKE frame; there is no write path here.

Frame layout and CRC match tools/flash.py from bf-5rh-firmware-open.
"""
import sys, time
import serial

PORT = sys.argv[1]
CMD_HANDSHAKE, ACK = 0x01, 0x06


def crc16_xmodem(data):
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else crc << 1
        crc &= 0xFFFF
    return crc


def frame(cmd, args, data):
    body = bytes([cmd, args, len(data) >> 8, len(data) & 0xFF]) + data
    crc = crc16_xmodem(body)
    return b"\xAA" + body + bytes([crc >> 8, crc & 0xFF, 0xEF])


s = serial.Serial(PORT, 115200, timeout=0.05)
s.dtr = True
s.rts = True
try:
    idle = bytearray()
    end = time.monotonic() + 1.0
    while time.monotonic() < end:
        idle += s.read(64)
    print("bytes heard before sending anything:", idle[:32].hex() or "(none)")

    reply = b""
    for attempt in range(1, 6):
        s.reset_input_buffer()
        s.write(frame(CMD_HANDSHAKE, 0, b"BOOTLOADER"))
        buf = bytearray()
        end = time.monotonic() + 2.0
        while time.monotonic() < end and len(buf) < 32:
            buf += s.read(32)
            if buf and buf[0] == 0xAA and len(buf) >= 8:
                break
        print(f"attempt {attempt}: reply {bytes(buf).hex() or '(none)'}")
        if buf:
            reply = bytes(buf)
            break
    if reply[:1] == b"\xAA" and len(reply) >= 8:
        cmd, args, hi, lo = reply[1:5]
        n = hi << 8 | lo
        body = reply[1:5 + n]
        got = reply[5 + n] << 8 | reply[6 + n]
        ok = got == crc16_xmodem(body)
        print(f"frame cmd=0x{cmd:02X} args=0x{args:02X} len={n} crc_ok={ok}")
        print("BOOTLOADER ANSWERED" if ok and args == ACK else "BOOTLOADER REFUSED THE HANDSHAKE")
    else:
        print("NO BOOTLOADER RESPONSE")
finally:
    s.close()
