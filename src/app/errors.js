export const STRUCTURED_ERROR_PREFIX = "__MEDIADROP_ERROR__";

function errorMetadata(source = {}) {
  return {
    fallbackOffer: source.fallback_offer || source.fallbackOffer || null,
    debugCode: source.debugCode || source.stage || source.code || source.errorCode || "",
    code: source.code || source.errorCode || "",
    retryable: source.retryable === true,
    action: String(source.action || source.recommendedAction || "").trim(),
    reportId: String(source.reportId || source.report_id || "").trim() || null,
  };
}

function structuredJsonPrefix(text) {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (inString) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') inString = false;
      continue;
    }
    if (character === '"') inString = true;
    else if (character === "{") depth += 1;
    else if (character === "}" && --depth === 0) {
      return { json: text.slice(0, index + 1), suffix: text.slice(index + 1) };
    }
  }
  return { json: text, suffix: "" };
}

function legacyReportReference(text) {
  const match = String(text || "").match(/(?:Hata raporu (?:oluşturuldu|oluÅŸturuldu):)\s*(.+)$/imu);
  return match ? { reportId: match[1].trim(), markerIndex: match.index } : null;
}

export function parseBackendError(error) {
  if (error && typeof error === "object") {
    return { raw: error, message: String(error.message || error), ...errorMetadata(error) };
  }

  const rawText = String(error || "Bilinmeyen hata.");
  const codeMatch = rawText.match(/^([a-z_]+):/);
  if (rawText.startsWith(STRUCTURED_ERROR_PREFIX)) {
    try {
      const parts = structuredJsonPrefix(rawText.slice(STRUCTURED_ERROR_PREFIX.length).trim());
      const payload = JSON.parse(parts.json.trim());
      const metadata = errorMetadata(payload);
      const legacyReport = legacyReportReference(parts.suffix);
      return {
        raw: payload,
        message: String(payload.message || "Bilinmeyen hata."),
        ...metadata,
        reportId: metadata.reportId || legacyReport?.reportId || null,
      };
    } catch {}
  }

  const legacyReport = legacyReportReference(rawText);
  return {
    raw: error,
    message: legacyReport ? rawText.slice(0, legacyReport.markerIndex).trim() : rawText,
    fallbackOffer: null,
    debugCode: codeMatch?.[1] || "",
    code: codeMatch?.[1] || "",
    retryable: false,
    action: "",
    reportId: legacyReport?.reportId || null,
  };
}
