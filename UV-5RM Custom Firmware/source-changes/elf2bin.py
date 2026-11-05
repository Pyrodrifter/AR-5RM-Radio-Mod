"""Flatten an ARM ELF into the raw image at 0x08001000 and report sizes (no external tools)."""
import struct, sys

path, out = sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None
BASE, LIMIT = 0x08001000, 60 * 1024
d = open(path, "rb").read()
assert d[:4] == b"\x7fELF" and d[4] == 1, "expected ELF32"
phoff, shoff = struct.unpack_from("<II", d, 28)
phentsize, phnum, shentsize, shnum, shstrndx = struct.unpack_from("<HHHHH", d, 42)

secs = []
stroff = struct.unpack_from("<I", d, shoff + shstrndx * shentsize + 16)[0]
for i in range(shnum):
    name, typ, flags, addr, off, size, _, _, align, _ = struct.unpack_from("<10I", d, shoff + i * shentsize)
    nm = d[stroff + name:d.index(b"\0", stroff + name)].decode()
    if flags & 2 and size:
        secs.append((nm, addr, size, align, typ))
print("alloc sections:")
for nm, addr, size, align, typ in secs:
    print(f"  {nm:18} addr={addr:#010x} size={size:6} align={align} {'NOBITS' if typ == 8 else ''}")

image = bytearray()
for i in range(phnum):
    typ, off, vaddr, paddr, filesz, memsz, flags, align = struct.unpack_from("<8I", d, phoff + i * phentsize)
    if typ != 1 or filesz == 0:
        continue
    if paddr < BASE:  # linker's own ELF-header segment; nothing the radio loads
        assert filesz < 0x1000, hex(paddr)
        continue
    rel = paddr - BASE
    if len(image) < rel:
        image += b"\xff" * (rel - len(image))
    image[rel:rel + filesz] = d[off:off + filesz]
sp, reset = struct.unpack_from("<II", image, 0)
print(f"image {len(image)} bytes of {LIMIT} ({LIMIT - len(image)} free); SP={sp:#x} reset={reset:#x}")
assert len(image) <= LIMIT, "too big"
assert 0x20000000 < sp <= 0x20004000 and BASE <= reset < BASE + LIMIT and reset & 1
if out:
    open(out, "wb").write(image)
    print("wrote", out)
