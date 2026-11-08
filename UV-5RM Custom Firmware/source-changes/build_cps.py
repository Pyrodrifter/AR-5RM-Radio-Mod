"""Build the offline Custom CPS page from the shared uploader template."""

import base64
import json
import re
from pathlib import Path


source_changes = Path(__file__).resolve().parent
firmware = source_changes.parent
settings = (firmware / "firmware-source/src/app/settings.rs").read_text(encoding="utf-8")
dcs_table = re.search(r"DCS_TABLE\s*:[^=]*=\s*\[(.*?)\];", settings, re.S)
dcs = re.findall(r"0o(\d+)", dcs_table.group(1))
assert len(dcs) == 105 and dcs[33] == "205"

page = (source_changes / "uploader_template.html").read_text(encoding="utf-8")
page = page.replace('url("__SKULL__")', "none")
page = page.replace("__CSV__", '""').replace("__SECTIONS__", "[]")
page = page.replace("__DCS__", json.dumps(dcs))
text_app = (firmware / 'apps/text.app').read_bytes()
page = page.replace('__TEXT_APP__', base64.b64encode(text_app).decode('ascii'))
watch_app = (firmware / 'apps/radio watch.app').read_bytes()
page = page.replace('__WATCH_APP__', base64.b64encode(watch_app).decode('ascii'))
sat_app = (firmware / 'apps/satellite rx.app').read_bytes()
page = page.replace('__SAT_RX_APP__', base64.b64encode(sat_app).decode('ascii'))
vendor = source_changes / 'vendor'
orbit = '/*\n' + (vendor / 'satellite-LICENSE.md').read_text(encoding='utf-8') + '\n*/\n'
orbit += (vendor / 'satellite-5.0.0.min.js').read_text(encoding='utf-8')
page = page.replace('/* __ORBIT_LIBRARY__ */', orbit)

output = firmware.parent / "AR-5RM_CPS.html"
output.write_text(page, encoding="utf-8", newline="\n")
print(f"Wrote {output}")
