# AR-5RM Radio Mod

Custom firmware and add-on apps for the Baofeng AR-5RM NRF and compatible UV-5RM
radios, based on [bf-5rh-firmware-open](https://github.com/sophiel-meow/bf-5rh-firmware-open).
Send short radio-to-radio texts with delivery confirmation, scan saved channels,
and customize the display and startup sound from a local browser tool.

> **Check the hardware ID and back up your radio before flashing.** The tested
> hardware has no documented recovery path back to its factory firmware. A bad
> flash can brick the radio. Use the included programming tools after flashing.
> CHIRP CSV import and export are supported by the CPS, but CHIRP cannot program
> the custom firmware directly.

## What it adds

| Feature | What it does |
|---|---|
| **TEXT** | Sends messages up to 50 characters, retries delivery, saves messages, and alerts on receipt. Load up to eight reusable drafts from the AR-5RM CPS. |
| **RADIO WATCH** | Scans named lists of saved channels and lets you listen to recent hits. Optional CPS install replaces SPECTRUM. |
| **SATELLITE RX** | Uses a pass plan from the CPS to track an FM downlink with receive Doppler correction. Optional CPS install replaces BAND SWEEP. |
| **FM RADIO** | The redesigned broadcast FM app adds a signal meter and an auto-scan that fills empty preset slots. Available as a separate app package. |
| **Scan modes** | Press Band to switch between Weather, Police/Fire/EMS, Air, Repeaters, FRS, GMRS, MURS and more, or type a custom range |
| **BAND SWEEP** | Sweeps a whole band for activity |
| **RPT FINDER** | Works out a repeater's input frequency and tone |
| **FOX HUNT** | Signal meter and rough distance estimate for finding a transmitter |
| **SEARCH/QT SCAN** | Finds a nearby signal's frequency, or the tone on your channel; press 1 to switch |
| **Screens and startup tones** | Custom artwork, themes, and built-in or imported startup tunes on compatible AR-5RM builds |

See the `preview_*.png` screenshots in [UV-5RM Custom Firmware](UV-5RM%20Custom%20Firmware/).

## Check your radio before flashing

Radios that look identical can have different chips. Check the hardware identifier before choosing a firmware image.

1. Turn the radio off, then hold **8** while turning it on.
2. Read the text on the screen, then turn the radio off without pressing anything else.

| Screen says | What to do |
|---|---|
| `5RHNRF` v0.21 or v0.22 | [Install on AR-5RM NRF](#install-on-ar-5rm-nrf) |
| `5RHNRFB-YP07` | [Legacy UV-5RM installation](#legacy-uv-5rm-installation) |
| Other identifiers | Hardware compatibility is unverified |

### Experimental AR-5RM NRF build

The `ar5rm-nrf` source feature targets the AR-5RM that identifies itself as
`5RHNRF`. Its FD6818 startup and low-power PA values come from the published
[5RH NRF v0.14 reference image](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/tree/main/factory_firmware/NRF_FD6818).
On AR-5RM v0.21 and v0.22, the custom firmware booted and all nine apps passed installation read-back. The radios exchanged a TEXT ping and acknowledgment. FM receive, broadcast FM and the day/night shortcut were tested. With the AM squelch update and MONITOR off, the radio
played ATIS on 132.300 MHz at SQL 3 and stayed quiet on 132.400 MHz.

The persistence build skips the factory format. On first boot it checks that
the private SPI flash region at `0x1A0000..0x1BA000` is erased, then copies
the stock channel and contact tables there. The firmware uses the private copy
for edits, settings, FM presets, and the skull artwork. The artwork can be
uploaded with `cps_tool.py COM7 logo skull_logo.rgb565`. The full build displays
it on startup and plays the configured tune when boot sound is enabled.
TX starts inhibited on every boot and requires a valid low-power calibration
byte. A short press of the upper side key opens FM with the default assignment
and the FM app installed.

### Custom CPS for the AR-5RM

Open [AR-5RM_CPS.html](AR-5RM_CPS.html) in Chrome or Edge on Windows. It runs
locally. Connect the programming cable with the radio on its normal screen,
then click **Read radio** and choose its USB serial port. You can edit
channels, import or export a CHIRP CSV, and save a backup before uploading.

| CPS control | Workflow |
|---|---|
| **Screens & themes** | Select **Read appearance**, then choose a photo or a 160 × 128 RGB565 image. Compatible builds also support a separate home-screen background and the Default, Blue, Purple, and Sun themes. Preview changes and select **Write and verify**. |
| **Startup tones** | Select **Read appearance**, then load a built-in tune, import a MIDI or RTTTL file, or edit the notes. Select **Play preview**, then **Write and verify**. |
| **Text drafts** | Enter up to eight drafts, one per line, with up to 50 plain keyboard characters each. **Read from radio** loads existing drafts. **Upload and verify** backs up the TEXT slot and updates the app if needed. |
| **Radio Watch** | Enter up to six named lists of up to 32 saved channels each, using their CPS channel numbers or ranges (for example, `Local: 1, 3, 5-9`). The first **Upload and verify** backs up and replaces SPECTRUM. Later uploads update the lists. |
| **Satellite RX** | Enter a satellite label, listening location, FM downlink, and current two-line orbital elements (TLE). **Calculate passes** previews up to 12 passes over the next 48 hours with peak elevation of at least 10°. The first **Upload and verify** backs up and replaces BAND SWEEP. |

Startup tunes are monophonic and can hold up to 48 notes and rests. Durations
use 30 ms units and the pitch range is A2 to approximately F6. MIDI import uses
the first track with non-drum notes and skips overlapping notes. The CPS
downloads an appearance backup before changing artwork, theme, or startup
sound. Use **Open appearance backup** to stage a restore.

On the radio, open **RADIO WATCH**, choose a list, and press **Menu** to start
scanning. Press **#** for Recent Activity and **Menu** on a hit to listen.
In **SATELLITE RX**, press **\*** to set the radio's UTC clock, then select a
pass and press **Menu** to track it.

To regenerate the page, run `py source-changes\build_cps.py` from
`UV-5RM Custom Firmware`.

## What you need

- A Windows PC. The tools are Python, so other systems may work, but they're untested.
- [Python](https://www.python.org/downloads/), plus the serial library: `py -m pip install pyserial`
- A Baofeng programming cable. The commands below assume `COM3`, so change it to match your port.

Run these from the `UV-5RM Custom Firmware` folder.

## Install on AR-5RM NRF

The application image tested on the v0.21 and v0.22 is [AR-5RM_NRF_ROUNDED.BF](AR-5RM%20Firmware/AR-5RM_NRF_ROUNDED.BF). The matching [SETTINGS app](AR-5RM%20Firmware/SETTINGS_AR5RM_DAYNIGHT.app) adds the day/night key. Other supported hardware revisions remain unverified.

Back up factory channels and calibration while the radio is on normally. From `UV-5RM Custom Firmware`, replace `COM3` with the programming cable's port:

```powershell
mkdir my-backup
py source-changes\radio_read.py COM3 my-backup\factory_memory.img
py source-changes\radio_read.py COM3 my-backup\calibration.img --calibration
```

Then power on while holding both side buttons and check the bootloader with `py source-changes\bootloader_probe.py COM3`. Once it responds, flash the application:

```powershell
py source-changes\flash_bf.py "..\AR-5RM Firmware\AR-5RM_NRF_ROUNDED.BF" COM3 cb85e213682bb96dfa8718642719b8c5714ef204b18c029ff353bd0c1328bf94
```

The firmware image must be encoded with `source-changes\wrap_bf.py`. Do not wrap a raw binary with only a header: the bootloader acknowledges it but the application will not boot. A custom firmware image can be reinstalled through the bootloader. The stock application image has not been recovered.

After the radio boots to its main screen and you confirm that it receives, run
the app installer. The `--preserve-stock` option requires verified blank slots
for new apps and checks the TEXT log area before installing TEXT.

```powershell
py source-changes\install_new_apps.py --preserve-stock COM3 - 64 "..\AR-5RM Firmware\SETTINGS_AR5RM_DAYNIGHT.app" apps/text.app apps/chanmgr.app apps/searchqt.app apps/spectrum.app apps/bandsweep.app apps/foxhunt.app apps/rptfinder.app apps/fm.app
```

Use the CPS for the optional Radio Watch and Satellite RX replacements after
installing the base apps. The installer uses the standard `apps/fm.app`.
The [redesigned FM app](UV-5RM%20Custom%20Firmware/source-changes/fm_redesign.app)
is packaged separately. In that version, hold **#** to scan the broadcast band
and fill empty presets. Press **Exit** during the scan to stop and save.

Use **SETTINGS → KEYS** to assign `DAY/NIGHT` to a side key. On the tested
v0.22, holding the lower side button switches between Sun and the most recent
dark theme. Select **CHDISP → NAME** to show a large channel name and smaller
frequency.

## Legacy UV-5RM installation

**1. Back up the radio.** Flash mode can't read it, so do this first, with the radio on and on
its main screen. This only reads. It may not answer until the radio has been power-cycled, and
the folder has to exist.

```
mkdir my-backup
py source-changes\radio_read.py COM3 my-backup\factory_memory.img
```

**2. Enter flash mode.** Turn the radio off, hold **both side buttons** and turn it on. The screen
stays black. Check that it answers (this doesn't write anything):

```
py source-changes\bootloader_probe.py COM3
```

**3. Flash.** This is the point of no return. It takes under a minute, so don't touch the radio or
the cable. The long number is a checksum, and the tool refuses a file that doesn't match it.

```
py source-changes\flash_bf.py UV-5RM_Custom_Firmware.BF COM3 4340f205430f0741
```

**4. Install the logo and apps.** Wait for the radio to restart to its main screen. This reads
everything back to confirm it.

```
py source-changes\setup_radio.py COM3 skull_logo.rgb565 64 apps
```

**5. Turn on transmit.** It starts off as a safety step. Go to `SETTINGS` and set `TXINH` to off.

**6. Load channels (optional).** Open `UV-5RM_Channel_Uploader.html` in Chrome or Edge. The channel
list in this repo is for Clark County, Nevada, so check that it suits your area. Your radio should
keep whatever channels it already had.

To choose which channels get scanned, open `UV-5RM_Scan_Builder.html`, tick the ones you want, and
load the CSV it saves into the uploader.

Optional extras: `set_boot_tune.py` sets a Doom-style start-up riff, and `set_radio2_mario_tune.py`
sets a Super Mario one but only runs on one specific radio, so copy and edit it for yours.

## Using TEXT

Both radios need `TEXT` open, on the same simplex frequency. It uses the frequency the radio was on
when you opened it. Don't use a repeater.

| Key | Action |
|---|---|
| Number keys | Type, flip-phone style; the letters are shown at the top of the screen |
| **Menu** | Send |
| **Exit** | Delete a letter; stops retries while sending; closes the app when nothing is typed |
| Hold **Exit** | Close the app |
| **Up / Down** | Scroll saved messages |
| **\*** | Send a numbered test ping |
| Hold **\*** | Browse uploaded drafts with **Up / Down**. Press **Menu** to load one, then **Menu** again when ready to send. |
| **A/B** | Switch between low (default) and high power |
| Hold **#** twice | Erase all saved messages |

| Mark | Meaning |
|---|---|
| `>` | Sent, waiting for a reply |
| `+` | The other radio confirmed it |
| `!` | Never confirmed. It may still have arrived, since only the reply may have been lost |
| `<` | Received |

**Limits:**
- A radio only receives while `TEXT` is open.
- It's built for two radios; with more, the automatic replies would collide.
- Messages have **no encryption**, and they're stored unencrypted on the radio. Scrambling isn't
  security, so any radio with this app can read them.
- Range and long messages have barely been tested. Most testing was short pings a few feet apart.

## Legal

You're responsible for what you transmit.

- **`TEXT` transmits on its own.** With the app open, the radio sends a reply automatically to any
  message it receives, so stay on a frequency you're licensed for.
- **Ham bands:** you need a license and must identify with your callsign. `TEXT` doesn't send one,
  so you'd have to identify by voice. Encrypting messages isn't allowed.
- **FRS, GMRS and MURS:** this radio isn't type-accepted for them, so treat those channels as
  receive-only. Rules also vary by country.
- **Power:** output hasn't been measured, and the radio's actual power may differ from its label.

## Building from source

The [firmware source](UV-5RM%20Custom%20Firmware/firmware-source/) includes the
firmware and apps. Install [Rust](https://rustup.rs) and the `thumbv7em-none-eabi`
target. Run the commands from `UV-5RM Custom Firmware` and keep `--target-dir`
short to avoid linker path limits on Windows.

```
cargo build --release --manifest-path firmware-source/Cargo.toml --config firmware-source/.cargo/config.toml --target-dir C:/t/fw
py source-changes/elf2bin.py C:/t/fw/thumbv7em-none-eabi/release/bf5rh-fw fw.bin
```

For AR-5RM NRF, add `--features ar5rm-nrf` to the Cargo command and set the
size flags in the same PowerShell session before building. Package the output
as a one-region application `.BF` using `source-changes\wrap_bf.py`.

```powershell
$env:RUSTFLAGS = '-C link-arg=-Tlink.x -C link-arg=--icf=all -C llvm-args=-enable-machine-outliner=always'
```

`source-changes` holds the patches against upstream and the PC tools. `source-changes/simulator` runs
the apps against a simulated radio, with no hardware needed:

```
cargo build --release --manifest-path source-changes/simulator/Cargo.toml --target x86_64-pc-windows-msvc --target-dir C:/t/sim
C:/t/sim/x86_64-pc-windows-msvc/release/sim.exe out-folder skull_logo.rgb565
```

The output folder has to exist. The simulator only checks logic, so it can't prove how a real radio
behaves on the air.

See the [screen emulator guide](UV-5RM%20Custom%20Firmware/source-changes/emulator/README.md)
for an interactive preview of the radio UI and controls.

## Credits and license

- Firmware base: [bf-5rh-firmware-open](https://github.com/sophiel-meow/bf-5rh-firmware-open)
- Modem settings researched from the UV-K5 community's
  [messenger firmware](https://github.com/kamilsss655/uv-k5-firmware-custom)
- Licensed under GPL-3.0, the same as the firmware it's built on. See `LICENSE`.

Provided as is, with no warranty.
