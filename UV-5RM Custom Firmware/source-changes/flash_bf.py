"""Send an application-only .BF to the Baofeng KDH bootloader."""

import argparse
from binascii import crc_hqx
import hashlib
from pathlib import Path
import re
import struct
import sys
import time

HEADER, FOOTER = 0xAA, 0xEF
CMD_HANDSHAKE, CMD_UPDATE, CMD_PACKAGES, CMD_END = 0x01, 0x03, 0x04, 0x45
ACK, CHUNK, TIMEOUT = 0x06, 1024, 2.0
APP_START, FLASH_END = 0x08001000, 0x08010000
ERRORS = {
    0xE1: "handshake, command or frame error",
    0xE2: "sequence or length mismatch",
    0xE3: "flash write failed",
    0xE5: "unknown command",
}


def pack(cmd, args, data):
    body = struct.pack(">BBH", cmd, args, len(data)) + data
    return bytes([HEADER]) + body + struct.pack(">HB", crc_hqx(body, 0), FOOTER)


def application_image(path, expected_sha):
    if re.fullmatch(r"[0-9a-fA-F]{16,64}", expected_sha) is None:
        raise ValueError("supply at least the first 16 hex characters of the expected SHA-256")
    raw = path.read_bytes()
    sha = hashlib.sha256(raw).hexdigest()
    if not sha.startswith(expected_sha.lower()):
        raise ValueError(f"file SHA-256 is {sha}, expected {expected_sha}")
    if len(raw) < 24:
        raise ValueError("file is too short for a .BF header and application vectors")
    count, length, auxiliary = struct.unpack_from(">BII", raw)
    if count != 1 or auxiliary != 0 or len(raw) != 16 + length or length > FLASH_END - APP_START:
        raise ValueError("expected one bounded application region and no auxiliary region")
    stack, reset = struct.unpack_from("<II", raw, 16)
    if not (0x20000000 < stack <= 0x20004000 and stack % 4 == 0):
        raise ValueError(f"initial stack pointer outside application RAM: {stack:#010x}")
    if not (reset & 1 and APP_START <= (reset & ~1) < APP_START + length):
        raise ValueError(f"reset vector outside the supplied Thumb application: {reset:#010x}")
    print(f"file ok: sha {sha}, {length} bytes, {(length + CHUNK - 1) // CHUNK} packages")
    return raw[16:]


def read_reply(port, cmd, request):
    deadline = time.monotonic() + TIMEOUT
    buf = bytearray()
    while time.monotonic() < deadline:
        buf.extend(port.read(64))
        while len(buf) >= 8:
            start = buf.find(bytes([HEADER]))
            if start < 0:
                buf.clear()
                break
            del buf[:start]
            if len(buf) < 8:
                break
            length = int.from_bytes(buf[3:5], "big")
            if length > CHUNK:
                del buf[0]
                continue
            end = 8 + length
            if len(buf) < end:
                break
            packet = bytes(buf[:end])
            del buf[:end]
            if length or packet == request or packet[-1] != FOOTER:
                continue  # Bootloader replies have no data, unlike echoed update frames.
            if int.from_bytes(packet[5:7], "big") != crc_hqx(packet[1:5], 0):
                continue
            reply_cmd, status = packet[1:3]
            if reply_cmd == cmd:
                if status == ACK:
                    return True
                raise RuntimeError(ERRORS.get(status, f"unexpected status {status:#04x}"))
            if reply_cmd in ERRORS:
                raise RuntimeError(ERRORS[reply_cmd])
    return False


def send(port, cmd, args=0, data=b"", retries=1):
    frame = pack(cmd, args, data)
    for _ in range(retries):
        port.reset_input_buffer()
        port.write(frame)
        if read_reply(port, cmd, frame):
            return
    raise RuntimeError(f"no matching acknowledgment for command {cmd:#04x}, argument {args}")


def flash(port, region):
    send(port, CMD_HANDSHAKE, data=b"BOOTLOADER", retries=5)
    packages = (len(region) + CHUNK - 1) // CHUNK
    send(port, CMD_PACKAGES, data=bytes([packages]))
    print("Application programming started.", flush=True)
    start = time.monotonic()
    for seq in range(packages):
        chunk = region[seq * CHUNK:(seq + 1) * CHUNK].ljust(CHUNK, b"\x00")
        # If a reply is lost, this page may already have been written. Stop
        # and allow a fresh bootloader session from page zero.
        send(port, CMD_UPDATE, seq, chunk)
        if (seq + 1) % 10 == 0 or seq + 1 == packages:
            print(f"  {seq + 1}/{packages}", flush=True)
    print(f"Application packets acknowledged in {time.monotonic() - start:.1f}s")
    send(port, CMD_END)
    print("Restart acknowledged. Verify boot and receive on the radio.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("port")
    parser.add_argument("sha256", help="expected SHA-256 or its first 16 or more hex characters")
    parser.add_argument("--check-only", action="store_true", help="validate the image without opening the port")
    args = parser.parse_args()

    try:
        region = application_image(args.image, args.sha256)
        if args.check_only:
            return 0
        import serial
        with serial.Serial(args.port, 115200, timeout=0.03, write_timeout=2) as port:
            port.dtr = True
            port.rts = True
            flash(port, region)
    except (OSError, ValueError, RuntimeError) as exc:
        print(f"Update stopped: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
