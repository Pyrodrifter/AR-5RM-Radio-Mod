// Run with node --test cps_tests.js after generating AR-5RM_CPS.html.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");

const page = fs.readFileSync(path.resolve(__dirname, "../../AR-5RM_CPS.html"), "utf8");
const script = page.match(/<script>([\s\S]*?)<\/script>/)[1].split("/* ---------- wiring")[0];
const fakeRadio = fs.readFileSync(path.join(__dirname, "uploader_tests.js"), "utf8").split("(async function")[0];

function run(body) {
  const controls = new Map();
  const document = { getElementById(id) {
    if (!controls.has(id)) controls.set(id, {
      value: "", classList: { toggle() {} },
      getContext: () => ({
        clearRect() {}, putImageData() {},
        createImageData: (w, h) => ({ data: new Uint8ClampedArray(w * h * 4) }),
      }),
    });
    return controls.get(id);
  } };
  const ctx = vm.createContext({
    assert, document, navigator: { serial: {} }, performance, setTimeout, clearTimeout,
    TextEncoder, ReadableStream, WritableStream, atob, btoa,
  });
  return vm.runInContext(`
    ${script}
    ${fakeRadio}
    class AppearanceRadio extends FakeRadio {
      constructor(independent) {
        super(new Uint8Array());
        this.independent = independent;
        this.startup = new Uint8Array(ART_BYTES).fill(0x27);
        this.background = new Uint8Array(ART_BYTES).fill(0x19);
        this.settings = new Uint8Array(SETTINGS_BYTES).fill(0xF1);
        this.settings[BOOT_THEME_BYTE] = 6;
        this.artWrites = [];
        this.settingsWrites = 0;
        this.corruptArtwork = false;
        this.backgroundPresent = true;
      }
      handle(cmd, addr, data, crc, body) {
        assert.equal(crc, crc16(body));
        if (addr === SETTINGS_ADDR) {
          if (cmd === CMD_READ) return this.reply(this.frame(cmd, addr, this.settings));
          if (cmd === CMD_WRITE) {
            assert.equal(data.length, SETTINGS_BYTES);
            this.settings.set(data);
            this.settingsWrites++;
            return this.reply(this.frame(cmd, addr));
          }
        }
        for (const [kind, [read, write]] of Object.entries(ART_COMMANDS)) {
          if (cmd !== read && cmd !== write) continue;
          if (kind === "background" && !this.independent) break;
          if (kind === "background" && addr === 0xFFFF) {
            if (cmd === read) return this.reply(this.frame(cmd, addr, [Number(this.backgroundPresent)]));
            assert.equal(data.length, 0);
            this.backgroundPresent = false;
            return this.reply(this.frame(cmd, addr));
          }
          if (kind === "background" && cmd === write && addr === 0xFFFE) {
            assert.equal(data.length, 0);
            this.backgroundPresent = true;
            return this.reply(this.frame(cmd, addr));
          }
          assert.ok(addr < ART_BYTES && addr % ART_CHUNK === 0);
          if (cmd === read) return this.reply(this.frame(cmd, addr, this[kind].subarray(addr, addr + ART_CHUNK)));
          assert.equal(data.length, ART_CHUNK);
          assert.ok(downloads.length, "backup must precede artwork writes");
          if (addr === 0) {
            this[kind].fill(0xFF);
            if (kind === "background") this.backgroundPresent = false;
          }
          this[kind].set(data, addr);
          if (this.corruptArtwork && addr === 0) this[kind][0] ^= 1;
          this.artWrites.push(kind);
          return this.reply(this.frame(cmd, addr));
        }
        return super.handle(cmd, addr, data, crc, body);
      }
    }
    const downloads = [];
    saveFile = (name, data) => downloads.push({ name, data });
    (async () => { ${body} })();
  `, ctx);
}

test("RTTTL import keeps notes, rests and dotted timing", () => run(`
  const tune = importRtttl("sample:d=4,o=4,b=120:a,p,8a.");
  assert.equal(tune, "25, 17\\n0, 17\\n25, 13");
  assert.throws(() => importRtttl("sample:d=4,o=8,b=120:c"), /outside/);
  assert.throws(() => importRtttl("sample:d=4,o=4,b=120:" + Array(49).fill("a").join(",")), /48/);
`));

test("all built-in RTTTL tunes fit the radio", () => run(`
  assert.equal(Object.keys(TUNE_PRESETS).length, 5);
  for (const preset of Object.values(TUNE_PRESETS)) {
    const tune = importRtttl(preset);
    assert.ok(tune.length > 0);
    assert.ok(tune.split("\\n").length <= 48);
    const encoded = parseBootTune(tune);
    assert.equal(encoded.length, TUNE_BYTES);
    assert.ok(encoded.some(byte => byte !== 0));
  }
`));

test("MIDI import respects tempo and running status", () => run(`
  const midi = Uint8Array.from([
    77,84,104,100, 0,0,0,6, 0,0, 0,1, 0,96,
    77,84,114,107, 0,0,0,18,
    0,0xff,0x51,3,0x07,0xa1,0x20,
    0,0x90,69,100, 96,69,0,
    0,0xff,0x2f,0
  ]);
  assert.equal(importMidi(midi.buffer), "25, 17");
  assert.throws(() => importMidi(midi.slice(0, 18).buffer), /track|Truncated/);
`));

test("legacy firmware gates themes and background, and verifies the shared artwork", () => run(`
  port = new AppearanceRadio(false);
  await readAppearance();
  assert.equal(appearance.independent, false);
  assert.ok($("radioTheme").disabled && $("backgroundFile").disabled);
  assert.equal(appearance.startup, appearance.background);
  pendingArt.background = { bytes: new Uint8Array(ART_BYTES), name: "unsupported" };
  await writeAppearance();
  assert.equal(port.artWrites.length, 0);
  assert.match($("appearanceStatus").textContent, /shared artwork/);
  const pixels = new Uint8Array(ART_BYTES).fill(0x31);
  pendingArt = { startup: { bytes: pixels, name: "startup" } };
  await writeAppearance();
  assert.ok(sameBytes(port.startup, pixels));
  assert.equal(port.artWrites.length, 80);
  assert.ok(port.artWrites.every(kind => kind === "startup"));
  assert.equal(downloads.length, 1);
  assert.equal(loadAppearanceBackup(JSON.parse(downloads[0].data)).images.startup.bytes[0], 0x27);
`));

test("extended firmware changes the background and theme independently", () => run(`
  port = new AppearanceRadio(true);
  await readAppearance();
  assert.equal(appearance.theme, 1);
  assert.equal($("radioTheme").disabled, false);
  const original = port.settings.slice();
  $("radioTheme").value = "2";
  const pixels = new Uint8Array(ART_BYTES).fill(0x77);
  pendingArt = { background: { bytes: pixels, name: "background" } };
  await writeAppearance();
  assert.ok(sameBytes(port.background, pixels));
  assert.ok(port.startup.every(byte => byte === 0x27));
  original[BOOT_THEME_BYTE] = 42;
  assert.ok(sameBytes(port.settings, original));
  assert.equal(port.settingsWrites, 1);
  const backup = loadAppearanceBackup(JSON.parse(downloads[0].data));
  assert.equal(backup.theme, 1);
  assert.equal(backup.images.background.bytes[0], 0x19);
`));

test("read-back detects corrupted artwork and leaves the backup available", () => run(`
  port = new AppearanceRadio(true);
  await readAppearance();
  port.corruptArtwork = true;
  pendingArt = { startup: { bytes: new Uint8Array(ART_BYTES), name: "startup" } };
  await writeAppearance();
  assert.match($("appearanceStatus").textContent, /Read-back failed/);
  assert.equal(downloads.length, 1);
  assert.ok(pendingArt.startup);
`));

test("the last partial sector verifies channel 1000 without requesting reserved records", () => run(`
  const table = new Uint8Array(TABLE_BYTES).fill(0xFF);
  table[TABLE_BYTES - REC] = 0x12;
  port = new FakeRadio(new Uint8Array(TABLE_BYTES).fill(0xFF));
  await withLink(async link => {
    assert.equal((await writeSectors(link, table, [7])).length, 0);
    await link.end();
  });
  assert.equal(port.writes, 104);
  assert.ok(sameBytes(port.flash.subarray(0, TABLE_BYTES), table));
`));

test("startup tune edits preserve other settings and back up the old tune", () => run(`
  port = new AppearanceRadio(false);
  port.settings[BOOT_THEME_BYTE] = 2;
  port.settings[BOOT_SOUND_BYTE] = 1;
  await readAppearance();
  const original = port.settings.slice();
  $("bootTune").value = "20, 4\\n0, 1\\n32, 6";
  tuneDirty = true;
  $("bootSound").value = "0";
  await writeAppearance();
  assert.equal(port.settingsWrites, 1);
  original[BOOT_SOUND_BYTE] = 0;
  original.set(parseBootTune("20, 4\\n0, 1\\n32, 6"), TUNE_OFFSET);
  assert.ok(sameBytes(port.settings, original));
  const backup = loadAppearanceBackup(JSON.parse(downloads[0].data));
  assert.equal(backup.sound, 1);
  assert.equal(backup.tune.length, TUNE_BYTES);
  assert.throws(() => parseBootTune("46, 1"), /note 0/);
  assert.throws(() => parseBootTune("1, 0"), /duration/);
`));

test("sun mode preserves the startup display setting", () => run(`
  port = new AppearanceRadio(true);
  await readAppearance();
  $("radioTheme").value = "3";
  await writeAppearance();
  assert.equal(port.settings[BOOT_THEME_BYTE], 14);
  assert.equal(port.settingsWrites, 1);
  assert.equal(loadAppearanceBackup(JSON.parse(downloads[0].data)).theme, 1);
`));

test("white backgrounds are committed and backups can clear them", () => run(`
  port = new AppearanceRadio(true);
  port.background.fill(0xFF);
  port.backgroundPresent = false;
  await readAppearance();
  pendingArt = { background: { bytes: new Uint8Array(ART_BYTES).fill(0xFF), name: "white" } };
  await writeAppearance();
  assert.equal(port.backgroundPresent, true);
  const backup = loadAppearanceBackup(JSON.parse(downloads[0].data));
  assert.equal(backup.backgroundPresent, false);
  assert.equal(backup.images.background.bytes[0], 0xFF);
  pendingBackgroundClear = true;
  await writeAppearance();
  assert.equal(port.backgroundPresent, false);
`));

test("failed background verification leaves it inactive", () => run(`
  port = new AppearanceRadio(true);
  await readAppearance();
  port.corruptArtwork = true;
  pendingArt = { background: { bytes: new Uint8Array(ART_BYTES), name: "background" } };
  await writeAppearance();
  assert.match($("appearanceStatus").textContent, /Read-back failed/);
  assert.equal(port.backgroundPresent, false);
  assert.equal(downloads.length, 1);
`));

test("theme backups preserve the last dark palette when restoring Sun", () => run(`
  port = new AppearanceRadio(true);
  port.settings[BOOT_THEME_BYTE] = 0x2E;
  await readAppearance();
  $("radioTheme").value = "1";
  await writeAppearance();
  assert.equal(port.settings[BOOT_THEME_BYTE], 0x16);
  const backup = loadAppearanceBackup(JSON.parse(downloads[0].data));
  assert.equal(backup.theme, 3);
  assert.equal(backup.darkTheme, 2);
  $("radioTheme").value = String(backup.theme);
  pendingDarkTheme = backup.darkTheme;
  await writeAppearance();
  assert.equal(port.settings[BOOT_THEME_BYTE], 0x2E);
`));
