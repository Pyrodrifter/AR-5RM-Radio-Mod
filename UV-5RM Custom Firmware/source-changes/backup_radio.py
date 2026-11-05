"""Read-only: save every channel, the settings and the names of the installed apps.

  backup_radio.py COM3 FOLDER
Writes radio_channels.bin, radio_settings.bin and radio_apps.txt, then restarts the radio.
Run with the radio on at its main screen. Flash mode can't read these.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

READ, SETTINGS_ADDR, SETTINGS_BYTES, CHANNELS, RECORD, RAW_CHUNK, SLOT_BASE = 0x52, 0x9000, 168, 1000, 32, 64, 0x14C000

port, folder = sys.argv[1:3]
os.makedirs(folder, exist_ok=True)
cps = Cps(port)

channels = bytearray()
for n in range(CHANNELS):
    record = cps.send(READ, n * RECORD)[2]
    assert len(record) == RECORD, (n, len(record))
    channels += record
settings = cps.send(READ, SETTINGS_ADDR)[2]
assert len(settings) == SETTINGS_BYTES, len(settings)

names = []
for slot in range(SLOT_COUNT):
    head = cps.send(READ_RAW, (SLOT_BASE + slot * SLOT_SIZE) // RAW_CHUNK)[2]
    names.append(head[12:28].split(b"\0")[0].decode("ascii", "replace") if head[:4] == b"OVL1" else "(empty)")
# Is anything already where the ninth app slot and TEXT's message log will go?
for label, start, size in [("ninth app slot", SLOT_BASE + 8 * SLOT_SIZE, SLOT_SIZE), ("message log", 0x1F0000, 0x4000)]:
    used = 0
    for off in range(0, size, RAW_CHUNK):
        used += any(b != 0xFF for b in cps.send(READ_RAW, (start + off) // RAW_CHUNK)[2])
    print(f"{label} at {start:#x}: " + ("blank" if not used else f"NOT BLANK, {used} of {size // RAW_CHUNK} blocks in use"))
cps.end()

open(os.path.join(folder, "radio_channels.bin"), "wb").write(channels)
open(os.path.join(folder, "radio_settings.bin"), "wb").write(settings)
open(os.path.join(folder, "radio_apps.txt"), "w", encoding="utf-8").write("\n".join(names) + "\n")
used = sum(channels[n * RECORD:(n + 1) * RECORD] != b"\xff" * RECORD for n in range(CHANNELS))
print(f"saved {used} channels, settings and app list to {folder}")
for slot, name in enumerate(names):
    print(f"  {slot + 1}. {name}")
print("done; radio restarting")
