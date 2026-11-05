/* Simulated radio: same frame format, per-session sector erase and NOR flash behavior as the firmware. */
class FakeRadio {
  constructor(image) {
    this.flash = new Uint8Array(0x8000).fill(0xFF);
    this.flash.set(image);
    this.reset();
    this.erases = []; this.writes = 0; this.reads = 0; this.ends = 0; this.handshakes = 0;
    this.dropEvery = 0; this.corruptAddr = -1; this.dieAfterWrites = -1; this.silent = false; this.sent = 0;
  }
  reset() { this.session = false; this.mask = 0; this.rx = []; }
  get writable() {
    const self = this;
    return this._w || (this._w = new WritableStream({ write(chunk) { self.feed(chunk); } }));
  }
  get readable() {
    const self = this;
    return this._r || (this._r = new ReadableStream({ start(c) { self.ctrl = c; } }));
  }
  async open() { this._w = null; this._r = null; void this.writable; void this.readable; this.reset(); this.isOpen = true; }
  async setSignals() {}
  async close() { this.isOpen = false; }
  reply(bytes) { if (this.isOpen) this.ctrl.enqueue(Uint8Array.from(bytes)); }
  frame(cmd, addr, data = []) {
    const body = [cmd, addr >> 8, addr & 255, data.length >> 8, data.length & 255, ...data];
    const c = crc16(body);
    return [0xA6, ...body, c >> 8, c & 255];
  }
  feed(chunk) {
    if (this.silent) return;
    this.rx.push(...chunk);
    if (!this.session) {
      const s = String.fromCharCode(...this.rx);
      if (s.includes("PROGRAMBF-5RH")) { this.rx = []; this.session = true; this.mask = 0; this.handshakes++; this.reply([0x06]); }
      else if (this.rx.length > 64) this.rx = [];
      return;
    }
    while (this.rx.length >= 8) {
      if (this.rx[0] !== 0xA6) { this.rx.shift(); continue; }
      const n = this.rx[4] << 8 | this.rx[5];
      if (this.rx.length < 8 + n) return;
      const f = this.rx.splice(0, 8 + n);
      this.handle(f[1], f[2] << 8 | f[3], f.slice(6, 6 + n), f[6 + n] << 8 | f[7 + n], f.slice(1, 6 + n));
    }
  }
  handle(cmd, addr, data, crc, body) {
    if (crc !== crc16(body)) { this.reply(this.frame(0xEE, addr, [1])); return; }
    this.sent++;
    if (this.dropEvery && this.sent % this.dropEvery === 0) return;
    if (cmd === 0x52) {
      this.reads++;
      if (addr % 32 || addr / 32 > 999) { this.reply(this.frame(0xEE, addr, [2])); return; }
      this.reply(this.frame(0x52, addr, [...this.flash.subarray(addr, addr + 32)]));
    } else if (cmd === 0x57) {
      if (data.length !== 32 || addr % 32 || addr / 32 > 999) { this.reply(this.frame(0xEE, addr, [2])); return; }
      if (this.dieAfterWrites === 0) { this.silent = true; return; }
      if (this.dieAfterWrites > 0) this.dieAfterWrites--;
      const sector = addr >> 12;
      if (!(this.mask & 1 << sector)) { this.mask |= 1 << sector; this.flash.fill(0xFF, sector << 12, (sector + 1) << 12); this.erases.push(sector); }
      this.writes++;
      for (let i = 0; i < 32; i++) {
        const v = addr + i === this.corruptAddr ? 0x00 : data[i];
        this.flash[addr + i] &= v;
      }
      this.reply(this.frame(0x57, addr));
    } else if (cmd === 0x45) {
      this.ends++;
      this.reply(this.frame(0x45, addr));
      this.session = false;
    } else this.reply(this.frame(0xEE, addr, [3]));
  }
}

(async function () {
  const out = {};
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const dump = Uint8Array.from(atob(RADIO_DUMP_B64), c => c.charCodeAt(0));
  const downloads = [];
  const realClick = HTMLAnchorElement.prototype.click;
  HTMLAnchorElement.prototype.click = function () { if (this.download) downloads.push(this.download); else realClick.call(this); };
  const settle = async () => { await sleep(5); for (let i = 0; i < 40000 && busy; i++) await sleep(25); };
  const notice = () => $("notice").textContent;
  const useRadio = r => { port = r; };
  const closeDlgs = () => { for (const d of document.querySelectorAll("dialog")) if (d.open) d.close(); };

  try {
    /* 1. The encoder must reproduce what CHIRP actually put on the radio, byte for byte. */
    const fresh = () => channelsFromCsv(BUILTIN_CSV).chans;
    const t0 = buildTable(fresh());
    let mism = [];
    for (let n = 0; n < 1000; n++) {
      if (!sameBytes(t0.subarray(n * 32, n * 32 + 32), dump.subarray(n * 32, n * 32 + 32))) mism.push(n + 1);
    }
    out.encoder_matches_real_radio = mism.length === 0;
    out.encoder_mismatches = mism.slice(0, 12);
    if (mism.length) out.first_mismatch = { want: [...dump.subarray((mism[0] - 1) * 32, mism[0] * 32)].join(","), got: [...t0.subarray((mism[0] - 1) * 32, mism[0] * 32)].join(",") };

    /* 2. Decoding then re-encoding is lossless for every channel on the real radio. */
    let rt = 0;
    for (let n = 0; n < 349; n++) if (!sameBytes(encode(decode(dump, n)), dump.subarray(n * 32, n * 32 + 32))) rt++;
    out.decode_roundtrip_failures = rt;
    const c234 = decode(dump, 233);
    out.decode_sample = [c234.name, c234.freq, c234.duplex, c234.offset, c234.tone, c234.dcs].join("|");
    out.decode_am = decode(dump, 7).mode; out.decode_rxonly = decode(dump, 0).duplex;

    /* 3. CSV: untouched list saves byte for byte; edits change only that row. */
    out.csv_identity = exportCsv() === BUILTIN_CSV;
    out.bcd_sample = [...bcdBytes(to10Hz("147.18"))].map(x => x.toString(16)).join(" ");
    out.bad_csv = (() => { try { channelsFromCsv("a,b\n1,2\n"); return "ACCEPTED"; } catch (e) { return e.message; } })();

    /* 4. No difference: reads only, nothing erased or written. */
    let r = new FakeRadio(dump); useRadio(r);
    $("uploadBtn").click(); await settle();
    out.same_notice = notice(); out.same_writes = r.writes; out.same_erases = r.erases.length; out.same_ends = r.ends;
    out.same_dialog_open = $("confirmDlg").open;
    await compareWithRadio();
    out.compare_notice = notice();

    /* 5. One edit: only its sector is rewritten, every neighbor survives, backup comes first. */
    r = new FakeRadio(dump); useRadio(r);
    const c257 = chans.find(c => c.num === 257);
    setField(c257, { offset: "5.000000" }); setDirty(true);
    $("uploadBtn").click(); await settle();
    out.confirm_open = $("confirmDlg").open;
    out.confirm_text = $("confirmText").textContent;
    out.confirm_list = $("diffList").textContent.slice(0, 60);
    out.preview_wrote_nothing = r.writes === 0 && r.erases.length === 0;
    downloads.length = 0;
    $("confirmYes").click(); await sleep(0); await settle();
    out.upload_notice = notice();
    out.upload_erased_sectors = r.erases;
    out.upload_writes = r.writes;
    out.upload_ends = r.ends;
    out.backup_downloaded = downloads.some(d => /^radio-backup-.*\.bin$/.test(d));
    let wrong = [];
    const want = buildTable(chans);
    for (let n = 0; n < 1000; n++) if (!sameBytes(r.flash.subarray(n * 32, n * 32 + 32), want.subarray(n * 32, n * 32 + 32))) wrong.push(n + 1);
    out.flash_matches_list = wrong.length === 0;
    let changedOnRadio = [];
    for (let n = 0; n < 1000; n++) if (!sameBytes(r.flash.subarray(n * 32, n * 32 + 32), dump.subarray(n * 32, n * 32 + 32))) changedOnRadio.push(n + 1);
    out.channels_changed_on_radio = changedOnRadio;
    out.ch257_new_tx = decode(r.flash, 256).offset;

    /* 6. Delete everything in a sector: a sector that ends up blank must still get erased. */
    loadCsvText(BUILTIN_CSV, "Your channel list");
    chans = chans.filter(c => !(c.num >= 129 && c.num <= 256));
    r = new FakeRadio(dump); useRadio(r);
    $("uploadBtn").click(); await settle();
    out.delete_confirm = $("confirmText").textContent;
    $("confirmYes").click(); await sleep(0); await settle();
    out.delete_erased = r.erases; out.delete_notice = notice();
    out.sector_now_blank = (() => { for (let i = 128 * 32; i < 256 * 32; i++) if (r.flash[i] !== 0xFF) return false; return true; })();
    out.other_sectors_intact = sameBytes(r.flash.subarray(0, 128 * 32), dump.subarray(0, 128 * 32)) && sameBytes(r.flash.subarray(256 * 32, 32000), dump.subarray(256 * 32, 32000));

    /* 7. Dropped replies get retried and the upload still lands. */
    loadCsvText(BUILTIN_CSV, "Your channel list");
    setField(chans.find(c => c.num === 1), { name: "WX HOME" });
    r = new FakeRadio(dump); r.dropEvery = 97; useRadio(r);
    $("uploadBtn").click(); await settle();
    if ($("confirmDlg").open) { $("confirmYes").click(); await sleep(0); await settle(); }
    out.retry_notice = notice(); out.retry_ch1 = decode(r.flash, 0).name;

    /* 8. A bad write is caught by the read-back. */
    loadCsvText(BUILTIN_CSV, "Your channel list");
    setField(chans.find(c => c.num === 1), { name: "WX HOME" });
    r = new FakeRadio(dump); r.corruptAddr = 21; useRadio(r);
    $("uploadBtn").click(); await settle();
    $("confirmYes").click(); await sleep(0); await settle();
    out.verify_fail_notice = notice();

    /* 9. Cable pulled mid-write: clear warning, backup already saved. */
    loadCsvText(BUILTIN_CSV, "Your channel list");
    setField(chans.find(c => c.num === 1), { name: "WX HOME" });
    r = new FakeRadio(dump); r.dieAfterWrites = 20; useRadio(r);
    downloads.length = 0;
    $("uploadBtn").click(); await settle();
    $("confirmYes").click(); await sleep(0); await settle();
    out.die_notice = notice(); out.die_backup = downloads.length > 0;

    /* 10. Radio off: friendly error, no writes. */
    r = new FakeRadio(dump); r.silent = true; useRadio(r);
    loadCsvText(BUILTIN_CSV, "Your channel list");
    $("readBtn").click(); await settle();
    out.off_notice = notice();

    /* 11. Channels edited on the radio since the preview stop the upload. */
    loadCsvText(BUILTIN_CSV, "Your channel list");
    setField(chans.find(c => c.num === 2), { name: "WX TWO" });
    r = new FakeRadio(dump); useRadio(r);
    $("uploadBtn").click(); await settle();
    out.stale_byte_before = r.flash[20 * 32 + 20];
    r.flash[20 * 32 + 20] ^= 0x7F;
    $("confirmYes").click(); await sleep(0); await settle();
    out.stale_notice = notice(); out.stale_writes = r.writes;

    /* Restoring: a backup .bin opened through the file picker goes back to the exact bytes. */
    closeDlgs();
    const dt = new DataTransfer();
    dt.items.add(new File([dump.slice(0, 32000)], "radio-backup-test.bin"));
    $("fileIn").files = dt.files;
    $("fileIn").dispatchEvent(new Event("change"));
    for (let i = 0; i < 4000 && !notice().startsWith("Opened"); i++) await sleep(25);
    out.restore_notice = notice();
    out.restore_count = chans.length;
    out.restore_identical = sameBytes(buildTable(chans), dump.subarray(0, 32000));
    const shortBin = new DataTransfer();
    shortBin.items.add(new File([new Uint8Array(100)], "bad.bin"));
    $("fileIn").files = shortBin.files;
    $("fileIn").dispatchEvent(new Event("change"));
    for (let i = 0; i < 4000 && !notice().startsWith("That backup"); i++) await sleep(25);
    out.restore_bad_notice = notice();
    out.restore_bad_kept = chans.length;

    /* 12. Bad inputs and the editor. */
    closeDlgs();
    loadCsvText(BUILTIN_CSV, "Your channel list");
    out.validate_freq = validate({ num: 1, freq: "600", duplex: "off", tone: "", name: "" });
    out.validate_name = validate({ num: 1, freq: "146", duplex: "off", tone: "", name: "é" });
    out.validate_dcs = validate({ num: 1, freq: "146", duplex: "off", tone: "DTCS", dcs: "999", name: "" });
    out.tx_allowed_303 = txAllowed(chans.find(c => c.num === 303));
    out.tx_allowed_234 = txAllowed(chans.find(c => c.num === 234));
    openEditor(chans.find(c => c.num === 234));
    out.editor_tone = $("fTone").value + "/" + $("fToneVal").value;
    $("fFreq").value = "9999"; applyEditor();
    out.editor_err = $("formErr").textContent; out.editor_stays_open = $("editDlg").open;
    $("fFreq").value = "147.180"; $("fNum").value = "1"; applyEditor();
    out.editor_dup_err = $("formErr").textContent;
    $("fNum").value = "234"; $("fName").value = "SKY/ARES2"; applyEditor();
    out.editor_closed = !$("editDlg").open;
    out.edited_only_one_row = exportCsv().split("\r\n").filter((l, i) => l !== BUILTIN_CSV.split("\r\n")[i]).length === 1;
    out.edited_row = exportCsv().split("\r\n")[234].slice(0, 60);
    out.scan_toggle = (() => { const c = chans.find(x => x.num === 305); setField(c, { skip: "S" }); return encode(c)[15].toString(16); })();

    /* 13. Theme and environment. */
    const bgRule = getComputedStyle(document.body, "::before").backgroundImage;
    out.skull_background = bgRule.startsWith('url("data:image/png');
    out.page_bg = getComputedStyle(document.body).backgroundColor;
    out.header_bg = getComputedStyle(document.querySelector("header")).backgroundColor;
    out.primary_bg = getComputedStyle($("uploadBtn")).backgroundColor;
    out.secure_context = window.isSecureContext;
    out.web_serial_present = "serial" in navigator;
    out.done = true;
  } catch (e) { out.ERROR = e.stack || String(e); }
  $("testout").textContent = "RESULT" + JSON.stringify(out) + "ENDRESULT";
})();
