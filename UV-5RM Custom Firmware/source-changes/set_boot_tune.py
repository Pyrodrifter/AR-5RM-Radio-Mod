"""Replace the start-up tune in the radio's settings, leaving every other setting untouched.

  set_boot_tune.py COM3 BACKUP.bin
Backs up the 168-byte settings record first, then reads it back and confirms only the tune changed.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SETTINGS_ADDR, SETTINGS_BYTES, TUNE_OFFSET, TUNE_PAIRS = 0x9000, 168, 72, 48
READ, WRITE = 0x52, 0x57

# Indexes into the firmware's note table, one step per semitone: 8 E3, 20 E4, 32 E5.
LOW, HIGH, D, C, BB, B = 20, 32, 30, 28, 26, 27
RIFF = [LOW, LOW, HIGH, LOW, LOW, D, LOW, LOW, C, LOW, LOW, BB, LOW, LOW, B, C]
NOTE, GAP, LAST = 4, 1, 6  # in units of 30 ms

tune = []
for note in RIFF[:-1]:
    tune += [(note, NOTE), (0, GAP)]
tune.append((RIFF[-1], LAST))
assert len(tune) <= TUNE_PAIRS and all(0 <= n <= 45 and d > 0 for n, d in tune)
tune_bytes = bytes(b for pair in tune for b in pair).ljust(TUNE_PAIRS * 2, b"\0")
millis = sum(d for _, d in tune) * 30
print(f"tune: {len(tune)} steps, {millis / 1000:.2f} seconds")

port, backup = sys.argv[1:3]
cps = Cps(port)
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
same = check[:TUNE_OFFSET] == before[:TUNE_OFFSET] and check[TUNE_OFFSET + len(tune_bytes):] == before[TUNE_OFFSET + len(tune_bytes):]
print("tune written and verified" if same else "UNEXPECTED: other settings changed")
print("other settings unchanged" if same else "")
cps.end()
print("done; radio restarting")
