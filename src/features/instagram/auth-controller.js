import {
  isInstagramAuthRecoverySignal,
  nextInstagramCookiePrepareStep,
} from "./auth-policy.js";

export const INSTAGRAM_COOKIE_CONSENT_KEY = "mediadrop_instagram_cookie_consent";
export const YOUTUBE_COOKIE_CONSENT_KEY = "mediadrop_youtube_cookie_consent";
export const TWITTER_COOKIE_CONSENT_KEY = "mediadrop_twitter_cookie_consent";
export const SAVED_INSTAGRAM_AUTH_MODE = "saved:instagram";
export const PUBLIC_MEDIA_AUTH_MODE = "public";
export const BROWSER_MEDIA_AUTH_MODE = "browserAuto";

export const DEFAULT_COOKIE_BROWSER_FALLBACKS = [
  { id: "opera_gx", label: "Opera GX", installed: false, recommended: true, defaultBrowser: false },
  { id: "opera", label: "Opera", installed: false, recommended: false, defaultBrowser: false },
  { id: "chrome", label: "Google Chrome", installed: false, recommended: false, defaultBrowser: false },
  { id: "edge", label: "Microsoft Edge", installed: false, recommended: false, defaultBrowser: false },
  { id: "firefox", label: "Firefox", installed: false, recommended: false, defaultBrowser: false },
];

function text(value) {
  if (value === null || value === undefined) return "";
  return String(value).trim();
}

export function cookieBrowserAuthMode(
  browserId,
  { save = false, browserMode = BROWSER_MEDIA_AUTH_MODE } = {}
) {
  const clean = text(browserId);
  return clean ? `browser:${clean}${save ? ":save" : ""}` : browserMode;
}

export function isSavingInstagramAuthMode(authMode = "") {
  const clean = text(authMode);
  return clean.startsWith("browser:") && clean.endsWith(":save");
}

export function mediaAuthModeAfterSuccessfulAnalysis(
  authMode = "",
  savedMode = SAVED_INSTAGRAM_AUTH_MODE
) {
  return isSavingInstagramAuthMode(authMode) ? savedMode : authMode;
}

export function browserIdFromInstagramAuthMode(authMode = "") {
  const clean = text(authMode);
  if (!clean.startsWith("browser:")) return "";
  return clean.slice("browser:".length).replace(/:save$/, "").trim();
}

export function mediaAuthModeForUrl(_value, publicMode = PUBLIC_MEDIA_AUTH_MODE) {
  return publicMode;
}

export function isInstagramPhotoOrStoryUrl(value = "") {
  const lower = String(value || "").toLowerCase();
  return (
    lower.includes("instagram.com/p/") ||
    lower.includes("instagram.com/stories/") ||
    lower.includes("instagram.com/s/")
  );
}

export function isInstagramStoryUrl(value = "") {
  const lower = String(value || "").toLowerCase();
  return (
    !lower.includes("instagram.com/stories/highlights/") &&
    (lower.includes("instagram.com/stories/") || lower.includes("instagram.com/s/"))
  );
}

export function normalizeCookieBrowser(browser = {}) {
  const id = text(browser.id);
  if (!id) return null;

  return {
    id,
    label: text(browser.label) || id,
    installed: Boolean(browser.installed),
    recommended: Boolean(browser.recommended),
    defaultBrowser: Boolean(browser.defaultBrowser),
  };
}

export function recommendedCookieBrowserId(browsers = [], preferredId = "", avoidId = "") {
  const hasInstalled = browsers.some((browser) => browser.installed);
  const preferred = browsers.find(
    (browser) =>
      browser.id === preferredId &&
      browser.id !== avoidId &&
      (!hasInstalled || browser.installed)
  );
  if (preferred) return preferred.id;

  return (
    browsers.find(
      (browser) =>
        browser.id !== avoidId &&
        browser.recommended &&
        (!hasInstalled || browser.installed)
    )?.id ||
    browsers.find((browser) => browser.id !== avoidId && browser.installed)?.id ||
    browsers.find((browser) => browser.id !== avoidId)?.id ||
    browsers.find((browser) => browser.installed)?.id ||
    browsers[0]?.id ||
    "opera_gx"
  );
}

export function installedCookieBrowsers(browsers = []) {
  const installed = browsers.filter((browser) => browser.installed);
  return installed.length ? installed : [];
}

export function cookieBrowserDetail(browser) {
  if (!browser) return "Kurulu tarayıcı bulunamadı";
  const detailParts = [
    browser.defaultBrowser ? "Varsayılan tarayıcı" : "",
    browser.installed ? "Kurulu" : "",
    browser.recommended ? "Önerilen" : "",
  ].filter(Boolean);
  return detailParts.join(" · ") || "Kurulu";
}

export function cookiePermissionCopy(purpose = "instagram", error = null) {
  if (purpose === "youtube") {
    return {
      title: "YouTube oturum izni",
      status: error
        ? "Seçili tarayıcıda yaş doğrulaması yapılmış YouTube oturumu bulunamadı. YouTube'a giriş yaptığın başka bir tarayıcı seç."
        : "Yaş kısıtlamalı videoyu indirebilmek için YouTube'a giriş yaptığın tarayıcının oturum çerezlerini kullanmamız gerekiyor.",
    };
  }

  if (purpose === "twitter") {
    return {
      title: "X/Twitter oturum izni",
      status: error
        ? "Seçili tarayıcıda giriş yapılmış X/Twitter oturumu bulunamadı. X'e giriş yaptığın başka bir tarayıcı seç."
        : "Bu gönderideki videoyu ve açıklamayı alabilmek için X'e giriş yaptığın tarayıcının yalnızca X/Twitter oturum çerezlerini kullanmamız gerekiyor.",
    };
  }

  return {
    title: "Instagram çerez izni",
    status: error
      ? "Seçili tarayıcıdan Instagram oturumu okunamadı. Lütfen Instagram'a giriş yaptığın kurulu tarayıcıyı seçip tekrar izin ver."
      : "Bu linkteki görselleri indirebilmeniz için bazı çerezlerinize ihtiyacımız var! Lütfen tarayıcınızı seçiniz!",
  };
}

export function resolveSavedInstagramConsent(parsed, backendState) {
  const backendBrowserId = text(backendState?.browserId);
  const consentBrowserId = parsed?.allowed === true ? text(parsed?.browserId) : "";
  const cookieStatus = text(backendState?.status).toLowerCase();
  const hasSavedCookies =
    cookieStatus === "ready" && Boolean(backendState?.hasSavedCookies);
  const browserId = consentBrowserId || (hasSavedCookies ? backendBrowserId : "");
  if (!hasSavedCookies || !browserId) return null;

  return {
    browserId,
    consentedAtMs: Number(parsed?.consentedAtMs || backendState?.updatedAtMs || 0),
    hasSavedCookies,
    shouldRehydrateConsent: hasSavedCookies && !consentBrowserId,
  };
}

export function createInstagramAuthController({
  invoke,
  storage,
  documentRef,
  elements = {},
  parseBackendError = (error) => ({ message: String(error || ""), raw: error }),
  logger = console,
  now = () => Date.now(),
  consentKey = INSTAGRAM_COOKIE_CONSENT_KEY,
  youtubeConsentKey = YOUTUBE_COOKIE_CONSENT_KEY,
  twitterConsentKey = TWITTER_COOKIE_CONSENT_KEY,
  publicMode = PUBLIC_MEDIA_AUTH_MODE,
  savedMode = SAVED_INSTAGRAM_AUTH_MODE,
  browserMode = BROWSER_MEDIA_AUTH_MODE,
  browserFallbacks = DEFAULT_COOKIE_BROWSER_FALLBACKS,
  modalController = null,
  cookieModalId = "instagram-cookie-permission",
  browserRestartModalId = "instagram-browser-restart",
} = {}) {
  if (typeof invoke !== "function") {
    throw new TypeError("Instagram auth controller requires an invoke function.");
  }

  const {
    cookieAuthOverlay,
    cookieAuthTitle,
    cookieAuthStatus,
    cookieBrowserSelect,
    cookieBrowserSelectedName,
    cookieBrowserSelectedDetail,
    cookieBrowserList,
    cookieRememberCheck,
    cookieAllowBtn,
    cookieDenyBtn,
    browserRestartOverlay,
    browserRestartTitle,
    browserRestartStatus,
    browserRestartAllowBtn,
    browserRestartDenyBtn,
  } = elements;

  let pendingInstagramCookieConsent = null;
  let pendingInstagramCookiePrepareNotice = "";
  let activeDialogCancel = null;
  let browserRestartBusy = false;

  const saveInstagramCookieConsent = (browserId) => {
    storage?.setItem(
      consentKey,
      JSON.stringify({
        allowed: true,
        browserId,
        remember: true,
        consentedAtMs: now(),
      })
    );
  };

  const saveYoutubeCookieConsent = (browserId) => {
    const clean = text(browserId);
    if (!clean) return;
    storage?.setItem(youtubeConsentKey, JSON.stringify({ allowed: true, browserId: clean }));
  };

  const savedYoutubeCookieConsent = () => {
    try {
      const parsed = JSON.parse(storage?.getItem(youtubeConsentKey) || "null");
      const browserId = parsed?.allowed === true ? text(parsed.browserId) : "";
      return browserId ? { browserId } : null;
    } catch {
      return null;
    }
  };

  const clearYoutubeCookieConsent = () => storage?.removeItem(youtubeConsentKey);

  const saveTwitterCookieConsent = (browserId) => {
    const clean = text(browserId);
    if (!clean) return;
    storage?.setItem(twitterConsentKey, JSON.stringify({ allowed: true, browserId: clean }));
  };

  const savedTwitterCookieConsent = () => {
    try {
      const parsed = JSON.parse(storage?.getItem(twitterConsentKey) || "null");
      const browserId = parsed?.allowed === true ? text(parsed.browserId) : "";
      return browserId ? { browserId } : null;
    } catch {
      return null;
    }
  };

  const clearTwitterCookieConsent = () => storage?.removeItem(twitterConsentKey);

  const loadCookieBrowserOptions = async () => {
    try {
      const browsers = await invoke("list_cookie_browsers");
      const normalized = Array.isArray(browsers)
        ? browsers.map(normalizeCookieBrowser).filter(Boolean)
        : [];

      return normalized.length ? normalized : browserFallbacks;
    } catch (error) {
      logger.warn("Cookie browsers could not be listed:", error);
      return browserFallbacks;
    }
  };

  const savedInstagramCookieConsent = async () => {
    try {
      const parsed = JSON.parse(storage?.getItem(consentKey) || "null");
      let state = null;
      try {
        state = await invoke("get_instagram_cookie_state");
      } catch (error) {
        logger.warn("Instagram cookie state could not be read:", error);
      }

      const resolved = resolveSavedInstagramConsent(parsed, state);
      if (!resolved) return null;

      if (resolved.shouldRehydrateConsent) {
        saveInstagramCookieConsent(resolved.browserId);
      }

      const { shouldRehydrateConsent: _ignored, ...consent } = resolved;
      return consent;
    } catch {
      return null;
    }
  };

  const clearInstagramCookieConsent = ({ clearBackend = true } = {}) => {
    storage?.removeItem(consentKey);
    pendingInstagramCookieConsent = null;
    if (clearBackend) {
      invoke("clear_instagram_cookie_state").catch((error) => {
        logger.warn("Instagram cookie state could not be cleared:", error);
      });
    }
  };

  const setCookieBrowserDropdownOpen = (open) => {
    const isOpen = Boolean(open);
    cookieBrowserList?.classList.toggle("is-open", isOpen);
    cookieBrowserList?.setAttribute("aria-hidden", isOpen ? "false" : "true");
    if (cookieBrowserList && "inert" in cookieBrowserList) cookieBrowserList.inert = !isOpen;
    if (isOpen) cookieBrowserList?.removeAttribute?.("inert");
    else cookieBrowserList?.setAttribute("inert", "");
    cookieBrowserSelect?.setAttribute("aria-expanded", open ? "true" : "false");
  };

  const updateCookieBrowserSelected = (browser) => {
    if (cookieBrowserList) cookieBrowserList.dataset.selectedId = text(browser?.id);
    if (cookieBrowserSelectedName) {
      cookieBrowserSelectedName.textContent = browser?.label || "Tarayıcı bulunamadı";
    }
    if (cookieBrowserSelectedDetail) {
      cookieBrowserSelectedDetail.textContent = cookieBrowserDetail(browser);
    }
  };

  const renderCookieBrowserOptions = (browsers = [], selectedId = "") => {
    if (!cookieBrowserList) return;

    const options = installedCookieBrowsers(browsers);
    const selected = options.find((browser) => browser.id === selectedId) || options[0] || null;
    cookieBrowserList.replaceChildren();
    setCookieBrowserDropdownOpen(false);
    updateCookieBrowserSelected(selected);

    if (cookieBrowserSelect) {
      cookieBrowserSelect.disabled = options.length <= 1;
      cookieBrowserSelect.classList.toggle("is-disabled", options.length <= 1);
    }
    if (cookieAllowBtn) cookieAllowBtn.disabled = !selected;

    options.forEach((browser) => {
      const option = documentRef.createElement("button");
      option.type = "button";
      option.setAttribute("role", "option");
      option.className = "cookie-browser-option";
      option.dataset.browserId = browser.id;
      option.setAttribute("aria-selected", browser.id === selected?.id ? "true" : "false");

      const content = documentRef.createElement("span");
      content.className = "cookie-browser-copy";
      const title = documentRef.createElement("strong");
      title.textContent = browser.label;
      const detail = documentRef.createElement("small");
      detail.textContent = cookieBrowserDetail(browser);

      content.append(title, detail);
      option.append(content);
      option.addEventListener("click", () => {
        updateCookieBrowserSelected(browser);
        [...cookieBrowserList.querySelectorAll(".cookie-browser-option")].forEach((node) => {
          node.setAttribute("aria-selected", node === option ? "true" : "false");
        });
        setCookieBrowserDropdownOpen(false);
      });
      cookieBrowserList.appendChild(option);
    });
  };

  const hideCookieAuthOverlay = () => {
    setCookieBrowserDropdownOpen(false);
    if (modalController?.close) {
      modalController.close(cookieModalId);
      return;
    }
    cookieAuthOverlay?.classList.add("is-hidden");
    cookieAuthOverlay?.setAttribute("aria-hidden", "true");
  };

  const setBrowserRestartOverlayVisible = (visible) => {
    if (modalController?.open && modalController?.close) {
      if (visible) modalController.open(browserRestartModalId);
      else modalController.close(browserRestartModalId);
      return;
    }
    browserRestartOverlay?.classList.toggle("is-hidden", !visible);
    browserRestartOverlay?.setAttribute("aria-hidden", visible ? "false" : "true");
  };

  const setBrowserRestartBusy = ({ title, status }) => {
    browserRestartBusy = true;
    if (browserRestartTitle) browserRestartTitle.textContent = title || "Tarayıcı hazırlanıyor";
    if (browserRestartStatus) browserRestartStatus.textContent = status || "İşlem sürüyor...";
    if (browserRestartAllowBtn) {
      browserRestartAllowBtn.disabled = true;
      browserRestartAllowBtn.textContent = "Lütfen bekle";
    }
    if (browserRestartDenyBtn) browserRestartDenyBtn.disabled = true;
    setBrowserRestartOverlayVisible(true);
  };

  const hideBrowserRestartOverlay = () => {
    browserRestartBusy = false;
    setBrowserRestartOverlayVisible(false);
    if (browserRestartAllowBtn) browserRestartAllowBtn.disabled = false;
    if (browserRestartDenyBtn) browserRestartDenyBtn.disabled = false;
  };

  const requestBrowserRestartConfirmation = ({ browserLabel = "Tarayıcı", force = false } = {}) => {
    const title = force ? `${browserLabel} kapanmadı` : `${browserLabel} açık görünüyor`;
    const status = force
      ? `${browserLabel} normal şekilde kapanmadı. Kaydedilmemiş işlerin varsa kaydettiğinden emin ol; onay verirsen zorla kapatıp çerezleri okuyacağız.`
      : `Çerezleri güvenli okuyabilmemiz için ${browserLabel} tarayıcını kısa süreliğine yeniden başlatmamız gerekiyor. Lütfen kaydedilmemiş işlerini kaydet.`;
    const allowText = force
      ? "Zorla kapat ve devam et"
      : "Tarayıcıyı yeniden başlat ve devam et";

    if (browserRestartTitle) browserRestartTitle.textContent = title;
    if (browserRestartStatus) browserRestartStatus.textContent = status;
    if (browserRestartAllowBtn) {
      browserRestartAllowBtn.textContent = allowText;
      browserRestartAllowBtn.disabled = false;
    }
    if (browserRestartDenyBtn) {
      browserRestartDenyBtn.textContent = "Vazgeç";
      browserRestartDenyBtn.disabled = false;
    }

    browserRestartBusy = false;
    setBrowserRestartOverlayVisible(true);

    return new Promise((resolve) => {
      const finish = (allowed) => {
        browserRestartAllowBtn?.removeEventListener("click", onAllow);
        browserRestartDenyBtn?.removeEventListener("click", onDeny);
        hideBrowserRestartOverlay();
        if (activeDialogCancel === onDeny) activeDialogCancel = null;
        resolve(Boolean(allowed));
      };
      const onAllow = () => finish(true);
      const onDeny = () => finish(false);

      activeDialogCancel = onDeny;
      browserRestartAllowBtn?.addEventListener("click", onAllow);
      browserRestartDenyBtn?.addEventListener("click", onDeny);
    });
  };

  const requestCookieBrowserPermission = async ({
    preferredBrowserId = "",
    avoidBrowserId = "",
    error = null,
    purpose = "instagram",
  } = {}) => {
    const browsers = await loadCookieBrowserOptions();
    const selectedId = recommendedCookieBrowserId(browsers, preferredBrowserId, avoidBrowserId);
    const installed = installedCookieBrowsers(browsers);

    renderCookieBrowserOptions(browsers, selectedId);
    if (cookieRememberCheck) cookieRememberCheck.checked = true;
    const copy = cookiePermissionCopy(purpose, error);
    if (cookieAuthTitle) cookieAuthTitle.textContent = copy.title;
    if (cookieAuthStatus) {
      cookieAuthStatus.textContent = !installed.length
        ? "Desteklenen kurulu tarayıcı bulunamadı."
        : copy.status;
    }

    return new Promise((resolve) => {
      const finish = (result) => {
        cookieAllowBtn?.removeEventListener("click", onAllow);
        cookieDenyBtn?.removeEventListener("click", onDeny);
        cookieBrowserSelect?.removeEventListener("click", onToggleBrowserList);
        documentRef?.removeEventListener("click", onOutsideClick);
        hideCookieAuthOverlay();
        if (activeDialogCancel === onDeny) activeDialogCancel = null;
        resolve(result);
      };

      const onToggleBrowserList = (event) => {
        event.stopPropagation();
        if (!installed.length || cookieBrowserSelect?.disabled) return;
        const isOpen = cookieBrowserList?.classList.contains("is-open");
        setCookieBrowserDropdownOpen(!isOpen);
      };

      const onOutsideClick = (event) => {
        if (
          cookieBrowserList?.classList.contains("is-open") &&
          !cookieBrowserList.contains(event.target) &&
          !cookieBrowserSelect?.contains(event.target)
        ) {
          setCookieBrowserDropdownOpen(false);
        }
      };

      const onAllow = () => {
        const browserId = text(cookieBrowserList?.dataset.selectedId) || selectedId || "";
        if (!browserId) {
          if (cookieAuthStatus) {
            cookieAuthStatus.textContent = "Desteklenen kurulu tarayıcı bulunamadı.";
          }
          return;
        }

        finish({
          allowed: true,
          browserId,
          remember: cookieRememberCheck?.checked !== false,
        });
      };

      const onDeny = () => finish({ allowed: false, browserId: "", remember: false });

      activeDialogCancel = onDeny;
      cookieAllowBtn?.addEventListener("click", onAllow);
      cookieDenyBtn?.addEventListener("click", onDeny);
      cookieBrowserSelect?.addEventListener("click", onToggleBrowserList);
      documentRef?.addEventListener("click", onOutsideClick);
      if (modalController?.open) {
        modalController.open(cookieModalId);
      } else {
        cookieAuthOverlay?.classList.remove("is-hidden");
        cookieAuthOverlay?.setAttribute("aria-hidden", "false");
      }
    });
  };

  const requestInstagramCookiePermission = (options = {}) =>
    requestCookieBrowserPermission({ ...options, purpose: "instagram" });

  const requestYoutubeCookiePermission = (options = {}) =>
    requestCookieBrowserPermission({ ...options, purpose: "youtube" });

  const requestTwitterCookiePermission = (options = {}) =>
    requestCookieBrowserPermission({ ...options, purpose: "twitter" });

  const prepareInstagramCookieAuthFromPermission = async (permission) => {
    const browserId = text(permission?.browserId);
    const remember = permission?.remember !== false;
    if (!browserId) throw new Error("Desteklenen kurulu tarayıcı bulunamadı.");

    let browserLabel = cookieBrowserSelectedName?.textContent || "Tarayıcı";
    try {
      const state = await invoke("get_cookie_browser_runtime_state", { browserId });
      browserLabel = state?.label || browserLabel;
    } catch (error) {
      logger.warn("Cookie browser runtime state could not be read:", error);
    }

    let restartBrowser = false;
    let forceClose = false;
    let restartPromptAttempts = 0;
    let forcePromptAttempts = 0;

    for (let attempt = 0; attempt < 3; attempt += 1) {
      setBrowserRestartBusy({
        title: restartBrowser
          ? `${browserLabel} hazırlanıyor`
          : "Instagram çerezleri hazırlanıyor",
        status: restartBrowser
          ? `${browserLabel} kapatılıyor, çerezler okunuyor ve ardından yeniden açılacak...`
          : "Çerezler okunuyor...",
      });

      try {
        const result = await invoke("prepare_instagram_cookie_auth", {
          browserId,
          remember,
          restartBrowser,
          forceClose,
        });
        hideBrowserRestartOverlay();
        if (result?.relaunchError) {
          logger.warn("Browser relaunch failed:", result.relaunchError);
          pendingInstagramCookiePrepareNotice = text(result?.message);
        }
        return {
          authMode: text(result?.authMode),
          saved: Boolean(result?.saved),
          browserId: text(result?.browserId) || browserId,
          label: text(result?.label) || browserLabel,
          message: text(result?.message),
        };
      } catch (error) {
        hideBrowserRestartOverlay();
        const parsed = parseBackendError(error);
        const recoveryStep = nextInstagramCookiePrepareStep({
          code: parsed.code || parsed.debugCode,
          restartPromptAttempts,
          forcePromptAttempts,
        });
        if (recoveryStep === "restart") {
          restartPromptAttempts += 1;
          const allowed = await requestBrowserRestartConfirmation({
            browserLabel,
            force: false,
          });
          if (!allowed) {
            throw new Error(
              "Tarayıcı yeniden başlatma iznini reddettiniz. Görsel/Görseller indirilemedi."
            );
          }
          restartBrowser = true;
          forceClose = false;
          continue;
        }
        if (recoveryStep === "force-close") {
          forcePromptAttempts += 1;
          const allowed = await requestBrowserRestartConfirmation({
            browserLabel,
            force: true,
          });
          if (!allowed) {
            throw new Error(
              "Tarayıcıyı zorla kapatma iznini reddettiniz. Görsel/Görseller indirilemedi."
            );
          }
          restartBrowser = true;
          forceClose = true;
          continue;
        }

        throw error;
      }
    }

    throw new Error("Instagram çerezleri hazırlanamadı.");
  };

  const instagramCookieAuthMode = async ({
    forcePrompt = false,
    error = null,
    avoidBrowserId = "",
  } = {}) => {
    const saved = await savedInstagramCookieConsent();
    if (saved?.hasSavedCookies && !forcePrompt) {
      pendingInstagramCookieConsent = null;
      return savedMode;
    }

    const permission = await requestInstagramCookiePermission({
      preferredBrowserId: saved?.browserId || "",
      avoidBrowserId,
      error,
    });

    if (!permission.allowed) {
      clearInstagramCookieConsent();
      throw new Error("Çerez izinlerini reddettiniz. Görsel/Görseller indirilemedi.");
    }

    const prepared = await prepareInstagramCookieAuthFromPermission(permission);
    if (!prepared.authMode) {
      throw new Error("Instagram çerezleri hazırlandı ama auth modu alınamadı.");
    }

    if (permission.remember && prepared.saved) {
      saveInstagramCookieConsent(prepared.browserId || permission.browserId);
      pendingInstagramCookieConsent = null;
    } else if (!permission.remember) {
      clearInstagramCookieConsent();
    }

    return prepared.authMode;
  };

  const confirmPendingInstagramCookieConsent = (authMode = "") => {
    if (!pendingInstagramCookieConsent?.browserId) return;
    if (!isSavingInstagramAuthMode(authMode)) return;

    saveInstagramCookieConsent(pendingInstagramCookieConsent.browserId);
    pendingInstagramCookieConsent = null;
  };

  const isInstagramAuthRecoverableError = (error) => {
    const parsed = parseBackendError(error);
    return isInstagramAuthRecoverySignal({
      code: parsed.code,
      action: parsed.raw?.action || parsed.raw?.recommendedAction || "",
      message: parsed.message || String(error || ""),
    });
  };

  return {
    browserIdFromInstagramAuthMode,
    clearInstagramCookieConsent,
    clearTwitterCookieConsent,
    clearYoutubeCookieConsent,
    cancelActiveDialog: () => {
      if (!activeDialogCancel) return false;
      activeDialogCancel();
      return true;
    },
    confirmPendingInstagramCookieConsent,
    instagramCookieAuthMode,
    isInstagramAuthRecoverableError,
    isBrowserRestartBusy: () => browserRestartBusy,
    isInstagramPhotoOrStoryUrl,
    isInstagramStoryUrl,
    mediaAuthModeAfterSuccessfulAnalysis: (authMode) =>
      mediaAuthModeAfterSuccessfulAnalysis(authMode, savedMode),
    mediaAuthModeForUrl: (value) => mediaAuthModeForUrl(value, publicMode),
    prepareInstagramCookieAuthFromPermission,
    requestBrowserRestartConfirmation,
    requestInstagramCookiePermission,
    requestTwitterCookiePermission,
    requestYoutubeCookiePermission,
    saveTwitterCookieConsent,
    saveYoutubeCookieConsent,
    savedInstagramCookieConsent,
    savedTwitterCookieConsent,
    savedYoutubeCookieConsent,
    clearPrepareNotice() {
      pendingInstagramCookiePrepareNotice = "";
    },
    takePrepareNotice() {
      const notice = pendingInstagramCookiePrepareNotice;
      pendingInstagramCookiePrepareNotice = "";
      return notice;
    },
    getState() {
      return {
        hasPendingConsent: Boolean(pendingInstagramCookieConsent?.browserId),
        prepareNotice: pendingInstagramCookiePrepareNotice,
        browserMode,
      };
    },
  };
}
