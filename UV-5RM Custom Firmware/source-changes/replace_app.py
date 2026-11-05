"""Replace one installed app, refusing unless the slot holds the app named. Verifies the result.

  replace_app.py COM3 LOGO.rgb565 CHUNK OLD_NAME FILE.app
Reads every slot first, reads the new app back, and confirms other apps and the logo are unchanged.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SLOT_BASE = 0x14C000

port, logo_path, raw_chunk, old_name, app_path = sys.argv[1:]
raw_chunk = int(raw_chunk)
data = open(app_path, "rb").read()
assert data[:4] == b"OVL1" and len(data) <= SLOT_SIZE
new_name = data[12:28].split(b"\0")[0].decode()
cps = Cps(port)


def read_raw(addr, length):
    assert addr % raw_chunk == 0
    out = bytearray()
    for i in range(-(-length // raw_chunk)):
        out += cps.send(READ_RAW, addr // raw_chunk + i)[2]
    return bytes(out[:length])


def slot_addr(slot):
    return SLOT_BASE + slot * SLOT_SIZE


heads = {}
target = None
for slot in range(SLOT_COUNT):
    head = read_raw(slot_addr(slot), raw_chunk)
    name = head[12:28].split(b"\0")[0].decode("ascii", "replace") if head[:4] == b"OVL1" else "(empty)"
    heads[slot] = head
    print(f"slot {slot}: {name}")
    if name == old_name:
        target = slot
    elif name == new_name:
        raise SystemExit(f"{new_name} is already in slot {slot}; nothing changed")
if target is None:
    raise SystemExit(f"NO SLOT HOLDS {old_name}; nothing changed")

cps.send(APP_ERASE, target, b"", ERASE_TIMEOUT)
padded = data.ljust(-(-len(data) // CHUNK) * CHUNK, b"\xff")
for off in range(0, len(padded), CHUNK):
    cps.send(APP_WRITE, off, padded[off:off + CHUNK])
assert read_raw(slot_addr(target), len(data)) == data, f"{new_name}: read-back differs"
print(f"slot {target}: {old_name} replaced by {new_name}, {len(data)} bytes written and verified")

for slot, head in heads.items():
    if slot != target:
        assert read_raw(slot_addr(slot), raw_chunk) == head, f"slot {slot} changed"
print("other apps unchanged")

logo = open(logo_path, "rb").read()
got = bytearray()
for off in range(0, len(logo), CHUNK):
    got += cps.send(READ_LOGO, off)[2]
print("logo intact" if bytes(got[:len(logo)]) == logo else "LOGO DIFFERS FROM FILE")
cps.end()
print("done; radio restarting")
