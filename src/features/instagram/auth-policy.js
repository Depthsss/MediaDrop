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
  "request_browser_restart",
]);

const COOKIE_PREPARE_CLOSE_CODES = new Set([
  "instagram_browser_locked",
  "browser_restart_required",
  "browser_still_running",
]);

function normalizedSignal(value) {
  return String(value ?? "").trim().toLowerCase().replace(/[\s-]+/g, "_");
}

export function instagramInitialAuthMode({
  isStory = false,
  hasSavedCookies = false,
  forcePrompt = false,
  publicMode = "public",
  savedMode = "saved:instagram",
} = {}) {
  if (forcePrompt) return null;
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
    text.includes("redirect to login page") ||
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

export async function recoverInstagramDownloadAuth(error, {
  requestAuth,
  refreshAnalysis,
} = {}) {
  if (!AUTH_CODES.has(normalizedSignal(error?.code))) return false;
  const authMode = await requestAuth(error);
  await refreshAnalysis(authMode);
  return true;
}

export async function executeInstagramDownloadWithRecovery({
  executeDownload,
  requestAuth,
  refreshAnalysis,
} = {}) {
  try {
    return { result: await executeDownload(), recovered: false };
  } catch (error) {
    if (!AUTH_CODES.has(normalizedSignal(error?.code))) throw error;

    try {
      await recoverInstagramDownloadAuth(error, { requestAuth, refreshAnalysis });
    } catch {
      throw error;
    }

    return { result: null, recovered: true };
  }
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
  forcePromptAttempts = 0,
} = {}) {
  const normalizedCode = normalizedSignal(code);
  const forceAttempts = Math.max(0, Number(forcePromptAttempts) || 0);
  return COOKIE_PREPARE_CLOSE_CODES.has(normalizedCode) && forceAttempts < 1
    ? "force-close"
    : "stop";
}
