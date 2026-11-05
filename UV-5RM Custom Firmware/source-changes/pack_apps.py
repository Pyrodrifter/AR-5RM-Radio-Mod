"""Package built app ELFs as .app files. Same layout as tools/mkapp.py, parsing the ELF directly."""
import os, struct, sys, zlib

fw_root, elf_dir, out_dir = sys.argv[1:4]
MAGIC, NAME_LEN, MAX_SEGMENTS, HEADER_SIZE = b"OVL1", 16, 4, 108
ARENA_SIZE, ARENA_MAX, SLOT_SIZE, ORIGIN = 8192, 12288, 32 * 1024, 0x200001D0
PKG_FMT, SEG_FMT = f"<4sIB3x{NAME_LEN}s", "<IIIII"
APPS = [("SETTINGS", ["menu", "radio", "system"]), ("CHAN MGR", ["chanmgr"]), ("SEARCH", ["search"]),
        ("SCANQT", ["scanqt"]), ("SPECTRUM", ["spectrum"]), ("FM", ["fm"]),
        ("RPT FINDER", ["rptfind"]), ("BAND SWEEP", ["bandsweep"]), ("FOX HUNT", ["foxhunt"]),
        ("SEARCH/QT SCAN", ["qtscan2", "search2"]),
        ("TEXT", ["textapp"])]

lib = os.path.join(fw_root, "abi", "src", "lib.rs")
build_hash = zlib.crc32(open(lib, "rb").read()) & 0xFFFFFFFF


def read_elf(path):
    d = open(path, "rb").read()
    assert d[:4] == b"\x7fELF" and d[4] == 1
    shoff = struct.unpack_from("<I", d, 32)[0]
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", d, 46)
    sec = [struct.unpack_from("<10I", d, shoff + i * shentsize) for i in range(shnum)]
    strs = sec[shstrndx][4]
    name = lambda off, base: d[base + off:d.index(b"\0", base + off)].decode()
    image = bytearray()
    bss = 0
    for s in sec:
        _, typ, flags, addr, off, size, *_ = s
        if not flags & 2 or size == 0:
            continue
        if typ == 8:
            bss += size
            continue
        rel = addr - ORIGIN
        assert rel >= 0, (name(s[0], strs), hex(addr))
        if len(image) < rel:
            image += b"\0" * (rel - len(image))
        image[rel:rel + size] = d[off:off + size]
    entry = None
    for s in sec:
        if s[1] != 2:  # SHT_SYMTAB
            continue
        strtab = sec[s[6]][4]
        for i in range(s[5] // 16):
            st_name, st_value, *_ = struct.unpack_from("<IIIBBH", d, s[4] + i * 16)
            if name(st_name, strtab) == "app_entry":
                entry = st_value
    assert entry is not None, f"{path}: no app_entry"
    return bytes(image), bss, entry - ORIGIN


for title, parts in APPS:
    body, entries = bytearray(), []
    for part in parts:
        image, bss, entry_off = read_elf(os.path.join(elf_dir, part))
        total = len(image) + bss
        assert total <= ARENA_MAX, (part, total)
        body += b"\0" * (-len(body) % 4)
        offset = HEADER_SIZE + len(body)
        body += image
        entries.append(struct.pack(SEG_FMT, offset, len(image), bss, entry_off, zlib.crc32(image) & 0xFFFFFFFF))
        print(f"  {part}: image={len(image)} bss={bss} total={total}/{ARENA_SIZE} entry={entry_off:#x}")
    name_field = title.encode()[:NAME_LEN].ljust(NAME_LEN, b"\0")
    header = struct.pack(PKG_FMT, MAGIC, build_hash, len(parts), name_field) + b"".join(entries)
    header += b"\0" * 20 * (MAX_SEGMENTS - len(parts))
    assert len(header) == HEADER_SIZE
    pkg = header + bytes(body)
    assert len(pkg) <= SLOT_SIZE, (title, len(pkg))
    out = os.path.join(out_dir, title.lower().replace("/", " ") + ".app")
    open(out, "wb").write(pkg)
    print(f"{title}: {len(pkg)} bytes, {len(parts)} segment(s), hash {build_hash:#010x} -> {out}")
