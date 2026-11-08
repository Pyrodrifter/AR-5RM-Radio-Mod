"""Build the channel uploader page and a test copy that drives it against a simulated radio."""
import base64, io, json, os, re, sys
from PIL import Image

base, firmware_src, skull_path, dump_path = sys.argv[1:5]
here = os.path.dirname(os.path.abspath(__file__))


def embed(v):
    return json.dumps(v).replace("</", "<\\/")


txt = open(base + r"\Clark_County_NV_UV-5RM_Channels.txt", encoding="utf-8").read().splitlines()
csv_text = open(base + r"\Clark_County_NV_UV-5RM_CHIRP.csv", encoding="utf-8", newline="").read()
sections = []
for i, line in enumerate(txt):
    if line.startswith("APPENDIX"):
        break
    m = re.match(r"^(\d{1,2})\. (.+)$", line)
    if m and i > 0 and txt[i - 1].startswith("===="):
        sections.append({"title": f"{m.group(1)}. {m.group(2).strip()}", "channels": []})
    elif re.match(r"^\d{3}  ", line):
        sections[-1]["channels"].append(int(line[:3]))

settings = open(os.path.join(firmware_src, "src", "app", "settings.rs"), encoding="utf-8").read()
table = re.search(r"DCS_TABLE\s*:[^=]*=\s*\[(.*?)\];", settings, re.S).group(1)
dcs = re.findall(r"0o(\d+)", table)
assert len(dcs) == 105 and dcs[33] == "205", (len(dcs), dcs[33])

# Same black-level pull as the radio's logo, keeping the original resolution for large screens.
img = Image.open(skull_path).convert("RGB")
bg = img.getpixel((5, 5))[0]
lut = [max(0, c - bg) * 255 // (255 - bg) for c in range(256)]
img = img.point(lut * 3)
buf = io.BytesIO()
img.save(buf, "PNG", optimize=True)
skull = "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode()

page = open(os.path.join(here, "uploader_template.html"), encoding="utf-8").read()
page = page.replace("__SKULL__", skull).replace("__CSV__", embed(csv_text)).replace("__SECTIONS__", embed(sections)).replace("__DCS__", embed(dcs))
with open(os.path.join(here, "..", "apps", "text.app"), "rb") as text_app:
    page = page.replace("__TEXT_APP__", base64.b64encode(text_app.read()).decode("ascii"))
with open(os.path.join(here, "..", "apps", "radio watch.app"), "rb") as watch_app:
    page = page.replace("__WATCH_APP__", base64.b64encode(watch_app.read()).decode("ascii"))
with open(os.path.join(here, "..", "apps", "satellite rx.app"), "rb") as sat_app:
    page = page.replace("__SAT_RX_APP__", base64.b64encode(sat_app.read()).decode("ascii"))
with open(os.path.join(here, "vendor", "satellite-LICENSE.md"), encoding="utf-8") as license, \
     open(os.path.join(here, "vendor", "satellite-5.0.0.min.js"), encoding="utf-8") as library:
    page = page.replace("/* __ORBIT_LIBRARY__ */", "/*\n" + license.read() + "\n*/\n" + library.read())
out = os.path.join(base, "UV-5RM_Channel_Uploader.html")
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write(page)

dump = open(dump_path, "rb").read()[:32000]
assert len(dump) == 32000
tests = open(os.path.join(here, "uploader_tests.js"), encoding="utf-8").read()
inject = "<script>const RADIO_DUMP_B64 = " + embed(base64.b64encode(dump).decode()) + ";\n" + tests + "</script>\n</body>"
with open(os.path.join(here, "uploader_test.html"), "w", encoding="utf-8", newline="\n") as f:
    f.write(page.replace("</body>", '<pre id="testout"></pre>\n' + inject))
print("wrote", out, os.path.getsize(out), "bytes; skull", len(skull), "chars; sections", len(sections))
