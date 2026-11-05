"""Set radio two's start-up tune to the first seven notes of the Super Mario theme.

  set_radio2_mario_tune.py COM3 BACKUP.bin
Refuses unless the connected radio is radio two, recognized by its empty channel 196, which
radio one has filled. Backs up the settings first and changes only the tune bytes.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SETTINGS_ADDR, SETTINGS_BYTES, TUNE_OFFSET, TUNE_PAIRS = 0x9000, 168, 72, 48
READ, WRITE = 0x52, 0x57
RADIO_TWO_EMPTY_CHANNEL = 196

# Indexes into the firmware's note table, one step per semitone from A2: 23 G4, 28 C5, 32 E5, 35 G5.
G4, C5, E5, G5 = 23, 28, 32, 35
REST = 0
EIGHTH, SOUND, GAP, LAST = 5, 4, 1, 8  # in units of 30 ms
# One entry per eighth note of the opening bar and a half.
MELODY = [E5, E5, REST, E5, REST, C5, E5, REST, G5, REST, REST, REST, G4]

tune = []
for note in MELODY[:-1]:
    if note == REST:
        tune.append((0, EIGHTH))
    else:
        tune += [(note, SOUND), (0, GAP)]
tune.append((MELODY[-1], LAST))
assert len(tune) <= TUNE_PAIRS and all(0 <= n <= 45 and d > 0 for n, d in tune)
assert sum(1 for n, _ in tune if n) == 7, "seven notes"
tune_bytes = bytes(b for pair in tune for b in pair).ljust(TUNE_PAIRS * 2, b"\0")
print(f"tune: {len(tune)} steps, {sum(d for _, d in tune) * 30 / 1000:.2f} seconds")

port, backup = sys.argv[1:3]
cps = Cps(port)

channel = cps.send(READ, (RADIO_TWO_EMPTY_CHANNEL - 1) * 32)[2]
if channel != b"\xff" * 32:
    name = channel[20:32].split(b"\xff")[0].decode("ascii", "replace")
    cps.end()
    raise SystemExit(f"THIS IS NOT RADIO TWO: channel {RADIO_TWO_EMPTY_CHANNEL} holds {name!r}. Nothing changed.")
print(f"radio two confirmed: channel {RADIO_TWO_EMPTY_CHANNEL} is empty")

before = cps.send(READ, SETTINGS_ADDR)[2]
assert len(before) == SETTINGS_BYTES, len(before)
open(backup, "wb").write(before)
print(f"settings backed up to {os.path.basename(backup)}")
print("boot sound on:", before[31] != 0)

after = before[:TUNE_OFFSET] + tune_bytes + before[TUNE_OFFSET + len(tune_bytes):]
assert len(after) == SETTINGS_BYTES
cps.send(WRITE, SETTINGS_ADDR, after)

check = cps.send(READ, SETTINGS_ADDR)[2]
if check != after:
    print("READ-BACK DIFFERS; restoring the backup")
    cps.send(WRITE, SETTINGS_ADDR, before)
    cps.end()
    raise SystemExit(1)
print("tune written and verified; other settings unchanged")
cps.end()
print("done; radio restarting")
