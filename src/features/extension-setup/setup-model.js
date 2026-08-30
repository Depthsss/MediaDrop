const GUIDES = {
  opera_gx: {
    page: "opera:extensions",
    shortcut: "Ctrl+Shift+E",
    launchesInternalPage: false,
    developerMode: "Sağ üstteki Geliştirici modu seçeneğini aç.",
    loadUnpacked: "MediaDrop kuruluysa kartındaki Yenile düğmesine bas; değilse Paketlenmemiş öğe yükle düğmesine bas.",
  },
  opera: {
    page: "opera:extensions",
    shortcut: "Ctrl+Shift+E",
    launchesInternalPage: false,
    developerMode: "Sağ üstteki Geliştirici modu seçeneğini aç.",
    loadUnpacked: "MediaDrop kuruluysa kartındaki Yenile düğmesine bas; değilse Paketlenmemiş öğe yükle düğmesine bas.",
  },
  chrome: {
    page: "chrome://extensions",
    shortcut: "",
    launchesInternalPage: true,
    developerMode: "Sağ üstteki Geliştirici modu anahtarını aç.",
    loadUnpacked: "MediaDrop kuruluysa kartındaki Yenile simgesine bas; değilse sol üstteki Paketlenmemiş öğe yükle düğmesine bas.",
  },
  edge: {
    page: "edge://extensions",
    shortcut: "",
    launchesInternalPage: true,
    developerMode: "Sol menüdeki Geliştirici modu anahtarını aç.",
    loadUnpacked: "MediaDrop kuruluysa kartındaki Yenile düğmesine bas; değilse Paketlenmemiş öğe yükle düğmesine bas.",
  },
};

function browserPriority(browser) {
  if (browser?.defaultBrowser) return 0;
  if (browser?.installed && browser?.recommended) return 1;
  if (browser?.installed) return 2;
  return 3;
}

export function orderExtensionBrowsers(browsers = []) {
  return [...browsers].sort((left, right) => browserPriority(left) - browserPriority(right));
}

export function extensionGuideForBrowser(browserId = "") {
  const guide = GUIDES[String(browserId || "").trim()];
  return guide ? { ...guide } : null;
}

export function extensionSetupStepStates({ selected = false, opened = false, connected = false } = {}) {
  if (connected) return ["complete", "complete", "complete", "complete"];
  if (opened) return ["complete", "complete", "current", "pending"];
  if (selected) return ["complete", "current", "pending", "pending"];
  return ["current", "pending", "pending", "pending"];
}
