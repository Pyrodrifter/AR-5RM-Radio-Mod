"""Give a freshly flashed radio the skull logo and the standard apps, in menu order.

  setup_radio.py COM3 LOGO.rgb565 CHUNK APPS_DIR
Writes only what differs, so it's safe to run again after an interruption. Reads everything
back in full. Doesn't touch channels or settings.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SLOT_BASE = 0x14C000
LAYOUT = ["settings.app", "chanmgr.app", "searchqt.app", "spectrum.app",
          "bandsweep.app", "foxhunt.app", "rptfinder.app", "text.app", "fm.app"]

port, logo_path, raw_chunk, apps_dir = sys.argv[1:]
raw_chunk = int(raw_chunk)
assert len(LAYOUT) == SLOT_COUNT
apps = [open(os.path.join(apps_dir, name), "rb").read() for name in LAYOUT]
logo = open(logo_path, "rb").read()
assert len(logo) == LOGO_SIZE, len(logo)
assert all(a[:4] == b"OVL1" and len(a) <= SLOT_SIZE for a in apps)
cps = Cps(port)


def read_raw(addr, length):
    assert addr % raw_chunk == 0
    out = bytearray()
    for i in range(-(-length // raw_chunk)):
        out += cps.send(READ_RAW, addr // raw_chunk + i)[2]
    return bytes(out[:length])


def slot_addr(slot):
    return SLOT_BASE + slot * SLOT_SIZE


def name_of(data):
    return data[12:28].split(b"\0")[0].decode("ascii", "replace")


def read_logo():
    got = bytearray()
    for off in range(0, LOGO_SIZE, CHUNK):
        got += cps.send(READ_LOGO, off)[2]
    return bytes(got[:LOGO_SIZE])


if read_logo() == logo:
    print("logo already in place")
else:
    for off in range(0, LOGO_SIZE, CHUNK):
        cps.send(WRITE_LOGO, off, logo[off:off + CHUNK], ERASE_TIMEOUT if off == 0 else FRAME_TIMEOUT)
    print("logo written")

for slot, data in enumerate(apps):
    if read_raw(slot_addr(slot), len(data)) == data:
        print(f"slot {slot}: {name_of(data)} already in place")
        continue
    cps.send(APP_ERASE, slot, b"", ERASE_TIMEOUT)
    padded = data.ljust(-(-len(data) // CHUNK) * CHUNK, b"\xff")
    for off in range(0, len(padded), CHUNK):
        cps.send(APP_WRITE, off, padded[off:off + CHUNK])
    print(f"slot {slot}: wrote {name_of(data)}, {len(data)} bytes")

print("read back in full:")
if read_logo() != logo:
    raise SystemExit("LOGO VERIFY FAILED")
print("  logo verified")
for slot, data in enumerate(apps):
    if read_raw(slot_addr(slot), len(data)) != data:
        raise SystemExit(f"SLOT {slot} VERIFY FAILED")
    print(f"  {slot + 1}. {name_of(data)}  verified")
cps.end()
print("done; radio restarting")
