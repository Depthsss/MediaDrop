import { buildFormatCards } from "../shared/format-model.js";
import {
  advancedIntentForAction,
  completedStateForAction,
  pendingStateForAction,
  readyStateForReturn,
  resultActionForState,
  shouldPollState,
} from "../shared/browser-flow.js";
import {
  choiceIdForCard,
  clipInputLabel,
  clipRangeForInput,
  mediaPrimaryAction,
  parseClipTime,
  qualityLabelForCard,
} from "../shared/choice-model.js";

const app = document.querySelector("#app");
const connectionStatus = document.querySelector("#connectionStatus");
const connectionText = document.querySelector("#connectionText");
let currentState = null;
let lastReadyState = null;
let selectedMediaIndex = 0;
let pollTimer = null;
let busy = false;
const previewCache = new Map();
const previewPending = new Set();
const clipDrafts = new Map();
const disconnectedState = {
  status: "error",
  payload: {},
  capabilities: {},
  error: {
    code: "native_host_disconnected",
    message: "MediaDrop eklenti bağlantısı kurulamadı.",
  },
};

function send(message) {
  return new Promise((resolve) => {
    try {
      chrome.runtime.sendMessage(message, (response) => {
        if (!chrome.runtime.lastError && response) {
          resolve(response);
          return;
        }
        resolve(disconnectedState);
      });
    } catch {
      resolve(disconnectedState);
    }
  });
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function button(label, action, className = "") {
  const node = element("button", className, label);
  node.type = "button";
  node.dataset.action = action;
  node.disabled = busy;
  return node;
}

function statusCopy(status, error) {
  if (["native_host_not_found", "native_host_disconnected"].includes(error?.code)) {
    return ["MediaDrop bulunamadı", "Masaüstü uygulamasını kur veya yeniden başlat."];
  }
  if (error?.code === "pipe_disconnected") {
    return ["Bağlantı koptu", "MediaDrop bağlantısını yeniden kurmak için tekrar dene."];
  }
  if (error?.code === "analysis_busy") {
    return ["Başka bir medya analizi devam ediyor", "Devam eden analizin sonucunu bekle."];
  }
  if (error?.code === "content_page_required") {
    return [
      "İçeriğin sayfasını aç",
      "YouTube, Instagram, X veya TikTok’ta indirmek istediğin gönderiyi ya da videoyu açıp tekrar dene.",
    ];
  }
  if (error?.code === "version_mismatch" && error?.action === "reload_extension") {
    return [
      "Eklenti güncellemesi hazır",
      "Yeni MediaDrop dosyalarını etkinleştirmek için eklentiyi bir kez yenile.",
    ];
  }
  const copy = {
    accepted: ["Bağlanıyor", "MediaDrop yanıtı bekleniyor…"],
    app_starting: ["MediaDrop başlatılıyor", "Masaüstü uygulaması açılıyor…"],
    connecting: ["Bağlanıyor", "Güvenli yerel bağlantı kuruluyor…"],
    analyzing: ["Medya analiz ediliyor", "Kalite seçenekleri hazırlanıyor…"],
    downloading: ["İndiriliyor", "İndirme masaüstünde devam ediyor."],
    paused: ["İndirme duraklatıldı", "Hazır olduğunda devam ettirebilirsin."],
    postprocessing: ["Birleştiriliyor / dönüştürülüyor", "FFmpeg çıktıyı hazırlıyor…"],
    validating: ["Doğrulanıyor", "İndirilen dosya kontrol ediliyor…"],
    completed: ["Tamamlandı", "Dosyan Downloads\\MediaDrop klasöründe hazır."],
    app_opened: ["MediaDrop'ta açıldı", "Gelişmiş seçenekler masaüstü uygulamasında hazır."],
    cancelled: ["İndirme iptal edildi", "Yeni bir indirme seçebilirsin."],
    needs_user: ["Masaüstü uygulamasında işlem gerekiyor", error?.message || "İzin veya oturum adımını MediaDrop'ta tamamla."],
    unsupported: ["Medya bulunamadı", error?.message || "Bu kaynak henüz desteklenmiyor."],
    version_mismatch: ["Bridge/protokol sürümü uyumsuz", "MediaDrop ve eklentiyi güncelle."],
    busy: ["Başka bir indirme devam ediyor", "MediaDrop aynı anda tek indirme çalıştırır."],
    invalid_request: ["Geçersiz istek", error?.message || "Eklenti isteği MediaDrop tarafından reddedildi."],
    error: ["Hata", error?.message || "MediaDrop bağlantısı tamamlanamadı."],
  };
  return copy[status] || ["MediaDrop bulunamadı", "Masaüstü uygulamasını kur veya yeniden başlat."];
}

function renderStatus(state) {
  const [title, defaultMessage] = statusCopy(state.status, state.error);
  const result = state.payload?.activeJob?.result;
  const message = state.status === "completed" && result?.displayName
    ? `${result.displayName} hazır.`
    : defaultMessage;
  const polling = shouldPollState(state);
  const section = element("section", `state centered status-view status-${state.status}`);
  if (polling) section.append(element("div", "spinner"));
  else {
    const symbol = element("div", "status-symbol");
    symbol.setAttribute("aria-hidden", "true");
    section.append(symbol);
  }
  section.append(element("h2", "", title), element("p", "muted", message));
  if (state.error?.code === "content_page_required" || state.status === "unsupported") {
    section.append(button("Aktif sekmeyi yeniden tara", "retry_active_tab", "retry-button"));
  }
  if (state.status === "version_mismatch" && state.error?.action === "reload_extension") {
    section.append(button("Eklentiyi yenile", "reload_extension", "download-primary"));
  } else if (
    ["error", "needs_user", "version_mismatch"].includes(state.status)
    && state.error?.code !== "content_page_required"
  ) {
    const continueInApp = state.status === "needs_user" && state.payload?.analysisRequestId;
    section.append(button(
      continueInApp ? "Masaüstünde devam et" : "MediaDrop'u aç",
      continueInApp ? "advanced" : "open_app",
      "secondary",
    ));
  }
  const resultAction = resultActionForState(state);
  if (resultAction) section.append(button(resultAction.label, resultAction.action, "download-primary status-action"));
  if (["completed", "app_opened"].includes(state.status) && lastReadyState) {
    section.append(button("İndirme seçeneklerine dön", "back_to_ready", "secondary status-back"));
  }
  if (state.error?.reportId && state.capabilities?.revealErrorReport === true) {
    section.append(button("Hata raporunu göster", "reveal_error_report", "secondary"));
  }
  if (state.error?.fallbackOffer?.kind === "hls_1080") {
    section.append(button(state.error.fallbackOffer.label || "1080p hızlı klip dene", "retry_clip_1080"));
  }
  return section;
}

function durationLabel(seconds) {
  if (!Number.isFinite(Number(seconds))) return "";
  const total = Math.max(0, Math.round(seconds));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

function siteLabel(site) {
  return ({ youtube: "YouTube", instagram: "Instagram", twitter: "X/Twitter", tiktok: "TikTok" })[site]
    || String(site || "");
}

function previewKey(payload, media) {
  return `${payload?.analysisRequestId || ""}:${media?.mediaId || ""}`;
}

async function loadPreview(state, media) {
  const payload = state?.payload || {};
  const key = previewKey(payload, media);
  if (!media?.previewAvailable || !key || previewCache.has(key) || previewPending.has(key)) return;
  previewPending.add(key);
  const response = await send({
    type: "native_command",
    command: "get_media_preview",
    payload: {
      analysisRequestId: payload.analysisRequestId,
      mediaId: media.mediaId,
    },
  });
  previewPending.delete(key);
  if (response.status === "ready" && response.payload?.dataUrl) {
    previewCache.set(key, {
      dataUrl: response.payload.dataUrl,
      durationSeconds: response.payload.durationSeconds,
    });
    if (currentState?.payload?.analysisRequestId === payload.analysisRequestId) render(currentState);
  }
}

function renderJob(job, state) {
  const card = element("section", "job-card");
  card.append(element("strong", "", statusCopy(state.status)[0]));
  const percent = Math.max(0, Math.min(100, Number(job?.percent || 0)));
  const progress = element("progress");
  progress.max = 100;
  progress.value = percent;
  progress.setAttribute("aria-label", "İndirme ilerlemesi");
  card.append(progress);
  const line = element("div", "job-line");
  line.append(
    element("span", "", `${Math.round(percent)}%`),
    element("span", "", job?.speedMb ? `${Number(job.speedMb).toFixed(2)} MB/s` : job?.stage || ""),
  );
  card.append(line);
  if (job?.controllable) {
    const actions = element("div", "actions");
    actions.append(
      button(state.status === "paused" ? "Devam et" : "Duraklat", state.status === "paused" ? "resume" : "pause", "secondary"),
      button("İptal", "cancel", "danger"),
    );
    card.append(actions);
  }
  return card;
}

function renderReady(state) {
  const payload = state.payload || {};
  const mediaList = Array.isArray(payload.media) ? payload.media : [];
  if (!mediaList.length) return renderStatus({ status: "unsupported", error: { message: "Medya bulunamadı." } });
  selectedMediaIndex = Math.min(selectedMediaIndex, mediaList.length - 1);
  const media = mediaList[selectedMediaIndex];
  const preview = previewCache.get(previewKey(payload, media));
  const section = element("section", "state ready-view");
  const heading = element("div", "section-heading");
  const headingCopy = element("div");
  headingCopy.append(
    element("h1", "", mediaList.length > 1 ? "Medyayı seç" : "İndirmeye hazır"),
    element("p", "", mediaList.length > 1 ? "İndirmek istediğin içeriği seç." : "Kaliteyi seç, indirmeyi başlat."),
  );
  heading.append(headingCopy, element("span", "site-badge", siteLabel(payload.site)));
  section.append(heading);

  if (mediaList.length > 1) {
    const label = element("label", "quality-field media-picker");
    label.append(element("span", "field-header", "Medya"));
    const shell = element("span", "select-shell");
    const select = element("select");
    select.id = "mediaSelect";
    select.setAttribute("aria-label", "İndirilecek medya");
    mediaList.forEach((item, index) => {
      const option = element("option", "", item.displayTitle || item.title || `Medya ${index + 1}`);
      option.value = String(index);
      option.selected = index === selectedMediaIndex;
      select.append(option);
    });
    shell.append(select);
    label.append(shell);
    section.append(label);
  }

  const card = element("div", "media-card");
  const summary = element("div", "media-summary media-block");
  const poster = element("div", "poster");
  if (preview?.dataUrl || media.thumbnailUrl) {
    const image = element("img", "thumbnail poster-image");
    image.src = preview?.dataUrl || media.thumbnailUrl;
    image.alt = "";
    image.referrerPolicy = "no-referrer";
    image.width = 132;
    image.height = 78;
    poster.append(image);
  } else {
    poster.append(element("div", "thumbnail placeholder", "Ön izleme"));
  }
  const duration = durationLabel(media.durationSeconds || preview?.durationSeconds)
    || (media.type === "video" ? "Süre bilinmiyor" : "");
  if (duration && duration !== "Süre bilinmiyor") poster.append(element("span", "poster-duration", duration));
  summary.append(poster);
  const details = element("div", "media-copy");
  details.append(element("h2", "title", media.displayTitle || media.title || "Başlıksız medya"));
  const mediaType = media.type === "photo" ? "Fotoğraf" : media.type === "text" ? "Gönderi" : "Video";
  const dimensions = media.width && media.height ? `${media.width}×${media.height}` : "";
  const meta = [mediaType, duration, dimensions].filter(Boolean).join(" • ");
  details.append(element("div", "meta media-meta", meta));
  summary.append(details);
  card.append(summary);

  const cards = buildFormatCards({ formats: media.formats || [] }, payload.site);
  const qualityLabel = element("label", "quality-field");
  const fieldHeader = element("span", "field-header");
  fieldHeader.append(element("span", "", "Video kalitesi"), element("span", "field-hint", "Önerilen"));
  qualityLabel.append(fieldHeader);
  const qualityShell = element("span", "select-shell");
  const qualitySelect = element("select");
  qualitySelect.id = "qualitySelect";
  qualitySelect.setAttribute("aria-label", "Video kalitesi");
  cards.filter((item) => item.type !== "audio").forEach((item) => {
    const option = element("option", "", qualityLabelForCard(item));
    option.value = item.id;
    qualitySelect.append(option);
  });
  qualityShell.append(qualitySelect);
  qualityLabel.append(qualityShell);
  if (qualitySelect.options.length) card.append(qualityLabel);

  const actions = element("div", "actions primary-actions");
  const selectedCard = cards.find((item) => item.id === qualitySelect.value);
  const primaryAction = mediaPrimaryAction(media, payload.site, selectedCard);
  const videoButton = primaryAction ? button(primaryAction.label, "download", "download-primary") : null;
  const audioButton = media.type === "photo"
    || media.type === "text"
    || (payload.site !== "youtube" && mediaList.length > 1)
    ? null
    : button("MP3 indir", "download_audio", "audio-secondary");
  const postButton = payload.site === "twitter"
    ? button("Gönderiyi kart olarak indir", "download_post", "secondary utility-action")
    : null;
  const batchButton = mediaList.length > 1 && ["instagram", "twitter", "tiktok"].includes(payload.site)
    ? button("Tümünü indir", "download_all", "secondary utility-action")
    : null;
  const advancedButton = button("", "advanced", "advanced-action");
  const advancedCopy = element("span", "action-copy");
  advancedCopy.append(
    element("strong", "", "Gelişmiş seçenekler"),
    element("small", "", "Klip editörü ve özel indirme ayarları"),
  );
  advancedButton.append(
    element("span", "action-icon"),
    advancedCopy,
    element("span", "chevron"),
  );
  if (videoButton) videoButton.disabled ||= state.capabilities?.startDownload !== true;
  if (audioButton) {
    audioButton.disabled ||= state.capabilities?.startDownload !== true
      || !media.audioAvailable;
  }
  advancedButton.disabled ||= state.capabilities?.openAdvanced !== true;
  if (postButton) {
    postButton.disabled ||= state.capabilities?.startPostExport !== true
      && state.capabilities?.openAdvanced !== true;
  }
  if (batchButton) batchButton.disabled ||= state.capabilities?.startMediaBatch !== true;
  if (videoButton) actions.append(videoButton);
  if (audioButton) actions.append(audioButton);
  card.append(actions);
  if (batchButton) card.append(batchButton);
  if (postButton) card.append(postButton);

  if (payload.site === "youtube" && media.type !== "photo") {
    const key = previewKey(payload, media);
    const durationSeconds = Number(media.durationSeconds || preview?.durationSeconds);
    const draft = clipDrafts.get(key) || {
      start: "00:00",
      end: clipInputLabel(Math.min(15, Number.isFinite(durationSeconds) && durationSeconds > 0 ? durationSeconds : 15)),
      target: "start",
      expanded: false,
    };
    clipDrafts.set(key, draft);
    const clipPanel = element("details", "clip-panel quick-clip");
    clipPanel.open = draft.expanded === true;
    const clipTitle = element("summary", "clip-title");
    const clipCopy = element("span", "clip-title-copy");
    clipCopy.append(
      element("strong", "", "Hızlı klip"),
      element("small", "", "Başlangıç ve bitiş anını seç"),
    );
    clipTitle.append(
      element("span", "clip-icon"),
      clipCopy,
      element("span", "chevron"),
    );
    clipPanel.append(clipTitle);
    clipPanel.addEventListener("toggle", () => {
      draft.expanded = clipPanel.open;
      clipDrafts.set(key, draft);
    });
    const fields = element("div", "clip-fields");
    for (const [id, labelText, value] of [
      ["clipStart", "Başlangıç", draft.start],
      ["clipEnd", "Bitiş", draft.end],
    ]) {
      const label = element("label", "", labelText);
      label.classList.toggle("selected", draft.target === (id === "clipStart" ? "start" : "end"));
      const input = element("input");
      input.id = id;
      input.type = "text";
      input.inputMode = "decimal";
      input.autocomplete = "off";
      input.placeholder = "MM:SS";
      input.value = value;
      label.append(input);
      fields.append(label);
    }
    clipPanel.append(fields);
    const hint = element("div", "clip-hint", "Alanı seç, videoyu konumlandır ve zamanı al.");
    hint.id = "clipHint";
    clipPanel.append(hint);
    const clipActions = element("div", "clip-actions");
    const captureButton = button("Bu anı al", "capture_clip_time", "secondary");
    captureButton.disabled ||= state.capabilities?.startClip !== true;
    const clipButton = button("Klibi indir", "download_clip");
    clipButton.disabled ||= state.capabilities?.startClip !== true
      || state.capabilities?.startDownload !== true
      || !qualitySelect.options.length;
    clipActions.append(captureButton, clipButton);
    clipPanel.append(clipActions);
    card.append(clipPanel);
  }
  card.append(advancedButton);
  section.append(card);
  if (payload.activeJob) section.append(renderJob(payload.activeJob, state));
  if (!preview && media.previewAvailable && state.capabilities?.previewMedia === true) {
    queueMicrotask(() => void loadPreview(state, media));
  }
  return section;
}

function render(state) {
  if (["ready", "completed", "app_opened"].includes(state.status)) {
    lastReadyState = readyStateForReturn(state) || lastReadyState;
  }
  currentState = state;
  app.replaceChildren();
  app.setAttribute("aria-busy", shouldPollState(state) ? "true" : "false");
  const unavailable = ["native_host_not_found", "native_host_disconnected", "pipe_disconnected"]
    .includes(state.error?.code);
  const connecting = ["accepted", "app_starting", "connecting"].includes(state.status);
  const working = ["analyzing", "downloading", "postprocessing", "validating"].includes(state.status);
  const waiting = ["error", "unsupported", "invalid_request", "version_mismatch", "cancelled"]
    .includes(state.status);
  const connectionKind = unavailable ? "offline" : connecting || waiting ? "idle" : working ? "working" : "connected";
  const connectionLabel = unavailable
    ? "Bağlantı yok"
    : connecting
      ? "Bağlanıyor"
      : waiting
        ? "Bekliyor"
        : working
          ? "Çalışıyor"
          : "Hazır";
  connectionStatus.className = `status-pill ${connectionKind}`;
  connectionText.textContent = connectionLabel;
  connectionStatus.setAttribute("aria-label", `MediaDrop ${connectionLabel.toLocaleLowerCase("tr-TR")}`);
  if (state.status === "ready") app.append(renderReady(state));
  else if (["busy", "downloading", "paused", "postprocessing", "validating"].includes(state.status) && state.payload?.activeJob) {
    const section = element("section", "state");
    section.append(renderJob(state.payload.activeJob, state));
    app.append(section);
  } else app.append(renderStatus(state));
  if (!shouldPollState(state)) {
    void send({
      type: "clear_badge",
      analysisRequestId: state.payload?.analysisRequestId,
    });
  }
}

function showActionError(message) {
  let node = document.querySelector("#actionError");
  if (!node) {
    node = element("div", "local-error");
    node.id = "actionError";
    node.setAttribute("role", "alert");
    app.append(node);
  }
  node.textContent = message;
}

async function restoreClipDraft(state) {
  const payload = state?.payload || {};
  if (state?.status !== "ready" || payload.site !== "youtube" || !Array.isArray(payload.media)) return;
  const media = payload.media[Math.min(selectedMediaIndex, payload.media.length - 1)];
  if (!media?.mediaId || !payload.analysisRequestId) return;
  const response = await send({
    type: "get_clip_draft",
    analysisRequestId: payload.analysisRequestId,
    mediaId: media.mediaId,
  });
  const draft = response.payload?.clipDraft;
  if (!draft) return;
  clipDrafts.set(previewKey(payload, media), {
    start: clipInputLabel(draft.startSeconds),
    end: clipInputLabel(draft.endSeconds),
    target: draft.target === "end" ? "end" : "start",
    expanded: true,
  });
}

async function refresh(initial = false) {
  clearTimeout(pollTimer);
  let state = await send({
    type: "get_state",
    preferActiveTab: initial,
    analysisRequestId: initial ? undefined : currentState?.payload?.analysisRequestId,
  });
  if (initial && state.status === "accepted" && !state.payload?.analysisRequestId && !state.payload?.activeJob) {
    render({ status: "analyzing", payload: {} });
    state = await send({ type: "analyze_active_tab" });
  }
  if (initial) await restoreClipDraft(state);
  render(state);
  if (shouldPollState(state)) pollTimer = setTimeout(() => refresh(false), 1_000);
}

async function runAction(action) {
  if (!currentState || busy) return;
  if (action === "reload_extension") {
    chrome.runtime.reload();
    return;
  }
  if (action === "back_to_ready" && lastReadyState) {
    render(lastReadyState);
    return;
  }
  if (action === "retry_active_tab") {
    const capabilities = currentState.capabilities || {};
    busy = true;
    render({ status: "analyzing", payload: {}, capabilities });
    const response = await send({ type: "analyze_active_tab" });
    busy = false;
    render(response);
    if (shouldPollState(response)) pollTimer = setTimeout(() => refresh(false), 1_000);
    return;
  }
  const selectedQualityId = document.querySelector("#qualitySelect")?.value;
  const payload = currentState.payload || {};
  const media = payload.media?.[selectedMediaIndex];
  const preview = previewCache.get(previewKey(payload, media));
  if (action === "capture_clip_time") {
    if (!media?.mediaId || !payload.analysisRequestId) {
      showActionError("Zamanı alınacak hazır bir YouTube videosu bulunamadı.");
      return;
    }
    const key = previewKey(payload, media);
    const draft = clipDrafts.get(key) || {
      start: "00:00",
      end: "00:15",
      target: "start",
      expanded: true,
    };
    const startSeconds = parseClipTime(document.querySelector("#clipStart")?.value ?? draft.start);
    const endSeconds = parseClipTime(document.querySelector("#clipEnd")?.value ?? draft.end);
    const response = await send({
      type: "capture_clip_time",
      analysisRequestId: payload.analysisRequestId,
      mediaId: media.mediaId,
      draft: {
        startSeconds: startSeconds ?? 0,
        endSeconds: endSeconds ?? 15,
        target: draft.target,
      },
    });
    if (response.status !== "accepted" || !response.payload?.clipDraft) {
      showActionError(response.error?.message || "Videonun şu anki zamanı alınamadı.");
      return;
    }
    const captured = response.payload.clipDraft;
    const nextDraft = {
      start: clipInputLabel(captured.startSeconds),
      end: clipInputLabel(captured.endSeconds),
      target: captured.target,
      expanded: true,
    };
    clipDrafts.set(key, nextDraft);
    const startInput = document.querySelector("#clipStart");
    const endInput = document.querySelector("#clipEnd");
    if (startInput) startInput.value = nextDraft.start;
    if (endInput) endInput.value = nextDraft.end;
    document.querySelectorAll(".clip-fields label").forEach((label) => {
      const target = label.querySelector("input")?.id === "clipEnd" ? "end" : "start";
      label.classList.toggle("selected", target === nextDraft.target);
    });
    const hint = document.querySelector("#clipHint");
    if (hint) {
      hint.textContent = captured.capturedTarget === "start"
        ? `Başlangıç ${nextDraft.start} alındı. Şimdi bitişe git.`
        : `Bitiş ${nextDraft.end} alındı.`;
    }
    return;
  }
  let clip = null;
  let fallbackChoiceId = null;
  if (["download_clip", "retry_clip_1080"].includes(action)) {
    const draft = clipDrafts.get(previewKey(payload, media));
    const startValue = document.querySelector("#clipStart")?.value ?? draft?.start;
    const endValue = document.querySelector("#clipEnd")?.value ?? draft?.end;
    clip = clipRangeForInput(
      startValue,
      endValue,
      media?.durationSeconds || preview?.durationSeconds,
    );
    if (!clip) {
      showActionError("Geçerli bir klip aralığı gir. Bitiş başlangıçtan en az 1 saniye sonra olmalı.");
      return;
    }
    if (action === "retry_clip_1080") {
      const cards = buildFormatCards({ formats: media?.formats || [] }, payload.site);
      const fallback = cards.find((card) => card.type !== "audio" && /^1080p\b/i.test(card.quality));
      fallbackChoiceId = choiceIdForCard(fallback, payload.site);
      if (!fallbackChoiceId) {
        showActionError("Bu analizde 1080p klip kalitesi bulunamadı.");
        return;
      }
    }
  }
  const sourceState = currentState;
  busy = true;
  render(pendingStateForAction(action, sourceState));
  let command;
  let commandPayload = {};
  if (action === "open_app") command = "open_app";
  if (action === "open_downloads") command = "open_downloads";
  if (action === "reveal_result") {
    command = "reveal_result";
    commandPayload = {
      analysisRequestId: payload.analysisRequestId,
      jobId: payload.activeJob?.jobId,
    };
  }
  if (action === "reveal_error_report") {
    command = "reveal_error_report";
    commandPayload = { reportId: currentState.error?.reportId };
  }
  if (action === "download_all") {
    command = "start_media_batch";
    commandPayload = { analysisRequestId: payload.analysisRequestId, scope: "all" };
  }
  if (action === "advanced" || (action === "download_post" && currentState.capabilities?.startPostExport !== true)) {
    command = "open_advanced";
    commandPayload = { analysisRequestId: payload.analysisRequestId };
    const intent = advancedIntentForAction(action, payload.site);
    if (intent) commandPayload.intent = intent;
  }
  if (action === "download_post" && currentState.capabilities?.startPostExport === true) {
    command = "start_post_export";
    commandPayload = {
      analysisRequestId: payload.analysisRequestId,
      mediaId: media?.mediaId,
    };
  }
  if (["download", "download_audio", "download_clip", "retry_clip_1080"].includes(action)) {
    command = "start_download";
    const cards = buildFormatCards({ formats: media?.formats || [] }, payload.site);
    const selectedCard = cards.find((card) => card.id === selectedQualityId);
    const primaryAction = mediaPrimaryAction(media, payload.site, selectedCard);
    commandPayload = {
      analysisRequestId: payload.analysisRequestId,
      mediaId: media?.mediaId,
      choiceId: action === "download_audio"
        ? choiceIdForCard(cards.find((card) => card.type === "audio"), payload.site, true)
        : fallbackChoiceId || primaryAction?.choiceId,
    };
    if (clip) commandPayload.clip = clip;
  }
  if (["pause", "resume", "cancel"].includes(action)) {
    command = `${action}_download`;
    commandPayload = { analysisRequestId: payload.analysisRequestId };
    commandPayload[action === "resume" ? "previousJobId" : "jobId"] = payload.activeJob?.jobId;
  }
  const response = command
    ? await send({ type: "native_command", command, payload: commandPayload })
    : currentState;
  busy = false;
  const nextState = completedStateForAction(command, response, sourceState);
  render(nextState);
  if (shouldPollState(nextState)) pollTimer = setTimeout(() => refresh(false), 1_000);
}

app.addEventListener("change", (event) => {
  if (event.target.id === "mediaSelect") {
    selectedMediaIndex = Number(event.target.value) || 0;
    render(currentState);
  }
});

app.addEventListener("input", (event) => {
  if (!["clipStart", "clipEnd"].includes(event.target.id) || !currentState) return;
  const media = currentState.payload?.media?.[selectedMediaIndex];
  const key = previewKey(currentState.payload, media);
  const draft = clipDrafts.get(key) || {
    start: "00:00",
    end: "00:15",
    target: "start",
    expanded: true,
  };
  draft[event.target.id === "clipStart" ? "start" : "end"] = event.target.value;
  clipDrafts.set(key, draft);
});

app.addEventListener("focusin", (event) => {
  if (!["clipStart", "clipEnd"].includes(event.target.id) || !currentState) return;
  const media = currentState.payload?.media?.[selectedMediaIndex];
  const key = previewKey(currentState.payload, media);
  const draft = clipDrafts.get(key) || {
    start: "00:00",
    end: "00:15",
    target: "start",
    expanded: true,
  };
  draft.target = event.target.id === "clipEnd" ? "end" : "start";
  clipDrafts.set(key, draft);
  document.querySelectorAll(".clip-fields label").forEach((label) => label.classList.remove("selected"));
  event.target.closest("label")?.classList.add("selected");
  if (media?.mediaId && currentState.payload?.analysisRequestId) {
    void send({
      type: "set_clip_draft",
      analysisRequestId: currentState.payload.analysisRequestId,
      mediaId: media.mediaId,
      draft: {
        startSeconds: parseClipTime(document.querySelector("#clipStart")?.value ?? draft.start) ?? 0,
        endSeconds: parseClipTime(document.querySelector("#clipEnd")?.value ?? draft.end) ?? 15,
        target: draft.target,
      },
    });
  }
});

app.addEventListener("click", (event) => {
  const target = event.target.closest("button[data-action]");
  if (target) void runAction(target.dataset.action);
});

chrome.runtime.onMessage.addListener((message, sender) => {
  if (sender.id === chrome.runtime.id && message?.type === "native_status" && message.state) {
    render(message.state);
  }
});

render({ status: "connecting", payload: {}, capabilities: {} });
void refresh(true);
