const { app, BrowserWindow, dialog, ipcMain, Menu, protocol, shell } = require("electron");
const fs = require("node:fs/promises");
const path = require("node:path");

const pageUrl = "cps://desktop/";
const externalLinks = new Set([
  "https://celestrak.org/NORAD/elements/gp.php?CATNR=25544&FORMAT=TLE",
  "https://www.ariss.org/current-status-of-iss-stations.html",
]);
const csp = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join("; ");

protocol.registerSchemesAsPrivileged([{
  scheme: "cps",
  privileges: { standard: true, secure: true },
}]);

function appOrigin(url) {
  return url === pageUrl || url === pageUrl.slice(0, -1);
}

function backupsFolder() {
  return path.join(app.getPath("documents"), "AR-5RM CPS", "Backups");
}

async function serveCps() {
  const resourceDir = app.isPackaged ? process.resourcesPath : path.resolve(__dirname, "..");
  const html = await fs.readFile(path.join(resourceDir, "AR-5RM_CPS.html"), "utf8");
  const desktop = await fs.readFile(path.join(__dirname, "desktop.js"), "utf8");
  const script = /<script>([\s\S]*?)<\/script>/;
  const match = script.exec(html);
  if (!match) throw new Error("The bundled CPS page is missing its script.");
  const page = html.replace(script, '<script src="cps.js"></script><script src="desktop.js"></script>');
  const assets = new Map([
    [pageUrl, [page, "text/html"]],
    [pageUrl + "cps.js", [match[1], "text/javascript"]],
    [pageUrl + "desktop.js", [desktop, "text/javascript"]],
  ]);
  protocol.handle("cps", request => {
    const asset = assets.get(request.url);
    if (request.method !== "GET" || !asset) return new Response(null, { status: 404 });
    return new Response(asset[0], {
      headers: {
        "Content-Type": asset[1] + "; charset=utf-8",
        "Content-Security-Policy": csp,
        "X-Content-Type-Options": "nosniff",
      },
    });
  });
}

async function writeBackup(name, bytes) {
  const directory = backupsFolder();
  await fs.mkdir(directory, { recursive: true });
  const { name: stem, ext } = path.parse(name);
  for (let suffix = 0; ; suffix++) {
    const destination = path.join(directory, suffix ? `${stem}-${suffix}${ext}` : name);
    let file;
    try {
      file = await fs.open(destination, "wx");
    } catch (error) {
      if (error.code === "EEXIST") continue;
      throw error;
    }
    try {
      await file.writeFile(bytes);
      await file.sync();
    } catch (error) {
      await file.close();
      await fs.rm(destination, { force: true });
      throw error;
    }
    await file.close();
    return true;
  }
}

async function openBackups() {
  try {
    await fs.mkdir(backupsFolder(), { recursive: true });
    const error = await shell.openPath(backupsFolder());
    if (error) throw new Error(error);
  } catch (error) {
    dialog.showErrorBox("Could not open backups", error.message);
  }
}

function installBridge(win) {
  const contents = win.webContents;
  const session = contents.session;
  let pending;
  const allowed = event => event.sender === contents && event.senderFrame === contents.mainFrame
    && event.senderFrame?.url === pageUrl;
  const serialAllowed = (sender, permission, origin, details) =>
    sender === contents && permission === "serial" && details.isMainFrame && appOrigin(origin);

  session.setPermissionCheckHandler(serialAllowed);
  session.setPermissionRequestHandler((sender, permission, callback, details) => {
    callback(serialAllowed(sender, permission, details.requestingUrl, details));
  });

  function sendPorts() {
    if (pending) contents.send("cps:ports", Array.from(pending.ports.values(), port => ({
      id: port.portId,
      name: port.portName,
      label: port.displayName,
    })));
  }

  function finish(portId) {
    if (!pending) return;
    const { callback } = pending;
    pending = undefined;
    callback(portId);
  }

  session.on("select-serial-port", (event, ports, sender, callback) => {
    event.preventDefault();
    if (sender !== contents || sender.getURL() !== pageUrl) {
      callback("");
      return;
    }
    finish("");
    pending = { callback, ports: new Map(ports.map(port => [port.portId, port])) };
    sendPorts();
  });
  session.on("serial-port-added", (_event, port, sender) => {
    if (sender !== contents || !pending) return;
    pending.ports.set(port.portId, port);
    sendPorts();
  });
  session.on("serial-port-removed", (_event, port, sender) => {
    if (sender !== contents || !pending) return;
    pending.ports.delete(port.portId);
    sendPorts();
  });
  ipcMain.on("cps:choose-port", (event, id) => {
    if (!allowed(event) || !pending) return;
    if (id === "" || pending.ports.has(id)) finish(id);
    else sendPorts();
  });
  contents.on("render-process-gone", () => finish(""));
  win.on("closed", () => finish(""));

  ipcMain.handle("cps:open-backups", event => {
    if (!allowed(event)) throw new Error("The CPS window is required to open backups.");
    return openBackups();
  });
  ipcMain.handle("cps:save-file", async (event, name, data) => {
    if (!allowed(event)) throw new Error("The CPS window is required to save files.");
    if (typeof name !== "string" || !/^[a-z0-9-]+\.(csv|bin|json|app)$/i.test(name)) {
      throw new Error("Invalid CPS file name.");
    }
    if (typeof data !== "string" && !ArrayBuffer.isView(data)) {
      throw new Error("Invalid CPS file data.");
    }
    const bytes = Buffer.from(data);
    if (path.extname(name).toLowerCase() !== ".csv") return writeBackup(name, bytes);
    const result = await dialog.showSaveDialog(win, {
      title: "Save channels",
      defaultPath: path.join(app.getPath("documents"), name),
      filters: [{ name: "CHIRP CSV", extensions: ["csv"] }],
    });
    if (result.canceled) return false;
    await fs.writeFile(result.filePath, bytes, { flush: true });
    return true;
  });
}

function installMenu(win) {
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    {
      label: "File",
      submenu: [{
        label: "Open backup folder",
        click: openBackups,
      }, { type: "separator" }, { role: "quit" }],
    },
    { role: "editMenu" },
    { label: "View", submenu: [{ role: "resetZoom" }, { role: "zoomIn" }, { role: "zoomOut" }] },
  ]));
  win.webContents.on("will-prevent-unload", event => {
    const choice = dialog.showMessageBoxSync(win, {
      type: "warning",
      buttons: ["Stay", "Close CPS"],
      defaultId: 0,
      cancelId: 0,
      message: "The CPS has unsaved changes or an active operation.",
      detail: "Finish any radio transfer before closing the app.",
    });
    if (choice === 1) event.preventDefault();
  });
}

async function start() {
  await serveCps();
  const win = new BrowserWindow({
    width: 1280,
    height: 840,
    minWidth: 760,
    minHeight: 560,
    backgroundColor: "#131110",
    title: "AR-5RM CPS",
    titleBarStyle: "hidden",
    titleBarOverlay: { color: "#241b14", symbolColor: "#c9bdad", height: 48 },
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  installBridge(win);
  installMenu(win);
  win.webContents.on("will-navigate", event => event.preventDefault());
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (externalLinks.has(url)) shell.openExternal(url).catch(error => {
      dialog.showErrorBox("Could not open link", error.message);
    });
    return { action: "deny" };
  });
  app.on("second-instance", () => {
    if (win.isDestroyed()) return;
    if (win.isMinimized()) win.restore();
    win.focus();
  });
  await win.loadURL(pageUrl);
}

if (app.requestSingleInstanceLock()) {
  app.whenReady().then(start).catch(error => {
    dialog.showErrorBox("Could not start AR-5RM CPS", error.message);
    app.exit(1);
  });
  app.on("window-all-closed", () => app.quit());
} else {
  app.quit();
}
