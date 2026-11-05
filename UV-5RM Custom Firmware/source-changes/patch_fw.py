"""Apply the Radio Program changes to a pristine bf-5rh-firmware-open checkout. Refuses partial matches."""
import os, sys

root = sys.argv[1]
here = os.path.dirname(os.path.abspath(__file__))


def patch(rel, *pairs):
    path = os.path.join(root, rel)
    s = open(path, encoding="utf-8", newline="").read()
    for old, new, *count in pairs:
        want = count[0] if count else 1
        got = s.count(old)
        assert got == want, f"{rel}: expected {want} of {old[:70]!r}, found {got}"
        s = s.replace(old, new)
    open(path, "w", encoding="utf-8", newline="").write(s)


# --- defaults: logo at boot, single-watch, transmit off until enabled in settings
patch("src/flash_map.rs",
      ("    pub const OVERLAY_SLOT_COUNT: u8 = 8;\n", "    pub const OVERLAY_SLOT_COUNT: u8 = 9;\n"),
      ("        tx_forbid: false,\n", "        tx_forbid: true,\n"),
      ("        dual_standby: true,\n", "        dual_standby: false,\n"),
      ("        boot_display_mode: 1,\n", "        boot_display_mode: 2,\n"),
      ("        boot_sound_enabled: false,\n", "        boot_sound_enabled: true,\n"),
      ("    pub fn tx_freq_deci_hz(&self) -> u32 {\n        bcd4_to_deci_hz(self.tx_freq_bcd)\n    }\n",
       "    pub fn tx_freq_deci_hz(&self) -> u32 {\n        bcd4_to_deci_hz(self.tx_freq_bcd)\n    }\n\n"
       "    /// CHIRP's Duplex \"off\" stores a blank TX frequency: receive-only.\n"
       "    pub fn tx_is_blank(&self) -> bool {\n        self.tx_freq_bcd == [0xFF; 4]\n    }\n"))

# --- run the upstream one-time cleanup of stale factory records (channels untouched)
patch("src/main.rs",
      ("    let mut storage = device::storage::Storage::new(norflash);\n",
       "    let mut storage = device::storage::Storage::new(norflash);\n"
       "    if !storage.is_first_boot_done() {\n"
       "        storage.first_boot_format();\n"
       "        storage.mark_first_boot_done();\n"
       "    }\n"))

# --- the logo replaces the text boot screen, which frees the flash the modes need
patch("src/main.rs",
      ("        1 => {\n            ui::boot::draw_message(\n                display.as_draw_target(),\n"
       "                app.settings(),\n                app.battery_voltage_cv(),\n                app.battery_bars(),\n"
       "                chip_id,\n            );\n        }\n        2 => draw_boot_logo(&mut display, &mut app),\n",
       "        1 | 2 => draw_boot_logo(&mut display, &mut app),\n"))

# --- channel side: AM from band, receive-only guard
patch("src/app/side.rs",
      ("use crate::device::radio::{ChannelConfig, Power, SubAudio};\n",
       "use crate::device::radio::{ChannelConfig, Modulation, Power, SubAudio};\n"),
      ("use crate::flash_map::{self, addr};\n\n#[derive(Clone, Copy)]\npub(super) struct VfoBackup {",
       "use crate::flash_map::{self, addr};\n\n"
       "/// Airband is AM; nothing else programmed on this radio is.\n"
       "fn modulation_for(freq_hz: u32) -> Modulation {\n"
       "    if (108_000_000..=136_000_000).contains(&freq_hz) {\n"
       "        Modulation::Am\n    } else {\n        Modulation::Fm\n    }\n}\n\n"
       "#[derive(Clone, Copy)]\npub(super) struct VfoBackup {"),
      ("    pub reversed: bool,\n", "    pub reversed: bool,\n    /// Channel has a blank TX frequency; PTT must never key it.\n    pub rx_only: bool,\n"),
      ("        if self.reversed {\n            self.cfg.freq_hz = tx;",
       "        if self.reversed && !self.rx_only {\n            self.cfg.freq_hz = tx;"),
      ("    pub fn load_vfo(&mut self, vfo: &flash_map::VfoMode) {\n        self.vfo_chan = ChVfoMode::Vfo;\n",
       "    /// Follow the band in VFO mode, leaving a manually chosen USB alone.\n"
       "    pub fn auto_modulation(&mut self) {\n"
       "        if matches!(self.cfg.modulation, Modulation::Fm | Modulation::Am) {\n"
       "            self.cfg.modulation = modulation_for(self.rx_freq_hz);\n"
       "        }\n    }\n\n"
       "    pub fn load_vfo(&mut self, vfo: &flash_map::VfoMode) {\n        self.vfo_chan = ChVfoMode::Vfo;\n        self.rx_only = false;\n"),
      ("    pub fn restore_vfo_from_backup(&mut self) {\n        let backup = self.vfo_backup;\n        self.vfo_chan = ChVfoMode::Vfo;\n",
       "    pub fn restore_vfo_from_backup(&mut self) {\n        let backup = self.vfo_backup;\n        self.vfo_chan = ChVfoMode::Vfo;\n        self.rx_only = false;\n"),
      ("        self.tx_freq_hz = ch.tx_freq_deci_hz() * 10;\n",
       "        self.tx_freq_hz = ch.tx_freq_deci_hz() * 10;\n        self.rx_only = ch.tx_is_blank();\n        self.cfg.modulation = modulation_for(self.rx_freq_hz);\n"),
      ("            ChVfoMode::Channel => {\n                if self.tx_freq_hz == self.rx_freq_hz {",
       "            ChVfoMode::Channel => {\n                if self.rx_only || self.tx_freq_hz == self.rx_freq_hz {"))

patch("src/app/tx.rs",
      ("        if app.settings.tx_forbid {\n            return;\n        }\n",
       "        if app.settings.tx_forbid\n            || (!external && app.sides[app.master].rx_only)\n        {\n"
       "            app.tx_prohibited = true;\n            return;\n        }\n"))

# --- channel navigation honors the active mode; numbers on screen count from 1
patch("src/app/side_ops.rs",
      ("                s.rx_freq_hz.saturating_sub(step_hz)\n            };\n            s.refresh_cfg_freqs();\n",
       "                s.rx_freq_hz.saturating_sub(step_hz)\n            };\n            s.auto_modulation();\n            s.refresh_cfg_freqs();\n"),
      ("        if !app.storage.is_channel_empty(num) {\n            return Some(num);\n        }\n",
       "        if !app.storage.is_channel_empty(num)\n            && modes::contains(app.chan_mode, num)\n        {\n            return Some(num);\n        }\n"),
      ("            if value <= MAX_CHANNEL_NUM as u32\n                && !app.storage.is_channel_empty(value as u16)\n            {\n"
       "                load_channel_num(app, value as u16);\n                sync_watching_to_master(app, syst);\n            }\n",
       "            // Typed numbers count from 1 like the printed sheet; flash slots from 0.\n"
       "            if (1..=MAX_CHANNEL_NUM as u32 + 1).contains(&value) {\n"
       "                let num = value as u16 - 1;\n"
       "                if !app.storage.is_channel_empty(num)\n"
       "                    && modes::contains(app.chan_mode, num)\n                {\n"
       "                    load_channel_num(app, num);\n"
       "                    sync_watching_to_master(app, syst);\n                }\n            }\n"),
      ("            s.rx_freq_hz = value * 1000;\n            s.refresh_cfg_freqs();\n",
       "            s.rx_freq_hz = value * 1000;\n            s.auto_modulation();\n            s.refresh_cfg_freqs();\n"),
      ("            let num = if app.storage.is_channel_empty(num) {\n",
       "            let num = if app.storage.is_channel_empty(num)\n                || !modes::contains(app.chan_mode, num)\n            {\n"))
s_path = os.path.join(root, "src/app/side_ops.rs")
s = open(s_path, encoding="utf-8", newline="").read()
open(s_path, "w", encoding="utf-8", newline="").write("use super::modes;\n" + s)

# --- scanner: stays inside the mode, or inside the custom range in VFO
patch("src/app/scan.rs",
      ("use super::{App, ChVfoMode, Mode, MAX_CHANNEL_NUM};\n", "use super::{modes, App, ChVfoMode, Mode, MAX_CHANNEL_NUM};\n"),
      ("        if app.storage.is_channel_empty(n) {\n            continue;\n        }\n",
       "        if app.storage.is_channel_empty(n)\n            || !modes::contains(app.chan_mode, n)\n        {\n            continue;\n        }\n"),
      ("            let (lo, hi) = FreqRanges::hardware().bounds();\n            let s = &mut app.sides[app.master];\n",
       "            let (lo, hi) = if modes::is_custom(app.chan_mode) {\n"
       "                (app.range_lo_hz, app.range_hi_hz)\n            } else {\n"
       "                FreqRanges::hardware().bounds()\n            };\n"
       "            let s = &mut app.sides[app.master];\n"),
      ("            s.rx_freq_hz = wrap_freq(s.rx_freq_hz, step_hz, up, lo, hi);\n",
       "            if s.rx_freq_hz < lo || s.rx_freq_hz > hi {\n                s.rx_freq_hz = lo;\n            }\n"
       "            s.rx_freq_hz = wrap_freq(s.rx_freq_hz, step_hz, up, lo, hi);\n            s.auto_modulation();\n"),
      ("        (KeyEventKind::Single, KeyId::Down) => {\n            app.scan.up = false;\n            hop(app, syst);\n        }\n",
       "        (KeyEventKind::Single, KeyId::Down) => {\n            app.scan.up = false;\n            hop(app, syst);\n        }\n"
       "        (KeyEventKind::Single, KeyId::Band) => {\n            exit(app, syst);\n            modes::open_menu(app);\n        }\n"))

# --- keys
patch("src/app/keys.rs",
      ("        Mode::Scan => scan::dispatch(app, syst, ev),\n",
       "        Mode::Scan => scan::dispatch(app, syst, ev),\n"
       "        Mode::ModeMenu => modes::dispatch_menu(app, syst, ev),\n"
       "        Mode::RangeInput => modes::dispatch_range(app, syst, ev),\n"),
      ("use super::launcher;\n", "use super::launcher;\nuse super::modes;\n"),
      ("                KeyId::Side1 | KeyId::Side2 | KeyId::Band => {\n"
       "                    let func = keyfn::from_u8(short_function(app, ev.key));\n",
       "                KeyId::Band => modes::open_menu(app),\n"
       "                KeyId::Side1 | KeyId::Side2 => {\n"
       "                    let func = keyfn::from_u8(short_function(app, ev.key));\n"))

# --- app core
patch("src/app/mod.rs",
      ("mod launcher;\n", "mod launcher;\nmod modes;\n"),
      ("    Scan,\n    /// running external app from spi flash\n",
       "    Scan,\n    ModeMenu,\n    RangeInput,\n    /// running external app from spi flash\n"),
      ("    launcher_index: usize,\n\n", "    launcher_index: usize,\n"
       "    chan_mode: u8,\n    mode_menu_index: usize,\n    range_lo_hz: u32,\n    range_hi_hz: u32,\n    range_stage: u8,\n    range_err: bool,\n\n"),
      ("                reversed: false,\n                ani_target: None,\n                cfg: default_cfg,\n",
       "                reversed: false,\n                rx_only: false,\n                ani_target: None,\n                cfg: default_cfg,\n", 2),
      ("                s.cfg.modulation = modulation_from_raw(state[6 + half]);\n",
       "                // Channels take their modulation from their band.\n"
       "                if s.vfo_chan == ChVfoMode::Vfo {\n"
       "                    s.cfg.modulation = modulation_from_raw(state[6 + half]);\n"
       "                }\n"),
      ("        radio.set_frequency(sides[0].cfg.freq_hz);\n",
       "        // First power-on: open on channel 1 rather than a bare VFO.\n"
       "        if storage.load_channel_state().is_none()\n"
       "            && !storage.is_channel_empty(0)\n        {\n"
       "            let ch = storage.read_channel(0);\n"
       "            for s in sides.iter_mut() {\n                s.load_channel(0, &ch);\n            }\n        }\n\n"
       "        let (chan_mode, range_lo_hz, range_hi_hz) = modes::restore(&settings);\n\n"
       "        radio.set_frequency(sides[0].cfg.freq_hz);\n"),
      ("            launcher_index: 0,\n",
       "            launcher_index: 0,\n            chan_mode,\n            mode_menu_index: chan_mode as usize,\n"
       "            range_lo_hz,\n            range_hi_hz,\n            range_stage: 0,\n            range_err: false,\n"))

# --- a short cue distinct from the key beep
patch("src/device/radio.rs",
      ("    fn play_roger_tone(&mut self, syst: &mut SYST) {\n",
       "    /// Local-speaker-only tone sequence; never keys the transmitter.\n"
       "    pub fn play_cue(&mut self, syst: &mut SYST, tones: &[(u16, u32)]) {\n"
       "        if !self.beeps_enabled || self.audio_open {\n            return;\n        }\n"
       "        if self.rf_parked {\n            self.fd6818.wake(syst);\n            self.rf_parked = false;\n        }\n"
       "        self.play_tone_sequence(syst, tones, false, true);\n    }\n\n"
       "    fn play_roger_tone(&mut self, syst: &mut SYST) {\n"))

# --- screens: black bars, numbering from 1, active mode shown
patch("src/ui/standby.rs",
      ("const STATUS_BG: Rgb565 = Rgb565::new(2, 6, 12);\n", "const STATUS_BG: Rgb565 = Rgb565::BLACK;\n"),
      ('        write!(label, "M{:03}", app.side_channel_num(i)).ok();\n',
       '        write!(label, "M{:03}", app.side_channel_num(i) + 1).ok();\n'),
      ("    if app.is_key_locked() {\n        icons::draw_padlock(lcd, x, 2, AMBER);\n    }\n}\n",
       "    if app.is_key_locked() {\n        icons::draw_padlock(lcd, x, 2, AMBER);\n    }\n"
       "    let short = app.chan_mode_short();\n"
       "    text(\n        lcd,\n        short,\n        &FONT_5X8,\n"
       "        FLAGS_W - short.len() as i32 * TINY_W - 2,\n        baseline,\n        ACCENT,\n    );\n}\n"))
patch("src/ui/boot.rs",
      ("const HEADER_BG: Rgb565 = Rgb565::new(2, 6, 12);\n", "const HEADER_BG: Rgb565 = Rgb565::BLACK;\n"))
patch("src/ui/scan.rs",
      ("const HEADER_H: i32 = 12;\n", "const HEADER_H: i32 = 26;\n"),
      ("    let mut header: TextBuf<20> = TextBuf::new();\n", "    let mut header: TextBuf<27> = TextBuf::new();\n"),
      ('        write!(header, "SCAN {} M{}", dir, app.watching_channel_num()).ok();\n',
       '        write!(\n            header,\n            "{} {} M{:03}",\n            app.chan_mode_name(),\n            dir,\n            app.watching_channel_num() + 1\n        )\n        .ok();\n'),
      ('        write!(header, "SCAN {}", dir).ok();\n',
       '        write!(header, "{} {}", app.chan_mode_name(), dir).ok();\n'),
      ("    Text::new(\n        header.as_str(),\n        Point::new(4, 14),\n        MonoTextStyle::new(&FONT_6X10, FG),\n    )\n    .draw(lcd)\n    .ok();\n",
       "    Text::new(\n        header.as_str(),\n        Point::new(4, 14),\n        MonoTextStyle::new(&FONT_6X10, FG),\n    )\n    .draw(lcd)\n    .ok();\n"
       "    if app.watching_is_channel_mode() {\n"
       "        Text::new(\n            app.side_name_str(app.watching_index()),\n            Point::new(4, 27),\n"
       "            MonoTextStyle::new(&FONT_6X10, FG),\n        )\n        .draw(lcd)\n        .ok();\n    }\n"))
patch("src/ui/mod.rs",
      ("mod list;\n", "mod list;\nmod modes;\n"),
      ("    scan: scan::Cache,\n}\n", "    scan: scan::Cache,\n    range: modes::RangeCache,\n}\n"),
      ("        state.scan = scan::Cache::new();\n", "        state.scan = scan::Cache::new();\n        state.range = modes::RangeCache::new();\n"),
      ("        app::Mode::Scan => scan::draw_scan(lcd, app, &mut state.scan),\n",
       "        app::Mode::Scan => scan::draw_scan(lcd, app, &mut state.scan),\n"
       "        app::Mode::ModeMenu => {\n            modes::draw_menu(lcd, app, &mut state.list)\n        }\n"
       "        app::Mode::RangeInput => {\n            modes::draw_range(lcd, app, &mut state.range)\n        }\n"))
s_path = os.path.join(root, "src/ui/mod.rs")
s = open(s_path, encoding="utf-8", newline="").read()
old = "            scan: scan::Cache::new(),\n"
assert s.count(old) == 1, "UiState::new scan line"
open(s_path, "w", encoding="utf-8", newline="").write(s.replace(old, old + "            range: modes::RangeCache::new(),\n"))

# --- name and frequency side by side on the main and scan screens
patch("src/ui/standby.rs",
      ("                let shown = &name[..name.len().min(16)];\n"
       "                let name_y = y + FONT_6X10.baseline as i32 + 1;\n"
       "                text_right(lcd, shown, &FONT_6X10, name_y, fg);\n"
       "                let mut freq_line: TextBuf<12> = TextBuf::new();\n"
       "                write!(\n                    freq_line,\n                    \"{:3}.{:05}\",\n"
       "                    freq_hz / 1_000_000,\n                    (freq_hz % 1_000_000) / 10\n                )\n                .ok();\n"
       "                text_right(\n                    lcd,\n                    freq_line.as_str(),\n                    &FONT_6X10,\n"
       "                    name_y + FONT_6X10.character_size.height as i32,\n                    fg,\n                );\n",
       "                let shown = &name[..name.len().min(10)];\n"
       "                text(lcd, shown, &FONT_9X18, CONTENT_X + 3, baseline, fg);\n"
       "                let mut freq_line: TextBuf<12> = TextBuf::new();\n"
       "                write!(\n                    freq_line,\n                    \"{}.{:04}\",\n"
       "                    freq_hz / 1_000_000,\n                    freq_hz / 100 % 10_000\n                )\n                .ok();\n"
       "                text_right(lcd, freq_line.as_str(), &FONT_6X10, baseline, fg);\n"))
patch("src/ui/scan.rs",
      ("        Text::new(\n            app.side_name_str(app.watching_index()),\n            Point::new(4, 27),\n",
       "        let hz = app.watching_freq_hz();\n"
       "        let mut line: TextBuf<26> = TextBuf::new();\n"
       "        write!(\n            line,\n            \"{}  {}.{:04}\",\n            app.side_name_str(app.watching_index()),\n"
       "            hz / 1_000_000,\n            hz / 100 % 10_000\n        )\n        .ok();\n"
       "        Text::new(\n            line.as_str(),\n            Point::new(4, 27),\n"))

# --- dark red theme: selection and title bars, plus the main screen's accent color
patch("src/ui/list.rs",
      ("const TITLE_BG: Rgb565 = Rgb565::new(9, 19, 9);\n",
       "const TITLE_BG: Rgb565 = Rgb565::new(11, 0, 0);\nconst SELECT_BG: Rgb565 = Rgb565::new(16, 0, 0);\n"),
      ("        .into_styled(PrimitiveStyle::with_fill(FG))\n        .draw(lcd)\n        .ok();\n    }\n"
       "    let fg = if is_selected { BG } else { FG };\n",
       "        .into_styled(PrimitiveStyle::with_fill(SELECT_BG))\n        .draw(lcd)\n        .ok();\n    }\n"
       "    let fg = FG;\n"))
patch("src/ui/standby.rs",
      ("const ACCENT: Rgb565 = Rgb565::new(0, 55, 28);\n", "const ACCENT: Rgb565 = Rgb565::new(24, 8, 4);\n"))
patch("apps/fm/src/main.rs",
      ("const TITLE_BG: u16 = 0x0881;\n", "const TITLE_BG: u16 = 0x5800;\n"))

# --- menus sit on a dimmed copy of the skull; the main and scan screens stay black
patch("src/ui/mod.rs", ("mod icons;\n", "mod backdrop;\nmod icons;\n"))
patch("src/ui/list.rs",
      ("use super::{clear_strip, TextBuf, SCREEN_H, SCREEN_W};\n",
       "use super::backdrop::draw_backdrop;\nuse super::{TextBuf, SCREEN_H, SCREEN_W};\n"),
      ("    fn row_count(&mut self) -> usize;\n",
       "    fn row_count(&mut self) -> usize;\n    /// Raw flash read, for the menu backdrop.\n    fn read_flash(&mut self, addr: u32, buf: &mut [u8]);\n"),
      ("            clear_body(lcd);\n", ""),
      ("    clear_strip(lcd, 0, SCREEN_H);\n\n    draw_title(lcd, title);\n", "    draw_title(lcd, title);\n"),
      ("        draw_row(lcd, source, top + slot, selected, show_arrows, total, slot);\n    }\n}\n",
       "        draw_row(lcd, source, top + slot, selected, show_arrows, total, slot);\n    }\n"
       "    let bottom = LIST_TOP + VISIBLE_ROWS as i32 * ROW_HEIGHT;\n"
       "    draw_backdrop(\n        lcd,\n        &mut |addr, buf| source.read_flash(addr, buf),\n        bottom,\n        SCREEN_H - bottom,\n    );\n}\n"),
      ("fn clear_body<D>(lcd: &mut D)\nwhere\n    D: DrawTarget<Color = Rgb565>,\n{\n    Rectangle::new(\n        Point::new(0, LIST_TOP),\n"
       "        Size::new(SCREEN_W as u32, (SCREEN_H - LIST_TOP) as u32),\n    )\n    .into_styled(PrimitiveStyle::with_fill(BG))\n    .draw(lcd)\n    .ok();\n}\n\n", ""),
      ("    Rectangle::new(\n        Point::new(0, row_top),\n        Size::new(SCREEN_W as u32, ROW_HEIGHT as u32),\n    )\n"
       "    .into_styled(PrimitiveStyle::with_fill(BG))\n    .draw(lcd)\n    .ok();\n\n    if index >= total {\n"
       "        return; // blank row below the end of a shorter list\n    }\n\n    let is_selected = index == selected;\n    if is_selected {\n",
       "    let is_selected = index < total && index == selected;\n    if !is_selected {\n        draw_backdrop(\n            lcd,\n"
       "            &mut |addr, buf| source.read_flash(addr, buf),\n            row_top,\n            ROW_HEIGHT,\n        );\n    }\n\n"
       "    if index >= total {\n        return; // blank row below the end of a shorter list\n    }\n\n    if is_selected {\n"))
patch("src/ui/launcher.rs",
      ("        self.0.launcher_item_count()\n    }\n",
       "        self.0.launcher_item_count()\n    }\n\n    fn read_flash(&mut self, addr: u32, buf: &mut [u8]) {\n        self.0.storage_mut().read_raw(addr, buf);\n    }\n"))
patch("src/app/overlay.rs",
      ("    fn row_count(&mut self) -> usize {\n        self.total\n    }\n",
       "    fn row_count(&mut self) -> usize {\n        self.total\n    }\n\n    fn read_flash(&mut self, addr: u32, buf: &mut [u8]) {\n        app_ref().storage_mut().read_raw(addr, buf);\n    }\n"))

# --- channels were written by CHIRP, whose raw power order is 0 high, 1 low, 2 medium
patch("src/app/convert.rs",
      ("        2 => Power::Low,\n        1 => Power::Mid,\n", "        1 => Power::Low,\n        2 => Power::Mid,\n"),
      ("        Power::Mid => 1,\n        Power::Low => 2,\n", "        Power::Mid => 2,\n        Power::Low => 1,\n"))
patch("apps/chanmgr/src/main.rs",
      ('        1 => "MID",\n        2 => "LOW",\n', '        1 => "LOW",\n        2 => "MID",\n'))
patch("apps/settings/common.rs",
      ('        2 => "LOW",\n        1 => "MID",\n', '        2 => "MID",\n        1 => "LOW",\n'))

# --- US transmit plan by default: only the services programmed here, nothing else
patch("src/flash_map.rs", ("        band_lock: 0,\n", "        band_lock: 1,\n"))
patch("src/device/radio.rs",
      ("                r(144_000_000, 148_000_000),\n                r(420_000_000, 450_000_000),\n            ]),\n            BandLock::Ce =>",
       "                r(144_000_000, 148_000_000),\n                r(222_000_000, 225_000_000),\n                r(420_000_000, 450_000_000),\n"
       "                // FRS/GMRS\n                r(462_550_000, 462_725_000),\n                r(467_550_000, 467_725_000),\n"
       "                // MURS\n                r(151_820_000, 151_940_000),\n                r(154_570_000, 154_600_000),\n"
       "            ]),\n            BandLock::Ce =>"))

s_path = os.path.join(root, "src/app/mod.rs")
s = open(s_path, encoding="utf-8", newline="").read()
s += "\npub(crate) fn modes_table() -> &'static [modes::ChanMode] {\n    &modes::MODES\n}\n"
open(s_path, "w", encoding="utf-8", newline="").write(s)

# --- new files
for src, dst in (("modes_app.rs", "src/app/modes.rs"), ("modes_ui.rs", "src/ui/modes.rs"), ("backdrop.rs", "src/ui/backdrop.rs")):
    data = open(os.path.join(here, src), encoding="utf-8", newline="").read()
    open(os.path.join(root, dst), "w", encoding="utf-8", newline="").write(data)
# --- add-on apps that don't exist upstream
for dirpath, _, names in os.walk(os.path.join(here, "apps_new")):
    rel = os.path.relpath(dirpath, os.path.join(here, "apps_new"))
    os.makedirs(os.path.join(root, "apps", rel), exist_ok=True)
    for name in names:
        data = open(os.path.join(dirpath, name), "rb").read()
        open(os.path.join(root, "apps", rel, name), "wb").write(data)
print("patched", root)
