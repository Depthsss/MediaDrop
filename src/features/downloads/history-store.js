export const DOWNLOAD_HISTORY_LIMIT = 60;

export function normalizePath(value) {
  return String(value || "").trim();
}

export function basename(path) {
  const clean = normalizePath(path);
  if (!clean) return "Dosya";
  return clean.split(/[\\/]/).filter(Boolean).pop() || clean;
}

export function historyTimeMs(item) {
  const direct = Number(item?.downloadedAtMs || 0);
  if (Number.isFinite(direct) && direct > 0) return direct;

  const parsed = Date.parse(item?.downloadedAt || "");
  return Number.isFinite(parsed) ? parsed : null;
}

export function readDownloadHistory(storage, key) {
  try {
    const parsed = JSON.parse(storage.getItem(key) || "[]");
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function writeDownloadHistory(storage, key, items) {
  const bounded = Array.isArray(items) ? items.slice(0, DOWNLOAD_HISTORY_LIMIT) : [];
  storage.setItem(key, JSON.stringify(bounded));
  return bounded;
}

export function removeDownloadHistoryItem(items, item) {
  const targetId = String(item?.id || "");
  const targetPath = normalizePath(item?.filePath);

  if (!targetId && !targetPath) return [...items];

  return items.filter((entry) => {
    if (targetId && String(entry?.id || "") === targetId) return false;
    if (targetPath && normalizePath(entry?.filePath) === targetPath) return false;
    return true;
  });
}

export function prependDownloadHistoryItem(items, item, now = Date.now(), randomId = "") {
  const filePath = normalizePath(item?.filePath);
  const filtered = items.filter((entry) => normalizePath(entry?.filePath) !== filePath);
  const timestamp = Number.isFinite(Number(now)) ? Number(now) : Date.now();

  filtered.unshift({
    id: `${timestamp}-${randomId || Math.random().toString(16).slice(2)}`,
    title: item?.title || basename(filePath),
    platform: item?.platform || "generic",
    quality: item?.quality || "Otomatik",
    url: item?.url || "",
    filePath,
    outputDir: item?.outputDir || "",
    fileSize: Number(item?.fileSize || 0),
    downloadedAtMs: timestamp,
    downloadedAt: new Date(timestamp).toISOString(),
  });

  return filtered.slice(0, DOWNLOAD_HISTORY_LIMIT);
}
