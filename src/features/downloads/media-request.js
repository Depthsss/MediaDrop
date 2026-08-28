const MEDIA_BATCH_SCOPES = new Set(["all", "all-stories", "photos"]);

function text(value) {
  return String(value ?? "").trim();
}

function requiredText(value, field) {
  const clean = text(value);
  if (!clean) throw new TypeError(`${field} is required.`);
  return clean;
}

function outputDirectory(value) {
  return text(value) || null;
}

export function buildMediaItemDownloadRequest({ analysisId, itemId, outputDir } = {}) {
  return {
    analysisId: requiredText(analysisId, "analysisId"),
    itemId: requiredText(itemId, "itemId"),
    outputDir: outputDirectory(outputDir),
  };
}

export function buildMediaBatchDownloadRequest({ analysisId, scope, outputDir } = {}) {
  const cleanScope = requiredText(scope, "scope");
  if (!MEDIA_BATCH_SCOPES.has(cleanScope)) {
    throw new TypeError("scope is invalid.");
  }

  return {
    analysisId: requiredText(analysisId, "analysisId"),
    scope: cleanScope,
    outputDir: outputDirectory(outputDir),
  };
}

export function buildOptionalMediaRegistryTarget({ analysisId, itemId } = {}) {
  const cleanAnalysisId = text(analysisId);
  const cleanItemId = text(itemId);
  if (!cleanAnalysisId && !cleanItemId) return { analysisId: null, itemId: null };
  if (!cleanAnalysisId || !cleanItemId) {
    throw new TypeError("analysisId and itemId must be provided together.");
  }
  return { analysisId: cleanAnalysisId, itemId: cleanItemId };
}
