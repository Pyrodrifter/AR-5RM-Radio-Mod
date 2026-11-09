(() => {
  const bridge = window.desktopCps;
  document.documentElement.classList.add("desktop");
  const icon = name => `<svg class="icon" aria-hidden="true"><use href="#icon-${name}"/></svg>`;
  const picker = document.createElement("dialog");
  picker.id = "portPicker";
  picker.setAttribute("aria-labelledby", "portTitle");
  picker.innerHTML = `
    <form method="dialog">
      <p class="kicker">Radio transfer</p>
      <h2 id="portTitle">Choose the programming cable</h2>
      <p class="muted">Leave the radio on its normal screen while connecting.</p>
      <label class="stacked-field">Serial port
        <select id="serialPort" autofocus></select>
      </label>
      <p id="portStatus" class="muted" role="status"></p>
      <div class="actions dialog-footer">
        <button value="cancel" formnovalidate>Cancel</button>
        <button id="connectPort" class="primary" value="connect">Connect</button>
      </div>
    </form>`;
  document.body.append(picker);
  const select = picker.querySelector("select");
  const connect = picker.querySelector("#connectPort");
  const status = picker.querySelector("#portStatus");

  bridge.onPorts(ports => {
    const previous = select.value;
    const placeholder = new Option("Select a serial port", "", true, true);
    placeholder.disabled = true;
    select.replaceChildren(placeholder, ...ports.map(({ id, name, label }) => {
      const option = document.createElement("option");
      option.value = id;
      option.textContent = label && name && !label.includes(name) ? `${label} (${name})` : label || name || "Serial device";
      return option;
    }));
    if (ports.some(port => port.id === previous)) select.value = previous;
    connect.disabled = !select.value;
    status.textContent = ports.length ? "Select the USB serial port for your radio." : "Waiting for a serial cable.";
    if (!picker.open) {
      picker.showModal();
    }
  });
  select.addEventListener("change", () => { connect.disabled = !select.value; });
  picker.addEventListener("submit", event => {
    event.preventDefault();
    picker.close();
    bridge.choosePort(event.submitter?.value === "connect" ? select.value : "");
  });
  picker.addEventListener("cancel", event => {
    event.preventDefault();
    picker.close();
    bridge.choosePort("");
  });

  const backups = document.createElement("button");
  backups.className = "quiet with-icon";
  backups.innerHTML = icon("archive") + "Backups";
  backups.title = "Open the backup folder";
  document.getElementById("fileActions").append(backups);
  backups.addEventListener("click", () => {
    bridge.openBackups().catch(error => setNotice(describeError(error), "err"));
  });

  const changePort = document.createElement("button");
  changePort.className = "quiet with-icon";
  changePort.innerHTML = icon("cable") + "Change port";
  document.getElementById("readBtn").before(changePort);
  changePort.addEventListener("click", () => {
    if (busy || appearanceBusy || draftsBusy || watchBusy || satBusy) return;
    port = null;
    setNotice("Choose a port on your next radio operation.");
  });
  navigator.serial?.addEventListener("disconnect", event => {
    if (event.target === port) port = null;
  });
})();
