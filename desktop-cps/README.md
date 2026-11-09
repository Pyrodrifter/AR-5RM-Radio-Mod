# AR-5RM CPS Desktop

The Windows desktop app uses the same channel editor and radio protocol as the offline CPS page. It runs locally and includes its own Electron runtime.

## Open the app

Extract the Windows x64 ZIP and run `AR-5RM-CPS.exe` from the extracted folder. Connect the programming cable with the radio on its normal screen. Choose **Read radio**, select the programming cable, then choose **Connect**. Use **Change port** when swapping cables.

The app saves radio backups under **Documents → AR-5RM CPS → Backups** before writing. **File → Open backup folder** opens that location. **Save CSV** opens the Windows save dialog.

## Run from source on Windows

With Node.js 24 and npm installed, double-click `Start CPS.cmd` in this folder. The launcher installs missing dependencies and opens the CPS from the repository. The first launch may download the Electron runtime.

To launch or package it from a terminal, run these commands in this directory.

```powershell
npm ci
npm start
npm run package:win
```

The packaged app is in `out/AR-5RM-CPS-win32-x64`. Distribute the entire folder with the executable and its runtime files.

The desktop app bundles the repository's `AR-5RM_CPS.html`. After editing the shared CPS template, regenerate the HTML with `py source-changes/build_cps.py` from the `UV-5RM Custom Firmware` directory before packaging.
