const MAX_URL_BYTES = 8192;
const MAX_CANDIDATES = 8;

function webUrl(value) {
  const text = String(value || "").trim();
  if (!text || new TextEncoder().encode(text).length > MAX_URL_BYTES) return null;
  try {
    const parsed = new URL(text);
    if (!["http:", "https:"].includes(parsed.protocol)) return null;
    if (parsed.username || parsed.password) return null;
    return parsed.href;
  } catch {
    return null;
  }
}

function hostMatches(host, expected) {
  return host === expected || host.endsWith(`.${expected}`);
}

function canonicalSourcePageUrl(value) {
  const clean = webUrl(value);
  if (!clean) return null;

  const url = new URL(clean);
  const host = url.hostname.toLowerCase();
  const parts = url.pathname.split("/").filter(Boolean);

  if (host === "youtu.be" && parts[0]) {
    url.hostname = "www.youtube.com";
    url.pathname = "/watch";
    url.search = "";
    url.searchParams.set("v", parts[0]);
    url.hash = "";
    return url.href;
  }
  if (hostMatches(host, "youtube.com")) {
    const videoId = url.pathname === "/watch" ? url.searchParams.get("v") : null;
    const directPath = ["shorts", "embed", "live"].includes(parts[0]) && parts[1];
    if (videoId || directPath) {
      url.hostname = "www.youtube.com";
      url.pathname = "/watch";
      url.search = "";
      url.searchParams.set("v", videoId || directPath);
      url.hash = "";
      return url.href;
    }
  }
  if (hostMatches(host, "x.com") || hostMatches(host, "twitter.com")) {
    const statusIndex = parts.indexOf("status");
    const statusId = statusIndex >= 0 ? parts[statusIndex + 1] : null;
    if (statusId) {
      url.hostname = "x.com";
      url.pathname = `/${parts[statusIndex - 1] || "i"}/status/${statusId}`;
      url.search = "";
      url.hash = "";
      return url.href;
    }
  }
  if (!hostMatches(host, "instagram.com")) {
    if (hostMatches(host, "tiktok.com")) {
      const directPost = parts[0]?.startsWith("@")
        && ["video", "photo"].includes(parts[1])
        && parts[2];
      if (directPost) {
        url.hostname = "www.tiktok.com";
        url.pathname = `/${parts[0]}/${parts[1]}/${parts[2]}`;
        url.search = "";
        url.hash = "";
      } else if (["vm.tiktok.com", "vt.tiktok.com"].includes(host) && parts[0]) {
        url.search = "";
        url.hash = "";
      }
    }
    return url.href;
  }

  const postKinds = new Set(["p", "reel", "reels", "tv"]);
  const kindIndex = postKinds.has(parts[0]) ? 0 : postKinds.has(parts[1]) ? 1 : -1;
  const shortcode = kindIndex >= 0 ? parts[kindIndex + 1] : "";
  if (/^[A-Za-z0-9_-]{6,}$/.test(shortcode)) {
    const kind = parts[kindIndex] === "reels" ? "reel" : parts[kindIndex];
    url.hostname = "www.instagram.com";
    url.pathname = `/${kind}/${shortcode}/`;
    url.search = "";
    url.hash = "";
  } else if ((parts[0] === "stories" && parts.length >= 3) || (parts[0] === "share" && parts.length >= 2)) {
    url.hostname = "www.instagram.com";
    url.search = "";
    url.hash = "";
  }
  return url.href;
}

export function classifySourcePage(value) {
  const clean = canonicalSourcePageUrl(value);
  if (!clean) return "unsupported";
  const url = new URL(clean);
  const host = url.hostname.toLowerCase();
  const parts = url.pathname.split("/").filter(Boolean);

  if (host === "youtu.be") return parts[0] ? "content" : "browse";
  if (hostMatches(host, "youtube.com")) {
    const directPath = ["shorts", "embed", "live"].includes(parts[0]) && Boolean(parts[1]);
    return (url.pathname === "/watch" && Boolean(url.searchParams.get("v"))) || directPath
      ? "content"
      : "browse";
  }
  if (hostMatches(host, "instagram.com")) {
    const directPost = ["p", "reel", "tv"].includes(parts[0]) && Boolean(parts[1]);
    const directStory = parts[0] === "stories" && parts.length >= 3;
    const sharedPost = parts[0] === "share" && parts.length >= 2;
    return directPost || directStory || sharedPost ? "content" : "browse";
  }
  if (hostMatches(host, "x.com") || hostMatches(host, "twitter.com")) {
    const statusIndex = parts.indexOf("status");
    return statusIndex >= 0 && Boolean(parts[statusIndex + 1]) ? "content" : "browse";
  }
  if (hostMatches(host, "tiktok.com")) {
    const directPost = parts[0]?.startsWith("@")
      && ["video", "photo"].includes(parts[1])
      && Boolean(parts[2]);
    const shortLink = ["vm.tiktok.com", "vt.tiktok.com"].includes(host) && Boolean(parts[0]);
    const sharedPost = ["t", "share"].includes(parts[0]) && Boolean(parts[1]);
    return directPost || shortLink || sharedPost ? "content" : "browse";
  }
  return "unsupported";
}

export function actionPresentationForPage(value) {
  const enabled = classifySourcePage(value) !== "unsupported";
  return {
    enabled,
    title: enabled ? "MediaDrop" : "MediaDrop bu sayfayı desteklemiyor.",
  };
}

function normalizedCandidate(candidate = {}) {
  const rawUrl = String(candidate.candidateUrl || candidate.srcUrl || "").trim();
  const isBlob = rawUrl.startsWith("blob:");
  const candidateUrl = webUrl(rawUrl);
  if (rawUrl && !candidateUrl && !isBlob) return null;
  return {
    ...(candidateUrl ? { candidateUrl } : {}),
    detectedBy: isBlob ? "blob_hint" : String(candidate.detectedBy || "dom_src").slice(0, 64),
    mediaType: ["video", "audio"].includes(candidate.mediaType) ? candidate.mediaType : "video",
    durationSeconds:
      Number.isFinite(Number(candidate.durationSeconds)) && Number(candidate.durationSeconds) >= 0
        ? Number(candidate.durationSeconds)
        : null,
    width: Number.isFinite(Number(candidate.width)) ? Math.max(0, Math.round(candidate.width)) : null,
    height: Number.isFinite(Number(candidate.height)) ? Math.max(0, Math.round(candidate.height)) : null,
    playing: Boolean(candidate.playing),
    visible: Boolean(candidate.visible),
    muted: Boolean(candidate.muted),
    loop: Boolean(candidate.loop),
    live: Boolean(candidate.live) || Number(candidate.durationSeconds) === Infinity,
  };
}

function score(candidate) {
  let value = 0;
  if (candidate.detectedBy === "context_menu_src") value += 100;
  if (candidate.playing) value += 40;
  if (candidate.detectedBy === "dom_current_src") value += 25;
  if (["dom_src", "dom_source"].includes(candidate.detectedBy)) value += 15;
  if (candidate.visible && (candidate.width || 0) >= 160 && (candidate.height || 0) >= 90) value += 20;
  if ((candidate.durationSeconds || 0) >= 5 || candidate.live) value += 10;
  if (!candidate.visible || candidate.width === 0 || candidate.height === 0) value -= 80;
  if (candidate.muted && candidate.loop && (candidate.durationSeconds || 0) <= 30) value -= 30;
  return value;
}

export function rankCandidates(candidates = []) {
  const seen = new Set();
  return candidates
    .map((candidate, index) => ({ candidate: normalizedCandidate(candidate), index }))
    .filter(({ candidate }) => {
      if (!candidate) return false;
      const key = candidate.candidateUrl || `hint:${candidate.detectedBy}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((left, right) => score(right.candidate) - score(left.candidate) || left.index - right.index)
    .slice(0, MAX_CANDIDATES)
    .map(({ candidate }) => candidate);
}

export function buildSourcePayload({ pageUrl, frameUrl = null, mediaType = "video", candidates = [] }) {
  return {
    pageUrl: canonicalSourcePageUrl(pageUrl) || "",
    frameUrl: webUrl(frameUrl),
    mediaType: mediaType === "audio" ? "audio" : "video",
    candidates: rankCandidates(candidates),
  };
}
