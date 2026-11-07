# AR-5RM Radio Mod
This repo contains new firmware for Baofeng UV5-RM radios adding new apps including texting. CAUTION: CHIRP will no longer work so use the included apps to program channels and categories

Custom firmware and add-on apps for the Baofeng UV-5RM, built on the open-source
[bf-5rh-firmware-open](https://github.com/sophiel-meow/bf-5rh-firmware-open). The main addition
is `TEXT`: short text messages between two radios, with delivery confirmation and no phone,
internet or extra hardware.

> **Stock firmware recovery is unavailable.** No copy of the factory firmware is known for this hardware, so
> there's no way back to stock, and CHIRP stops working with the radio. A bad flash could brick
> it. This has only been run on two radios, so use it at your own risk.

## What it adds

| Feature | What it does |
|---|---|
| **TEXT** | Sends messages up to 50 characters, retries until confirmed, saves them, and plays a tone when one arrives |
| **Scan modes** | Press Band to switch between Weather, Police/Fire/EMS, Air, Repeaters, FRS, GMRS, MURS and more, or type a custom range |
| **BAND SWEEP** | Sweeps a whole band for activity |
| **RPT FINDER** | Works out a repeater's input frequency and tone |
| **FOX HUNT** | Signal meter and rough distance estimate for finding a transmitter |
| **SEARCH/QT SCAN** | Finds a nearby signal's frequency, or the tone on your channel; press 1 to switch |
| **Look and feel** | Skull start-up logo, black and red menus, and a custom start-up tune |

Screenshots: see the `preview_*.png` files in `UV-5RM Custom Firmware`.

## Before you start: check your radio

Radios that look identical can have different chips. Check the hardware identifier before choosing a firmware image.

1. Turn the radio off, then hold **8** while turning it on.
2. Read the text on the screen, then turn the radio off without pressing anything else.

| Screen says | What to do |
|---|---|
| `5RHNRF` v0.21 or v0.22 | Use the experimental AR-5RM NRF build described below |
| `5RHNRFB-YP07` | Use the original UV-5RM firmware described under Legacy UV-5RM installation |
| Other identifiers | Hardware compatibility is unverified |

### Experimental AR-5RM NRF build

The `ar5rm-nrf` source feature targets the AR-5RM that identifies itself as
`5RHNRF`. Its FD6818 startup and low-power PA values come from the published
[5RH NRF v0.14 reference image](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/tree/main/factory_firmware/NRF_FD6818).
On AR-5RM v0.21 and v0.22, the custom firmware booted and all nine apps passed installation read-back. The radios exchanged a TEXT ping and acknowledgment. FM receive, broadcast FM and the day/night shortcut were tested. With the AM squelch update and MONITOR off, the radio
played ATIS on 132.300 MHz at SQL 3 and stayed quiet on 132.400 MHz.

Use Rust 1.99.1 and `--features ar5rm-nrf` for this target. The full build
needs the following size flags in the PowerShell session that runs Cargo.
Keep the packaged output to a one-region application `.BF`. A programming-data
backup and a bootloader response do not establish a way back to stock v0.21.

```powershell
$env:RUSTFLAGS = '-C link-arg=-Tlink.x -C link-arg=--icf=all -C llvm-args=-enable-machine-outliner=always'
```

While the stock firmware still runs, `radio_read.py PORT OUTPUT --calibration`
saves the raw factory calibration blocks separately from the channel backup.

The persistence build skips the factory format. On first boot it checks that
the private SPI flash region at `0x1A0000..0x1BA000` is erased, then copies
the stock channel and contact tables there. The firmware uses the private copy
for edits, settings, FM presets, and the skull artwork. The artwork can be
uploaded with `cps_tool.py COM7 logo skull_logo.rgb565`. The full build displays
it on startup and plays the configured tune when boot sound is enabled.
TX starts inhibited on every boot and requires a valid low-power calibration
byte. A short press of the upper side key opens FM with the default assignment
and the FM app installed.

After a successful bench boot and receive check, the installer can provision
the apps in verified blank slots and check TEXT's log area before it writes.
Run this from `UV-5RM Custom Firmware`, substituting the actual COM port.

```
py source-changes\install_new_apps.py --preserve-stock COM7 - 64 "..\AR-5RM Firmware\SETTINGS_AR5RM_DAYNIGHT.app" apps/text.app apps/chanmgr.app apps/searchqt.app apps/spectrum.app apps/bandsweep.app apps/foxhunt.app apps/rptfinder.app apps/fm.app
```

### Custom CPS for the AR-5RM

Open [AR-5RM_CPS.html](AR-5RM_CPS.html) in Chrome or Edge on Windows. It runs
locally. Connect the programming cable with the radio on its normal screen,
then click **Read radio** and choose its USB serial port. You can edit
channels, import or export a CHIRP CSV, and save a backup before uploading.

Open **Radio appearance** and click **Read appearance**. Choose a photo or a
160 × 128 RGB565 image and preview it before clicking **Write and verify**.
The startup tune editor accepts up to 48 note and duration pairs and provides
default and Doom presets with an audio preview. MIDI (`.mid` or `.midi`) and
RTTTL ringtone files can be imported into the tune editor. MIDI import uses the
first track with non-drum notes and skips overlapping notes. The radio is
monophonic, supports 48 note/rest entries in 30 ms units, and can play pitches
from A2 to approximately F6. Files outside those limits must be edited first.
The CPS downloads an
appearance backup before replacing artwork, a theme, or the tune. Use
**Open appearance backup** to stage that backup for restoration.

The `AR-5RM_NRF_FULL.BF` build uses the startup image for menu artwork as
well. The UI build adds a separate home-screen background and the Classic,
Blue, Purple, and Sun themes. The CPS checks the radio before enabling
those controls. A background becomes active after its upload is verified.
The DAY/NIGHT key function switches between Sun and the most recently chosen
dark theme. On the tested v0.22, holding the lower side button runs it. The
assignment can be changed under SETTINGS → KEYS.

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

The firmware image does not install apps or artwork. The tested radios have nine separate apps installed, including TEXT and the matching SETTINGS app. Provision apps separately using the AR-5RM installer, and preserve existing slots and radio data. Assign `DAY/NIGHT` to the desired side key in SETTINGS → KEYS. Select `CHDISP` → `NAME` to show a large channel name and smaller frequency.

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

`firmware-source` is the complete source of the provided firmware and apps; the firmware and `TEXT`
were confirmed to rebuild byte for byte. It needs [Rust](https://rustup.rs) and the target in its
`rust-toolchain.toml`. Use a short `--target-dir`, because long paths break linking on Windows.

```
cargo build --release --manifest-path firmware-source/Cargo.toml --config firmware-source/.cargo/config.toml --target-dir C:/t/fw
py source-changes/elf2bin.py C:/t/fw/thumbv7em-none-eabi/release/bf5rh-fw fw.bin
```

`source-changes` holds the patches against upstream and the PC tools. `source-changes/simulator` runs
the apps against a simulated radio, with no hardware needed:

```
cargo build --release --manifest-path source-changes/simulator/Cargo.toml --target x86_64-pc-windows-msvc --target-dir C:/t/sim
C:/t/sim/x86_64-pc-windows-msvc/release/sim.exe out-folder skull_logo.rgb565
```

The output folder has to exist. The simulator only checks logic, so it can't prove how a real radio
behaves on the air.

## Credits and license

- Firmware base: [bf-5rh-firmware-open](https://github.com/sophiel-meow/bf-5rh-firmware-open)
- Modem settings researched from the UV-K5 community's
  [messenger firmware](https://github.com/kamilsss655/uv-k5-firmware-custom)
- Licensed under GPL-3.0, the same as the firmware it's built on. See `LICENSE`.

Provided as is, with no warranty.
