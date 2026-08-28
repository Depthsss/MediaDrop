const NON_AUTH_CODES = new Set([
  "instagram_parser_error",
  "instagram_schema_error",
  "instagram_story_not_found",
  "instagram_story_access_denied",
  "instagram_content_not_found",
  "instagram_rate_limited",
  "instagram_highlight_unsupported",
]);

const AUTH_CODES = new Set([
  "instagram_auth_required",
  "instagram_auth_expired",
  "instagram_cookie_invalid",
  "instagram_browser_locked",
]);

const AUTH_ACTIONS = new Set([
  "request_instagram_auth",
  "refresh_instagram_auth",
  "request_cookie_permission",
  "restart_browser",
]);

const COOKIE_PREPARE_RESTART_CODES = new Set(["instagram_browser_locked", "browser_restart_required"]);
const COOKIE_PREPARE_FORCE_CLOSE_CODES = new Set(["browser_still_running"]);

function normalizedSignal(value) {
  return String(value ?? "").trim().toLowerCase().replace(/[\s-]+/g, "_");
}

export function instagramInitialAuthMode({
  isStory = false,
  hasSavedCookies = false,
  publicMode = "public",
  savedMode = "saved:instagram",
} = {}) {
  if (hasSavedCookies) return savedMode;
  return isStory ? null : publicMode;
}

export function isInstagramAuthRecoverySignal({ code = "", action = "", message = "" } = {}) {
  const normalizedCode = normalizedSignal(code);
  const normalizedAction = normalizedSignal(action);

  if (NON_AUTH_CODES.has(normalizedCode)) return false;
  if (AUTH_CODES.has(normalizedCode) || AUTH_ACTIONS.has(normalizedAction)) return true;

  // A structured, non-auth code must never fall through to fuzzy text matching.
  if (normalizedCode) return false;

  const text = String(message || "").toLowerCase();
  return (
    text.includes("failed to decrypt") ||
    text.includes("cookie anahtarı bulunamadı") ||
    text.includes("cookie anahtari bulunamadi") ||
    text.includes("cookie veritabanı kilitli") ||
    text.includes("cookie veritabani kilitli") ||
    text.includes("instagram oturumu bulunamadı") ||
    text.includes("instagram oturumu bulunamadi") ||
    text.includes("instagram oturumu doğrulanamadı") ||
    text.includes("instagram oturumu dogrulanamadi")
  );
}

export function nextInstagramAuthRecoveryStep({
  isAuthError = false,
  authMode = "",
  savedMode = "saved:instagram",
  hasRefreshBrowser = false,
  refreshAttempts = 0,
  promptAttempts = 0,
} = {}) {
  if (!isAuthError) return "stop";
  if (
    authMode === savedMode &&
    hasRefreshBrowser &&
    Number(refreshAttempts) < 1
  ) {
    return "refresh";
  }
  return Number(promptAttempts) < 1 ? "prompt" : "stop";
}

export function consumeInstagramAuthPromptBudget({
  promptAttempts = 0,
  maxPrompts = 1,
} = {}) {
  const attempts = Math.max(0, Number(promptAttempts) || 0);
  const limit = Math.max(0, Number(maxPrompts) || 0);
  if (attempts >= limit) {
    return { allowed: false, promptAttempts: attempts };
  }
  return { allowed: true, promptAttempts: attempts + 1 };
}

export function nextInstagramCookiePrepareStep({
  code = "",
  restartPromptAttempts = 0,
  forcePromptAttempts = 0,
} = {}) {
  const normalizedCode = normalizedSignal(code);
  const restartAttempts = Math.max(0, Number(restartPromptAttempts) || 0);
  const forceAttempts = Math.max(0, Number(forcePromptAttempts) || 0);

  if (COOKIE_PREPARE_FORCE_CLOSE_CODES.has(normalizedCode)) {
    return restartAttempts > 0 && forceAttempts < 1 ? "force-close" : "stop";
  }
  if (COOKIE_PREPARE_RESTART_CODES.has(normalizedCode)) {
    return restartAttempts < 1 ? "restart" : "stop";
  }
  return "stop";
}
