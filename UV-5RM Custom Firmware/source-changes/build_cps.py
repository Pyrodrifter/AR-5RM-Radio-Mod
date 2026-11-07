"""Build the offline Custom CPS page from the shared uploader template."""

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

output = firmware.parent / "AR-5RM_CPS.html"
output.write_text(page, encoding="utf-8", newline="\n")
print(f"Wrote {output}")
