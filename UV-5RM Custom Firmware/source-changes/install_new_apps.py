"""Install apps into empty slots only, read every byte back, and confirm the logo and other apps are intact.

  install_new_apps.py COM3 LOGO.rgb565 CHUNK FILE.app [...]
  install_new_apps.py --preserve-stock COM3 - CHUNK FILE.app [...]
Refuses before writing anything if there aren't enough empty slots.
Preserve-stock mode requires completely blank slots and checks TEXT's log area.
"""
import os, sys
from pathlib import Path

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SLOT_BASE = 0x14C000

args = sys.argv[1:]
preserve_stock = args[:1] == ["--preserve-stock"]
if preserve_stock:
    args = args[1:]
port, logo_path, raw_chunk, *apps = args
raw_chunk = int(raw_chunk)
wanted = {}
for path in apps:
    data = Path(path).read_bytes()
    if data[:4] != b"OVL1" or not 108 <= len(data) <= SLOT_SIZE:
        raise SystemExit(f"{path}: invalid overlay package")
    name = data[12:28].split(b"\0")[0].decode()
    if not name or name in wanted:
        raise SystemExit(f"{path}: empty or duplicate app name")
    wanted[name] = data

logo = None if preserve_stock and logo_path == "-" else Path(logo_path).read_bytes()
cps = Cps(port)


def read_raw(addr, length):
    assert addr % raw_chunk == 0
    out = bytearray()
    for i in range(-(-length // raw_chunk)):
        block = cps.send(READ_RAW, addr // raw_chunk + i)[2]
        if len(block) != raw_chunk:
            raise SystemExit(f"Unexpected read size {len(block)} (expected {raw_chunk})")
        out += block
    return bytes(out[:length])


def slot_addr(slot):
    return SLOT_BASE + slot * SLOT_SIZE


before = {}
free = []
for slot in range(SLOT_COUNT):
    head = read_raw(slot_addr(slot), raw_chunk)
    name = head[12:28].split(b"\0")[0].decode("ascii", "replace")
    if head[:4] == b"OVL1":
        before[slot] = (name, head)
        print(f"slot {slot}: {name}")
    elif not preserve_stock or (
        head == b"\xff" * raw_chunk
        and read_raw(slot_addr(slot), SLOT_SIZE) == b"\xff" * SLOT_SIZE
    ):
        free.append(slot)
        print(f"slot {slot}: empty")
    else:
        print(f"slot {slot}: contains data, leaving it alone")

already = {name: slot for slot, (name, _) in before.items() if name in wanted}
if preserve_stock:
    for name, slot in already.items():
        data = wanted[name]
        if read_raw(slot_addr(slot), len(data)) != data:
            raise SystemExit(f"slot {slot}: different {name} package, refusing replacement")
    if "TEXT" in wanted and "TEXT" not in already:
        log_size = 0x4000
        if read_raw(0x1F0000, log_size) != b"\xff" * log_size:
            raise SystemExit("TEXT log area already contains data, nothing was written")

targets = dict(already)
for name in wanted:
    if name not in targets:
        if not free:
            raise SystemExit(f"NO EMPTY SLOT for {name}; nothing else was changed")
        targets[name] = free.pop(0)

for name, data in wanted.items():
    slot = targets[name]
    if preserve_stock and name in already:
        print(f"slot {slot}: {name} already installed and verified")
        continue
    cps.send(APP_ERASE, slot, b"", ERASE_TIMEOUT)
    padded = data.ljust(-(-len(data) // CHUNK) * CHUNK, b"\xff")
    for off in range(0, len(padded), CHUNK):
        cps.send(APP_WRITE, off, padded[off:off + CHUNK])
    back = read_raw(slot_addr(slot), len(data))
    assert back == data, f"{name}: read-back differs"
    print(f"slot {slot}: {name} {len(data)} bytes written and verified")

for slot, (name, head) in before.items():
    if name not in wanted:
        assert read_raw(slot_addr(slot), raw_chunk) == head, f"slot {slot} {name} changed"
print("other apps unchanged")

if logo is not None:
    got = bytearray()
    for off in range(0, len(logo), CHUNK):
        got += cps.send(READ_LOGO, off)[2]
    print("logo intact" if bytes(got[:len(logo)]) == logo else "LOGO DIFFERS FROM FILE")
cps.end()
print("done; radio restarting")
