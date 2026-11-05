"""Put the installed apps in the menu order below. The menu shows slots in order.

  arrange_apps.py COM3 LOGO.rgb565 CHUNK APPS_DIR OLD_DIR
Stops before writing unless every slot holds what's expected. Rewrites only the slots that differ,
then reads all eight back in full. Running it again after an interruption finishes the job.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SLOT_BASE = 0x14C000

# (file there now, file it should hold); old names are in OLD_DIR when marked.
LAYOUT = [
    ("settings.app", "settings.app"),
    ("old:chanmgr.app", "chanmgr.app"),
    ("old:searchqt.app", "searchqt.app"),
    ("fm.app", "spectrum.app"),
    ("spectrum.app", "bandsweep.app"),
    ("foxhunt.app", "foxhunt.app"),
    ("rptfinder.app", "rptfinder.app"),
    ("bandsweep.app", "fm.app"),
]

port, logo_path, raw_chunk, apps_dir, old_dir = sys.argv[1:]
raw_chunk = int(raw_chunk)


def load(ref):
    return open(os.path.join(old_dir, ref[4:]) if ref.startswith("old:") else os.path.join(apps_dir, ref), "rb").read()


def name_of(data):
    return data[12:28].split(b"\0")[0].decode("ascii", "replace") if data[:4] == b"OVL1" else "(empty)"


plan = [(load(old), load(new)) for old, new in LAYOUT]
assert len(plan) <= SLOT_COUNT
assert all(new[:4] == b"OVL1" and len(new) <= SLOT_SIZE for _, new in plan)
cps = Cps(port)


def read_raw(addr, length):
    assert addr % raw_chunk == 0
    out = bytearray()
    for i in range(-(-length // raw_chunk)):
        out += cps.send(READ_RAW, addr // raw_chunk + i)[2]
    return bytes(out[:length])


def slot_addr(slot):
    return SLOT_BASE + slot * SLOT_SIZE


todo = []
for slot, (old, new) in enumerate(plan):
    now = read_raw(slot_addr(slot), max(len(old), len(new)))
    if now.startswith(new):
        print(f"slot {slot}: {name_of(new)} already in place")
    elif now.startswith(old) or now[:4] == b"\xff\xff\xff\xff" or name_of(now) == name_of(new):
        print(f"slot {slot}: {name_of(now)} -> {name_of(new)}")
        todo.append(slot)
    else:
        raise SystemExit(f"slot {slot} holds unexpected {name_of(now)}; NOTHING WAS CHANGED")

for slot in todo:
    data = plan[slot][1]
    cps.send(APP_ERASE, slot, b"", ERASE_TIMEOUT)
    padded = data.ljust(-(-len(data) // CHUNK) * CHUNK, b"\xff")
    for off in range(0, len(padded), CHUNK):
        cps.send(APP_WRITE, off, padded[off:off + CHUNK])
    print(f"slot {slot}: wrote {name_of(data)}, {len(data)} bytes")

print("menu on the radio, read back in full:")
for slot, (_, new) in enumerate(plan):
    assert read_raw(slot_addr(slot), len(new)) == new, f"slot {slot}: read-back differs"
    print(f"  {slot + 1}. {name_of(new)}  verified")

logo = open(logo_path, "rb").read()
got = bytearray()
for off in range(0, len(logo), CHUNK):
    got += cps.send(READ_LOGO, off)[2]
print("logo intact" if bytes(got[:len(logo)]) == logo else "LOGO DIFFERS FROM FILE")
cps.end()
print("done; radio restarting")
