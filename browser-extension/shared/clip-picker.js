export function captureClipTime(config = {}) {
  const markerId = "mediadrop-clip-draft";
  const sourceKey = String(config.sourceKey || "").slice(0, 8192);
  const analysisRequestId = String(config.analysisRequestId || "").slice(0, 64);
  const mediaId = String(config.mediaId || "").slice(0, 256);

  function currentSourceKey() {
    try {
      const url = new URL(window.location.href);
      const host = url.hostname.toLowerCase();
      const parts = url.pathname.split("/").filter(Boolean);
      if (host !== "youtu.be" && host !== "youtube.com" && !host.endsWith(".youtube.com")) return "";
      const videoId = host === "youtu.be"
        ? parts[0]
        : url.pathname === "/watch"
          ? url.searchParams.get("v")
          : ["shorts", "embed", "live"].includes(parts[0])
            ? parts[1]
            : "";
      if (!videoId) return "";
      const stable = new URL("https://www.youtube.com/watch");
      stable.searchParams.set("v", videoId);
      return stable.href;
    } catch {
      return "";
    }
  }

  if (!sourceKey || sourceKey !== currentSourceKey() || !analysisRequestId || !mediaId) {
    return { ok: false, error: "source_changed" };
  }

  const start = Number(config.startSeconds);
  const end = Number(config.endSeconds);
  const state = {
    sourceKey,
    analysisRequestId,
    mediaId,
    startSeconds: Number.isFinite(start) && start >= 0 && start <= 604_800 ? Math.floor(start) : 0,
    endSeconds: Number.isFinite(end) && end >= 0 && end <= 604_800 ? Math.ceil(end) : 15,
    target: config.target === "end" ? "end" : "start",
  };
  function storeState() {
    let host = document.getElementById(markerId);
    if (!host) {
      host = document.createElement("div");
      host.id = markerId;
      host.hidden = true;
      const cleanup = () => host.remove();
      document.addEventListener("yt-navigate-start", cleanup, { once: true });
      window.addEventListener("pagehide", cleanup, { once: true });
      window.addEventListener("popstate", cleanup, { once: true });
      document.documentElement.append(host);
    }
    host.dataset.mediadropClipState = JSON.stringify(state);
  }
  if (config.capture === false) {
    storeState();
    return { ok: true, ...state };
  }

  const videos = [...document.querySelectorAll("video")]
    .map((video) => {
      const rect = video.getBoundingClientRect();
      const visible = rect.width > 0 && rect.height > 0;
      const playing = !video.paused && !video.ended;
      return { video, score: (playing ? 1_000_000 : 0) + (visible ? rect.width * rect.height : 0) };
    })
    .sort((left, right) => right.score - left.score);
  const currentTime = Number(videos[0]?.video?.currentTime);
  if (!Number.isFinite(currentTime) || currentTime < 0) {
    return { ok: false, error: "video_time_unavailable" };
  }

  const capturedTarget = state.target;
  const capturedSeconds = Math.floor(currentTime);
  if (capturedSeconds > 604_800) return { ok: false, error: "video_time_unavailable" };
  if (capturedTarget === "end" && capturedSeconds <= state.startSeconds) {
    return { ok: false, error: "clip_range_invalid" };
  }
  state[`${capturedTarget}Seconds`] = capturedSeconds;
  if (capturedTarget === "start") state.target = "end";

  storeState();
  return { ok: true, ...state, capturedTarget, capturedSeconds };
}

export function readClipDraft(expected = {}) {
  const host = document.getElementById("mediadrop-clip-draft");
  const encoded = host?.dataset?.mediadropClipState;
  if (!host || !encoded || encoded.length > 10_000) return null;
  try {
    const state = JSON.parse(encoded);
    const startSeconds = Number(state.startSeconds);
    const endSeconds = Number(state.endSeconds);
    const matches = state.sourceKey === String(expected.sourceKey || "")
      && state.analysisRequestId === String(expected.analysisRequestId || "")
      && state.mediaId === String(expected.mediaId || "");
    if (!matches || !Number.isFinite(startSeconds) || !Number.isFinite(endSeconds)
      || startSeconds < 0 || endSeconds < 0 || startSeconds > 604_800 || endSeconds > 604_800) {
      if (state.sourceKey !== String(expected.sourceKey || "")) host.remove();
      return null;
    }
    return {
      sourceKey: state.sourceKey,
      analysisRequestId: state.analysisRequestId,
      mediaId: state.mediaId,
      startSeconds,
      endSeconds,
      target: state.target === "end" ? "end" : "start",
    };
  } catch {
    host.remove();
    return null;
  }
}
