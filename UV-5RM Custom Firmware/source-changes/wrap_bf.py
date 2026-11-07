"""Wrap a raw app image as a one-region .BF, using the rule from uv5rm-wrap-tool's main.c.

Packages 0, 1 and the last two stay plain; the rest XOR with KDHT/RBGI by index % 3.
Checks the implementation against the factory images before trusting it.
"""
import glob, os, struct, sys

K1, K2, PKG = b"KDHT", b"RBGI", 1024


def xor(data, key):
    out = bytearray()
    for i, x in enumerate(data):
        k = key[i % 4]
        out.append(x ^ k if x not in (0, 0xFF) and x != k and x != k ^ 0xFF else x)
    return bytes(out)


def crypt(data):
    n = len(data) // PKG + (1 if len(data) % PKG else 0)
    out = bytearray()
    for i in range(n):
        block = data[i * PKG:(i + 1) * PKG]
        if 2 <= i < n - 2 and i % 3 == 1:
            block = xor(block, K1)
        elif 2 <= i < n - 2 and i % 3 == 2:
            block = xor(block, K2)
        out += block
    return bytes(out)


def wrap(image):
    """Encode the application bytes the bootloader expects in a .BF file."""
    assert 8 <= len(image) <= 0xF000, len(image)
    sp, reset = struct.unpack_from("<II", image)
    assert 0x20000000 < sp <= 0x20004000 and 0x08001000 < reset < 0x08010000 and reset & 1
    encoded = crypt(image)
    assert crypt(encoded) == image
    return struct.pack(">BLL7x", 1, len(encoded), 0) + encoded


def main():
    image_path, out_path, stock_dir = sys.argv[1:4]

    # The transform must undo itself and turn factory files into real firmware.
    checked = 0
    for bf in sorted(glob.glob(os.path.join(stock_dir, "*", "*.BF"))):
        with open(bf, "rb") as f:
            raw = f.read()
        _, size, _ = struct.unpack(">BLL", raw[:9])
        encoded = raw[16:16 + size]
        decoded = crypt(encoded)
        assert crypt(decoded) == encoded, bf
        sp, reset = struct.unpack_from("<II", decoded)
        assert 0x20000000 < sp <= 0x20004000 and 0x08001000 < reset < 0x08010000, bf
        assert b"PROGRAMBFNORMALU" in decoded and b"VER:%s" in decoded, bf
        checked += 1
    assert checked >= 5, checked
    print("crypto verified against", checked, "factory images")

    with open(image_path, "rb") as f:
        image = f.read()
    with open(out_path, "wb") as f:
        f.write(wrap(image))
    packages = -(-len(image) // PKG)
    sp, reset = struct.unpack_from("<II", image)
    print(f"wrote {out_path}: {len(image)} bytes, {packages} packages, SP={sp:#x} reset={reset:#x}")


if __name__ == "__main__":
    main()
