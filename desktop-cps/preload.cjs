const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("desktopCps", {
  onPorts: listener => {
    ipcRenderer.on("cps:ports", (_event, ports) => listener(ports));
  },
  choosePort: id => ipcRenderer.send("cps:choose-port", id),
  saveFile: (name, data) => ipcRenderer.invoke("cps:save-file", name, data),
  openBackups: () => ipcRenderer.invoke("cps:open-backups"),
});
