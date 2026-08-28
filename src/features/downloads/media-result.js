function firstDefined(source, keys) {
  for (const key of keys) {
    if (source?.[key] !== undefined && source?.[key] !== null) return source[key];
  }
  return undefined;
}

function cleanText(value) {
  return String(value ?? "").trim();
}

function nonNegativeInteger(value, fallback = 0) {
  if (value === undefined || value === null || value === "") return fallback;
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? Math.trunc(number) : fallback;
}

function normalizeFile(file) {
  return {
    filePath: cleanText(firstDefined(file, ["filePath", "file_path"])),
    fileSize: nonNegativeInteger(firstDefined(file, ["fileSize", "file_size"])),
    title: cleanText(file?.title),
    sourceIndex: nonNegativeInteger(firstDefined(file, ["sourceIndex", "source_index"])),
  };
}

function normalizeFailure(failure) {
  return {
    itemId: cleanText(firstDefined(failure, ["itemId", "item_id"])),
    sourceIndex: nonNegativeInteger(firstDefined(failure, ["sourceIndex", "source_index"])),
    message: cleanText(failure?.message) || "Medya indirilemedi.",
  };
}

export function normalizeMediaDownloadResult(result) {
  const source = result && typeof result === "object" ? result : {};
  const files = Array.isArray(source.files) ? source.files.map(normalizeFile) : [];
  const failures = Array.isArray(source.failures) ? source.failures.map(normalizeFailure) : [];
  const downloadedCount = nonNegativeInteger(
    firstDefined(source, ["downloadedCount", "downloaded_count"]),
    files.length
  );
  const failedCount = nonNegativeInteger(
    firstDefined(source, ["failedCount", "failed_count"]),
    failures.length
  );

  return {
    message: cleanText(source.message || result) || "İndirme tamamlandı.",
    files,
    failures,
    filePath: files[0]?.filePath || "",
    outputDir: cleanText(firstDefined(source, ["outputDir", "output_dir"])),
    downloadedCount,
    failedCount,
    mode: cleanText(source.mode),
    fileSize: files.reduce((total, file) => total + file.fileSize, 0),
  };
}

function batchMediaLabel(args) {
  if (args?.scope === "all-stories" || nonNegativeInteger(args?.storyCount) > 0) {
    return "hikaye";
  }

  const photoCount = nonNegativeInteger(args?.photoCount);
  const videoCount = nonNegativeInteger(args?.videoCount);
  if (photoCount > 0 && videoCount > 0) return "medya";
  if (videoCount > 0 || (args?.hasVideo && photoCount === 0)) return "video";
  return "fotoğraf";
}

function itemMediaLabel(args) {
  const type = cleanText(args?.itemType).toLowerCase() === "video" ? "video" : "fotoğraf";
  return args?.isStory ? `${type} hikayesi` : type;
}

function capitalize(value) {
  return value ? `${value[0].toLocaleUpperCase("tr-TR")}${value.slice(1)}` : value;
}

export function mediaDownloadOutcome(result, args = {}) {
  const isBatch = args?.mode === "batch";
  const label = isBatch ? batchMediaLabel(args) : itemMediaLabel(args);
  const downloadedCount = nonNegativeInteger(result?.downloadedCount);
  const failedCount = nonNegativeInteger(result?.failedCount);
  const status = downloadedCount === 0 ? "error" : failedCount > 0 ? "warning" : "success";

  let text;
  if (status === "error") {
    text = failedCount > 0
      ? `${failedCount} ${label} indirilemedi.`
      : `${capitalize(label)} indirilemedi.`;
  } else if (status === "warning") {
    text = `${downloadedCount} ${label} indirildi, ${failedCount} ${label} indirilemedi.`;
  } else if (isBatch) {
    text = `${downloadedCount} ${label} indirildi.`;
  } else {
    text = `${capitalize(label)} indirildi.`;
  }

  return { status, text, label, downloadedCount, failedCount };
}

export function mediaDownloadTarget(result, args = {}) {
  if (nonNegativeInteger(result?.downloadedCount) === 0) return "";
  return cleanText(args?.mode === "batch" ? result?.outputDir : result?.filePath || result?.outputDir);
}
