export function scanMediaInPage() {
  const candidates = [];
  const seen = new Set();

  function add(element, url, detectedBy) {
    const candidateUrl = String(url || "").trim();
    if (!candidateUrl || seen.has(`${element.tagName}:${candidateUrl}`)) return;
    seen.add(`${element.tagName}:${candidateUrl}`);
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    const visible =
      rect.width > 0 &&
      rect.height > 0 &&
      style.display !== "none" &&
      style.visibility !== "hidden" &&
      Number(style.opacity || 1) > 0;
    candidates.push({
      candidateUrl,
      detectedBy,
      mediaType: element.tagName.toLowerCase() === "audio" ? "audio" : "video",
      durationSeconds: Number.isFinite(element.duration) ? element.duration : null,
      width: Math.round(rect.width || element.videoWidth || 0),
      height: Math.round(rect.height || element.videoHeight || 0),
      playing: !element.paused && !element.ended,
      visible,
      muted: Boolean(element.muted),
      loop: Boolean(element.loop),
      live: element.duration === Infinity,
    });
  }

  for (const element of document.querySelectorAll("video, audio")) {
    add(element, element.currentSrc, "dom_current_src");
    add(element, element.getAttribute("src"), "dom_src");
    for (const source of element.querySelectorAll("source[src]")) {
      add(element, source.src || source.getAttribute("src"), "dom_source");
    }
  }

  return { frameUrl: location.href, candidates };
}
