"""Fit the skull image to the 160x128 screen on pure black; write raw RGB565 plus a preview."""
import struct, sys
from PIL import Image

src, out_prefix = sys.argv[1], sys.argv[2]
W, H = 160, 128

img = Image.open(src).convert("RGB")
# The source background is a flat dark maroon (26,0,1). Pull that level to black.
bg = img.getpixel((5, 5))
print("source size", img.size, "background", bg)
lut = [max(0, c - bg[0]) * 255 // (255 - bg[0]) for c in range(256)]
img = img.point(lut * 3)

scale = min(W / img.width, H / img.height)
size = (round(img.width * scale), round(img.height * scale))
fit = img.resize(size, Image.LANCZOS)
canvas = Image.new("RGB", (W, H), (0, 0, 0))
canvas.paste(fit, ((W - size[0]) // 2, (H - size[1]) // 2))

raw = bytearray()
for y in range(H):
    for x in range(W):
        r, g, b = canvas.getpixel((x, y))
        raw += struct.pack("<H", (r >> 3) << 11 | (g >> 2) << 5 | b >> 3)
assert len(raw) == W * H * 2
open(out_prefix + ".rgb565", "wb").write(raw)
canvas.resize((W * 4, H * 4), Image.NEAREST).save(out_prefix + "_preview.png")
print("wrote", len(raw), "bytes; corners", canvas.getpixel((0, 0)), canvas.getpixel((W // 2, 20)))
