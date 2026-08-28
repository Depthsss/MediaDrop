export function choiceIdForCard(card, platform, audioFallback = false) {
  if (audioFallback) return "audio:best";
  if (["instagram", "twitter", "tiktok"].includes(platform)) return "social:auto";
  if (!card) return "audio:best";
  const id = String(card.id || "").trim();
  if (!id) return card.type === "audio" ? "audio:best" : null;
  return card.type === "audio" ? `audio:${id}` : `video:${id}`;
}

export function mediaPrimaryAction(media, platform, selectedCard) {
  const type = String(media?.type || "video").toLowerCase();
  if (type === "text") return null;
  const choiceId = choiceIdForCard(selectedCard, platform);
  if (!choiceId) return null;
  return {
    label: type === "photo" ? "Fotoğrafı indir" : "Videoyu indir",
    choiceId,
  };
}

export function qualityLabelForCard(card) {
  return String(card?.quality || "").trim();
}

export function parseClipTime(value) {
  const clean = String(value ?? "").trim();
  if (!clean) return null;
  const parts = clean.split(":");
  if (parts.length === 1) {
    if (!/^\d+(?:\.\d+)?$/.test(parts[0])) return null;
    const seconds = Number(parts[0]);
    return Number.isFinite(seconds) ? seconds : null;
  }
  if (parts.length < 2 || parts.length > 3 || !parts.every((part) => /^\d+(?:\.\d+)?$/.test(part))) {
    return null;
  }
  const numbers = parts.map(Number);
  const seconds = numbers.at(-1);
  const minutes = numbers.at(-2);
  if (seconds >= 60 || minutes >= 60) return null;
  return (numbers.length === 3 ? numbers[0] * 3600 : 0) + minutes * 60 + seconds;
}

export function clipTimeForCapture(seconds, target) {
  const value = Number(seconds);
  if (!Number.isFinite(value) || value < 0 || !["start", "end"].includes(target)) return null;
  return Math.floor(value);
}

export function clipInputLabel(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const tail = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${tail}`
    : `${String(minutes).padStart(2, "0")}:${tail}`;
}

export function clipRangeForInput(startValue, endValue, durationSeconds) {
  const startSeconds = parseClipTime(startValue);
  const endSeconds = parseClipTime(endValue);
  const duration = Number(durationSeconds);
  if (startSeconds === null || endSeconds === null || startSeconds < 0 || endSeconds - startSeconds < 1) {
    return null;
  }
  if (Number.isFinite(duration) && duration > 0 && endSeconds > duration) return null;
  return { startSeconds, endSeconds };
}
