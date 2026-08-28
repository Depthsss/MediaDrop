export function unitToMb(value, unit) {
  const number = Number(String(value).replace(",", "."));
  if (!Number.isFinite(number)) return null;

  const normalized = String(unit).toLowerCase();

  if (normalized.includes("gib") || normalized.includes("gb")) {
    return number * 1024;
  }

  if (normalized.includes("kib") || normalized.includes("kb")) {
    return number / 1024;
  }

  return number;
}

export function parseFallbackProgressLine(line, percent) {
  const text = String(line || "").replace(/\s+/g, " ");

  const totalMatch = text.match(/of\s+([\d.,]+)\s*(KiB|MiB|GiB|KB|MB|GB)/i);
  const speedMatch = text.match(/at\s+([\d.,]+)\s*(KiB|MiB|GiB|KB|MB|GB)\/s/i);

  const totalMb = totalMatch ? unitToMb(totalMatch[1], totalMatch[2]) : null;
  const speedMb = speedMatch ? unitToMb(speedMatch[1], speedMatch[2]) : null;

  const parts = [];

  if (typeof percent === "number" && Number.isFinite(percent) && totalMb) {
    const downloadedMb = (totalMb * percent) / 100;
    parts.push(`${downloadedMb.toFixed(1)} / ${totalMb.toFixed(1)} MB`);
  }

  if (speedMb) {
    parts.push(`${speedMb.toFixed(2)} MB/s`);
  }

  if (parts.length) return parts.join(" • ");

  if (text.includes("Destination")) return "Dosya hazırlanıyor...";
  if (text.includes("[Merger]")) return "Video ve ses birleştiriliyor...";
  if (text.includes("[ExtractAudio]")) return "Ses MP3'e dönüştürülüyor...";
  if (text.includes("Deleting original")) return "Geçici dosyalar temizleniyor...";
  if (text.includes("İndirme tamamlandı")) return "Tamamlandı.";

  return text
    .replace("[download]", "")
    .replace("[Merger]", "Birleştiriliyor:")
    .replace("[ExtractAudio]", "Ses dönüştürülüyor:")
    .trim()
    .slice(0, 120);
}

export function asNumber(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

export function progressJobId(payload = {}) {
  const value = payload?.jobId ?? payload?.job_id;
  return String(value ?? "").trim();
}

export function downloadCancellationCompletion(state) {
  return state === "paused" ? "immediate" : "deferred";
}

export function clipDownloadStatusText(payload = {}) {
  const text = `${payload.phase || ""} ${payload.line || ""}`.toLowerCase();

  if (text.includes("tamam")) return "Video tamamlanıyor...";
  if (text.includes("doğrulan")) return "Video doğrulanıyor...";

  if (
    text.includes("birleştir") ||
    text.includes("işlen") ||
    text.includes("kesil") ||
    text.includes("temizlen") ||
    text.includes("encode") ||
    text.includes("derlen")
  ) {
    return "Video derleniyor...";
  }

  if (text.includes("ses") && text.includes("indir")) return "Ses indiriliyor...";

  if (
    text.includes("indir") ||
    text.includes("byte") ||
    text.includes("range") ||
    text.includes("stream") ||
    text.includes("hls") ||
    text.includes("segment")
  ) {
    return "Video indiriliyor...";
  }

  if (
    text.includes("hazırl") ||
    text.includes("aran") ||
    text.includes("indeks") ||
    text.includes("url") ||
    text.includes("çöz")
  ) {
    return "Klip hazırlanıyor...";
  }

  return "Klip işleniyor...";
}

export function displayProgressPercent(percent, rawLine, clipActive) {
  const safePercent = Math.max(0, Math.min(100, percent));

  if (clipActive && String(rawLine || "").includes("[download]")) {
    return Math.min(82, safePercent * 0.82);
  }

  return safePercent;
}
