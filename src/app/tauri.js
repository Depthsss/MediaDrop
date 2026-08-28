function tauriApi() {
  const api = globalThis.window?.__TAURI__;
  if (!api?.core || !api?.event) {
    throw new Error("Tauri API hazır değil.");
  }
  return api;
}

export function invoke(command, args) {
  return tauriApi().core.invoke(command, args);
}

export function convertFileSrc(path, protocol) {
  return tauriApi().core.convertFileSrc(path, protocol);
}

export function listen(event, handler) {
  return tauriApi().event.listen(event, handler);
}

export function openDialog(options) {
  const open = tauriApi().dialog?.open;
  if (typeof open !== "function") {
    throw new Error("Tauri dialog API hazır değil.");
  }
  return open(options);
}

export function readClipboardText() {
  return invoke("plugin:clipboard-manager|read_text");
}

export function writeClipboardText(text) {
  return invoke("plugin:clipboard-manager|write_text", { text });
}

export function relaunch() {
  const restart = tauriApi().process?.relaunch;
  if (typeof restart !== "function") {
    throw new Error("Tauri process API hazır değil.");
  }
  return restart();
}

export function checkForUpdate() {
  const check = tauriApi().updater?.check;
  if (typeof check !== "function") {
    throw new Error("Tauri updater API hazır değil.");
  }
  return check();
}
