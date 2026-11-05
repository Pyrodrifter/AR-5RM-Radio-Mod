"""Install apps into empty slots only, read every byte back, and confirm the logo and other apps are intact.

  install_new_apps.py COM3 LOGO.rgb565 CHUNK FILE.app [...]
Refuses before writing anything if there aren't enough empty slots.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SLOT_BASE = 0x14C000

port, logo_path, raw_chunk, *apps = sys.argv[1:]
raw_chunk = int(raw_chunk)
cps = Cps(port)


def read_raw(addr, length):
    assert addr % raw_chunk == 0
    out = bytearray()
    for i in range(-(-length // raw_chunk)):
        out += cps.send(READ_RAW, addr // raw_chunk + i)[2]
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
    else:
        free.append(slot)
        print(f"slot {slot}: empty")

wanted = {open(a, "rb").read()[12:28].split(b"\0")[0].decode(): a for a in apps}
already = {name: slot for slot, (name, _) in before.items() if name in wanted}
targets = dict(already)
for name in wanted:
    if name not in targets:
        if not free:
            raise SystemExit(f"NO EMPTY SLOT for {name}; nothing else was changed")
        targets[name] = free.pop(0)

for name, path in wanted.items():
    slot = targets[name]
    data = open(path, "rb").read()
    assert data[:4] == b"OVL1" and len(data) <= SLOT_SIZE
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

logo = open(logo_path, "rb").read()
got = bytearray()
for off in range(0, len(logo), CHUNK):
    got += cps.send(READ_LOGO, off)[2]
print("logo intact" if bytes(got[:len(logo)]) == logo else "LOGO DIFFERS FROM FILE")
cps.end()
print("done; radio restarting")
