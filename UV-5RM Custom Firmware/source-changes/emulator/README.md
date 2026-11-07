# Screen emulator

A window on the PC that shows the radio's 160×128 screen, so you can work on app UI without
flashing anything. It runs the real app sources from `firmware-source/apps` and the firmware's own
menu-list and backdrop drawing code, so what you see is what the radio draws.

```
cargo run --release --manifest-path source-changes/emulator/Cargo.toml --target-dir C:/t/emu
```

The first start builds every app (about a minute). After that, edit an app's source and press
**F5** in the window: it rebuilds just what changed and returns to MENU.

## Keys

The window is drawn as a radio: click the on-screen keypad, the side keys (PTT, FM, LIGHT) and the
toolbar (SIGNAL, REBUILD, SCREENSHOT), or use the keyboard. A key held on the keyboard lights up its
on-screen button. The TX and RX lights show when an app is transmitting and when a signal is
present. The window can be resized.

| PC key | Radio key |
|---|---|
| `0`–`9` (or numpad) | digits |
| `Q` or numpad `*` | `*` |
| `W` or numpad `/` | `#` |
| `Enter` | MENU |
| `Esc` or `Backspace` | EXIT |
| `Up` / `Down` | arrows |
| `V` | V/M |
| `A` | A/B |
| `B` | BAND |
| `Z` | side key 1 (FM) |
| `X` | side key 2 (flashlight) |
| `Space` | PTT. Apps see it, nothing transmits |
| hold `S` | pretend a strong signal is on frequency (meters, squelch). The SIGNAL button latches it on |
| `T` | next colour theme (classic, blue, purple), as SETTINGS > DISP > THEME sets it |
| `F5` | rebuild apps from source |
| `F12` | screenshot to `source-changes/emulator/screenshots/` |

Holding a key for a second gives a long press, and holding Up/Down repeats, with the same timing as
`device/keypad.rs`.

## Options

- `--ar5rm`: report the AR-5RM's RF chip ID (0x4819) to apps.
- `--image FILE`: load channels from a `radio_read.py`/CHIRP image instead of the demo channels.
- `--background FILE`: a home-screen background, as 40960 bytes of little-endian RGB565 (what the
  CPS tool uploads) or a 160x128 24-bit BMP.
- `--scale 2|3|4`: LCD pixel size inside the radio body (default 3).
- `--selftest DIR`: no window. Opens every app, saves screenshots to `DIR` (including
  `00_window.bmp`, the whole radio body), and exits non-zero if any app fails to load or faults.

## How it works

- The launcher lists the packages from `source-changes/pack_apps.py`, in that order.
- Each app segment is built as a DLL from an unmodified copy of its source (`src/appbuild.rs`
  drops `#![no_std]`, `#![no_main]` and `#[panic_handler]`, nothing else). Builds go to
  `%TEMP%\uv5rm-emu`, or `EMU_BUILD_DIR` if set.
- Every launch loads a fresh copy of the DLL, so an app starts with clean state, as it does when the
  radio loads it into its RAM arena. Multi-segment apps (SETTINGS, SEARCH/QT SCAN) chain the same way.
- Apps run on their own thread. A panic or `app_fault` returns to MENU with the reason in the title
  bar instead of closing the emulator.
- SPI flash, settings and FM channels are kept in `state/` between runs. Delete it to start fresh.

## Home screen

The window starts on the home (standby) screen, drawn by the firmware's own `ui/standby.rs`. It
reads the radio through a stand-in for the firmware's `App` (`src/app.rs`) that has the same
methods. Its key handling is a simplified copy of the radio's:

- digits type a frequency in VFO mode (`146520` gives 146.520 MHz) or a channel number in channel
  mode (three digits, or MENU to accept),
- Up/Down step 12.5 kHz in VFO mode, or move to the next stored channel,
- V/M switches VFO and channel mode, A/B switches sides, EXIT clears what you've typed,
- MENU opens the app list, EXIT there comes back,
- the side keys run the function set in SETTINGS (FM RADIO opens the FM app, which returns here),
- long `*` locks the keys, and PTT shows TX, or TX PROHIBITED while TX is inhibited in settings.

Scan, the BAND modes menu, DTMF dialling and dual watch aren't emulated.

## Limits

- No RF. RSSI is noise unless you hold `S`, nothing is ever received or transmitted, and the TEXT
  app can't exchange messages. Use `source-changes/simulator` for logic checks with simulated signals.
- No sound.
- It shows the layout and drawing, but not real-radio timing or the LCD's colours exactly.
