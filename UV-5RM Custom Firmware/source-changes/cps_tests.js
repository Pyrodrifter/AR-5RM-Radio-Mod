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

test("bulk edit changes only the chosen fields and is all-or-nothing", () => run(`
  const mk = (num, freq, extra = {}) => ({ num, name: "CH" + num, freq, duplex: "", offset: "0", tone: "", rtone: "88.5", ctone: "88.5",
    dcs: "023", rxdcs: "023", pol: "NN", mode: "FM", skip: "", powerRaw: 2, powerText: "", note: "keep me", extra: null, csvRow: null, ...extra });
  const list = [mk(1, "462.562500"), mk(2, "146.520000", { tone: "Mixed", rawCodes: [1, 2] }), mk(3, "156.800000", { duplex: "off" })];
  assert.deepEqual(applyBulk(list, { tone: "TSQL", toneVal: "100.0", mode: "NFM", powerRaw: 1, scan: false }), []);
  for (const c of list) {
    assert.equal(c.tone, "TSQL"); assert.equal(c.ctone, "100.0"); assert.equal(c.mode, "NFM");
    assert.equal(c.powerRaw, 1); assert.equal(c.skip, "S"); assert.equal(c.note, "keep me"); assert.ok(c.edited);
  }
  assert.equal(list[1].rawCodes, undefined);
  assert.equal(list[2].duplex, "off");
  assert.equal(encode(list[0])[8] | encode(list[0])[9] << 8, 1000);
  const before = JSON.stringify(list);
  const errors = applyBulk(list, { duplex: "+", mode: "AM" });
  assert.equal(errors.length, 3);
  assert.match(errors[0], /offset/);
  assert.equal(JSON.stringify(list), before);
  assert.deepEqual(applyBulk(list, { tone: "DTCS", toneVal: "23" }), []);
  assert.equal(list[2].dcs, "023");
  assert.deepEqual(applyBulk(list, {}), []);
`));

test("RTTTL import keeps notes, rests and dotted timing", () => run(`
  const tune = importRtttl("sample:d=4,o=4,b=120:a,p,8a.");
  assert.equal(tune, "25, 17\\n0, 17\\n25, 13");
  assert.throws(() => importRtttl("sample:d=4,o=8,b=120:c"), /outside/);
  assert.throws(() => importRtttl("sample:d=4,o=4,b=120:" + Array(49).fill("a").join(",")), /48/);
`));

test("TEXT draft blocks round-trip eight 50-character messages and reject corruption", () => run(`
  const lines = Array.from({ length: 8 }, (_, i) => i + "A".repeat(49));
  const bytes = encodeDrafts(lines.join("\\r\\n"));
  assert.equal(decodeDrafts(bytes).join("|"), lines.join("|"));
  assert.equal(decodeDrafts(encodeDrafts("")).length, 0);
  assert.throws(() => encodeDrafts("A".repeat(51)), /1–50/);
  assert.throws(() => encodeDrafts("emoji ☺"), /keyboard/);
  assert.throws(() => encodeDrafts([...lines, "too many"].join("\\n")), /eight/);
  bytes[DRAFT_HEADER + 1] ^= 1;
  assert.throws(() => decodeDrafts(bytes), /damaged/);
`));

test("CPS backs up and verifies only the TEXT slot when uploading drafts", () => run(`
  const slot = 7, address = SLOT_BASE + slot * SLOT_BYTES;
  const installed = new Uint8Array(SLOT_BYTES).fill(255);
  const bundled = Uint8Array.from(atob(BUNDLED_TEXT), ch => ch.charCodeAt(0));
  installed.set(bundled);
  const untouched = DRAFTS_OFFSET - APP_BLOCK;
  installed[untouched] = 0x5a;
  const writes = [];
  const link = {
    async send(cmd, at, data) {
      if (cmd === FLASH_READ) {
        const offset = at * FLASH_BLOCK - address;
        const result = offset >= 0 && offset < SLOT_BYTES
          ? installed.slice(offset, offset + FLASH_BLOCK) : new Uint8Array(FLASH_BLOCK).fill(255);
        return { data: result };
      }
      if (cmd === APP_ERASE) {
        assert.equal(at, slot);
        assert.ok(downloads.length > 0);
        writes.push("erase");
        installed.fill(255);
      } else {
        assert.equal(cmd, APP_WRITE);
        assert.equal(data.length, APP_BLOCK);
        writes.push(at);
        installed.set(data, at);
      }
      return { data: new Uint8Array() };
    },
  };
  const original = installed.slice();
  const first = encodeDrafts("OK\\nON MY WAY");
  assert.equal(await writeDrafts(link, first, () => {}), true);
  assert.ok(sameBytes(downloads[0].data, original));
  assert.equal(writes.at(-1), 0);
  assert.equal(installed[untouched], 0x5a);
  assert.equal(decodeDrafts(installed.subarray(DRAFTS_OFFSET)).join("|"), "OK|ON MY WAY");
  assert.equal(await writeDrafts(link, first, () => {}), false);
  assert.equal(downloads.length, 1);
  assert.equal(await writeDrafts(link, encodeDrafts("CALL ME"), () => {}), true);
  assert.equal(decodeDrafts(installed.subarray(DRAFTS_OFFSET))[0], "CALL ME");
  assert.ok(sameBytes(installed.subarray(0, DRAFTS_OFFSET), original.subarray(0, DRAFTS_OFFSET)));
`));

test("Radio Watch replaces Spectrum only after checking saved channels", () => run(`
  const config = encodeWatch("Marine: 1, 3-5, 1000\\nAirband: 20");
  assert.equal(decodeWatch(config)[0].channels.join(","), "1,3,4,5,1000");
  assert.throws(() => encodeWatch("Too many: 1-33"), /32/);
  assert.throws(() => encodeWatch("Out of range: 1001"), /1–1000/);
  const slots = Array.from({ length: SLOT_COUNT }, () => new Uint8Array(SLOT_BYTES).fill(255));
  const spectrum = slots[4];
  spectrum.set(Uint8Array.from(atob(BUNDLED_WATCH), ch => ch.charCodeAt(0)));
  spectrum.fill(0, 12, 28);
  spectrum.set(new TextEncoder().encode("SPECTRUM"), 12);
  const previous = spectrum.slice();
  slots[0][100] = 7;
  let missing = true, erased = 0;
  const link = {
    async send(cmd, at, data) {
      if (cmd === CMD_READ) return { data: new Uint8Array(REC).fill(missing ? 255 : 1) };
      if (cmd === FLASH_READ) {
        const offset = at * FLASH_BLOCK - SLOT_BASE;
        const start = offset % SLOT_BYTES;
        return { data: slots[Math.floor(offset / SLOT_BYTES)].slice(start, start + FLASH_BLOCK) };
      }
      if (cmd === APP_ERASE) {
        assert.equal(at, 4);
        assert.ok(downloads.length);
        spectrum.fill(255);
        erased++;
      } else {
        assert.equal(cmd, APP_WRITE);
        spectrum.set(data, at);
      }
      return { data: new Uint8Array() };
    }
  };
  await assert.rejects(() => writeWatch(link, config, () => {}), /Channel 1 is empty/);
  assert.equal(erased, 0);
  missing = false;
  assert.equal(await writeWatch(link, config, () => {}), true);
  assert.ok(sameBytes(downloads[0].data, previous));
  assert.equal(appSlotName(spectrum), "RADIO WATCH");
  assert.ok(sameBytes(spectrum.subarray(DRAFTS_OFFSET), config));
  assert.equal(slots[0][100], 7);
  assert.equal(await writeWatch(link, config, () => {}), false);
  assert.equal(erased, 1);
`));

test("Satellite RX predicts timed passes and packs signed Doppler correction", () => run(`
  const tle = "1 25544U 98067A   26311.20783432  .00014294  00000+0  26479-3 0  9992\\n"
    + "2 25544  51.6321 312.3516 0007247 357.8301   2.2655 15.49235271593925";
  const now = Date.UTC(2026, 10, 8, 4) / 1000 - SECONDS_2000;
  const options = { name: "ISS", hz: 437800000, lat: 12.15, lon: -68.27, alt: 0, wide: true, tle };
  const { bytes, passes } = predictSatellite(options, now);
  assert.equal(String.fromCharCode(...bytes.subarray(0, 4)), "SRX1");
  assert.equal(bytes.length, SAT_PLAN_BYTES);
  assert.equal(bytes[4], passes.length);
  assert.ok(passes.length >= 3);
  assert.ok(passes.every(p => p.start >= now && p.end <= now + 48 * 3600 && p.peak >= 10));
  assert.ok(passes.every(p => p.points[0][2] > 0 && p.points.at(-1)[2] < 0));
  const view = new DataView(bytes.buffer);
  assert.equal(view.getInt16(SAT_PLAN_HEADER + 16, true), passes[0].points[0][2]);
  assert.equal(view.getUint16(SAT_PLAN_BYTES - 2), crc16(bytes.subarray(0, SAT_PLAN_BYTES - 2)));
  assert.throws(() => predictSatellite(options, now + 15 * 86400), /stale/);
  assert.throws(() => predictSatellite({ ...options, tle: tle.slice(0, -1) + "0" }, now), /checksums/);
`));

test("all built-in RTTTL tunes fit the radio", () => run(`
  assert.equal(Object.keys(TUNE_PRESETS).length, 21);
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

test("an asynchronous backup failure stops before any radio write", () => run(`
  port = new AppearanceRadio(true);
  await readAppearance();
  pendingArt = { startup: { bytes: new Uint8Array(ART_BYTES), name: "startup" } };
  saveFile = async () => { throw new Error("Could not save the backup"); };
  await writeAppearance();
  assert.match($("appearanceStatus").textContent, /Could not save the backup/);
  assert.equal(port.artWrites.length, 0);
  assert.equal(port.settingsWrites, 0);
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
