"""Probe a KDH bootloader using only its handshake command."""

import argparse
from binascii import crc_hqx
import struct
import sys
import time

import serial

START, END = 0xAA, 0xEF
CMD_HANDSHAKE, ACK = 0x01, 0x06
REPLY_COMMANDS = {CMD_HANDSHAKE, 0xE1, 0xE2, 0xE3, 0xE5}
HANDSHAKE_BODY = b"\x01\x00\x00\x0aBOOTLOADER"
HANDSHAKE = bytes([START]) + HANDSHAKE_BODY + struct.pack(
    ">HB", crc_hqx(HANDSHAKE_BODY, 0), END
)


def find_reply(received):
    for start in range(len(received) - 7):
        if received[start] != START:
            continue
        cmd, status, size = struct.unpack_from(">BBH", received, start + 1)
        if cmd not in REPLY_COMMANDS:
            continue
        crc_at = start + 5 + size
        end = crc_at + 3
        if end > len(received) or received[end - 1] != END:
            continue
        if received[start:end] == HANDSHAKE:
            continue  # Some cables echo the probe back.
        crc = struct.unpack_from(">H", received, crc_at)[0]
        if crc == crc_hqx(received[start + 1:crc_at], 0):
            return cmd, status, size
    return None


def probe(port):
    idle = port.read(32)
    print("bytes heard before sending anything:", idle.hex() or "(none)")
    saw_bytes = False
    for attempt in range(1, 6):
        port.reset_input_buffer()
        port.write(HANDSHAKE)
        received = bytearray()
        reply = None
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline and len(received) < 4096:
            chunk = port.read(32)
            if chunk:
                received.extend(chunk)
                reply = find_reply(received)
                if reply is not None:
                    break
        saw_bytes |= bool(received)
        print(f"attempt {attempt}: reply {received[:64].hex() or '(none)'}")
        if reply is not None:
            cmd, status, size = reply
            print(f"frame cmd=0x{cmd:02X} args=0x{status:02X} len={size} crc_ok=True")
            if cmd == CMD_HANDSHAKE and status == ACK:
                print("BOOTLOADER ANSWERED")
                return True
            print("BOOTLOADER REFUSED THE HANDSHAKE")
            return False

    print("NO VALID BOOTLOADER REPLY" if saw_bytes else "NO BOOTLOADER RESPONSE")
    return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port", help="serial port of the programming cable, such as COM7")
    args = parser.parse_args()

    try:
        with serial.Serial(args.port, 115200, timeout=0.05, write_timeout=1) as port:
            port.dtr = True
            port.rts = True
            return 0 if probe(port) else 1
    except serial.SerialException as exc:
        print(f"Serial error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
