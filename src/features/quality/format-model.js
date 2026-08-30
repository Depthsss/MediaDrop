export const SOCIAL_COMPATIBLE_FORMAT_ID =
  "best[ext=mp4][vcodec^=avc1][acodec!=none]/best[ext=mp4][vcodec^=h264][acodec!=none]/best[vcodec^=avc1][acodec!=none]/best[vcodec^=h264][acodec!=none]/bestvideo[ext=mp4][vcodec^=avc1]+bestaudio[ext=m4a]/bestvideo[ext=mp4][vcodec^=h264]+bestaudio[ext=m4a]/bestvideo[vcodec^=avc1]+bestaudio/bestvideo[vcodec^=h264]+bestaudio/best[ext=mp4]/best";

export function formatBytes(bytes) {
  if (!bytes) return "Boyut bilinmiyor";

  const mb = bytes / 1024 / 1024;
  if (mb < 1024) return `~${mb.toFixed(1)} MB`;

  const gb = mb / 1024;
  return `~${gb.toFixed(2)} GB`;
}

export function isH264Codec(codec) {
  const value = String(codec || "").toLowerCase();
  return value.startsWith("avc1") || value.startsWith("h264");
}

export function isHevcCodec(codec) {
  const value = String(codec || "").toLowerCase();
  return (
    value.startsWith("hev1") ||
    value.startsWith("hvc1") ||
    value.startsWith("hevc") ||
    value.startsWith("h265") ||
    value.startsWith("bytevc1")
  );
}

export function getCodecLabel(format) {
  const codec = String(format?.vcodec || "").toLowerCase();
  if (isH264Codec(codec)) return "AVC";
  if (codec.startsWith("av01")) return "AV1";
  if (codec.startsWith("vp9")) return "VP9";
  if (isHevcCodec(codec)) return "HEVC";
  return "Video";
}

export function codecPriority(format) {
  const codec = String(format?.vcodec || "").toLowerCase();
  if (isH264Codec(codec)) return 0;
  if (codec.startsWith("av01")) return 1;
  if (codec.startsWith("vp9")) return 2;
  if (isHevcCodec(codec)) return 4;
  return 3;
}

export function qualityLabelFromHeight(height) {
  const value = Number(height || 0);
  if (value >= 2160) return "4K";
  if (value >= 1440) return "2K";
  return value > 0 ? `${value}p` : "Best";
}

export function qualityHeightFromLabel(label = "") {
  const text = String(label || "").toLowerCase();
  if (text.includes("4k")) return 2160;
  if (text.includes("2k")) return 1440;
  const match = text.match(/(\d{3,4})p/);
  return match ? Number(match[1]) : 0;
}

function formatIsPreferred(candidate, current) {
  const codecDiff = codecPriority(candidate) - codecPriority(current);
  if (codecDiff !== 0) return codecDiff < 0;
  const size = (format) => format.filesize || format.filesize_approx || 0;
  return size(candidate) > size(current);
}

export function buildAutoBestCard(info, platform) {
  const rawFormats = Array.isArray(info?.formats) ? info.formats : [];
  const bestVideo = rawFormats
    .filter(
      (item) =>
        item &&
        item.vcodec &&
        item.vcodec !== "none" &&
        (item.ext === "mp4" || item.protocol),
    )
    .sort((a, b) => {
      const codecDiff = codecPriority(a) - codecPriority(b);
      if (codecDiff !== 0) return codecDiff;
      const heightDiff = (b.height || 0) - (a.height || 0);
      if (heightDiff !== 0) return heightDiff;
      return (b.filesize || b.filesize_approx || 0) - (a.filesize || a.filesize_approx || 0);
    })[0];

  const quality =
    bestVideo?.width && bestVideo?.height
      ? `${bestVideo.width}×${bestVideo.height}`
      : bestVideo?.height
        ? `${bestVideo.height}p`
        : "Best";
  const title =
    platform === "instagram" ? "Instagram" : platform === "tiktok" ? "TikTok" : "X/Twitter";

  return [
    {
      id: SOCIAL_COMPATIBLE_FORMAT_ID,
      type: platform,
      title,
      quality,
      detail: "Windows uyumlu MP4 · otomatik",
      size: bestVideo
        ? formatBytes(bestVideo.filesize || bestVideo.filesize_approx)
        : "Otomatik",
      raw: bestVideo || {},
      autoSelect: true,
    },
  ];
}

export function buildYoutubeFormatCards(info) {
  const rawFormats = Array.isArray(info?.formats) ? info.formats : [];
  const bestByHeight = new Map();

  for (const item of rawFormats) {
    if (
      !item ||
      !String(item.format_id ?? "").trim() ||
      item.vcodec === "none" ||
      !item.vcodec ||
      Number(item.height) <= 0
    ) {
      continue;
    }
    const height = Number(item.height);
    const existing = bestByHeight.get(height);
    if (!existing || formatIsPreferred(item, existing)) {
      bestByHeight.set(height, item);
    }
  }

  const cards = [...bestByHeight.entries()]
    .sort((a, b) => b[0] - a[0])
    .map(([height, item], index) => {
      const codecLabel = getCodecLabel(item);
      return {
        id: item.format_id,
        type: "video",
        title: String(item.ext || "mp4").toUpperCase(),
        quality: qualityLabelFromHeight(height),
        height,
        detail:
          item.acodec && item.acodec !== "none"
            ? `${codecLabel} · video + ses`
            : `${codecLabel} · ses sonradan birleşecek`,
        size: formatBytes(item.filesize || item.filesize_approx),
        raw: item,
        autoSelect: index === 0,
      };
    });

  const audioFormat = rawFormats
    .filter(
      (item) =>
        item.acodec &&
        item.acodec !== "none" &&
        item.vcodec === "none" &&
        ["m4a", "webm"].includes(item.ext),
    )
    .sort((a, b) => (b.abr || 0) - (a.abr || 0))[0];

  if (audioFormat) {
    cards.push({
      id: audioFormat.format_id,
      type: "audio",
      title: "MP3",
      quality: `${Math.round(audioFormat.abr || 0)} kbps`,
      height: 0,
      detail: "MP3 olarak indirilecek",
      size: formatBytes(audioFormat.filesize || audioFormat.filesize_approx),
      raw: audioFormat,
    });
  }

  return cards;
}

export function buildFormatCards(info, platform) {
  return ["twitter", "instagram", "tiktok"].includes(platform)
    ? buildAutoBestCard(info, platform)
    : buildYoutubeFormatCards(info);
}
