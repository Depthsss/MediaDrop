import test from "node:test";
import assert from "node:assert/strict";

import { parseBackendError } from "../src/app/errors.js";
import {
  browserIdFromInstagramAuthMode,
  cookiePermissionCopy,
  cookieBrowserAuthMode,
  createInstagramAuthController,
  isInstagramPhotoOrStoryUrl,
  isInstagramStoryUrl,
  mediaAuthModeAfterSuccessfulAnalysis,
  normalizeCookieBrowser,
  recommendedCookieBrowserId,
  resolveSavedInstagramConsent,
} from "../src/features/instagram/auth-controller.js";

test("shared browser picker explains YouTube age verification without claiming Instagram access", () => {
  const copy = cookiePermissionCopy("youtube");

  assert.equal(copy.title, "YouTube oturum izni");
  assert.match(copy.status, /yaş kısıtlamalı/i);
  assert.doesNotMatch(copy.status, /Instagram/i);
});

test("shared browser picker explains X session access without claiming Instagram access", () => {
  const copy = cookiePermissionCopy("twitter");

  assert.equal(copy.title, "X/Twitter oturum izni");
  assert.match(copy.status, /gönderideki videoyu/i);
  assert.doesNotMatch(copy.status, /Instagram/i);
});

test("YouTube browser consent stores only the selected browser id", () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  };
  const controller = createInstagramAuthController({
    invoke: async () => null,
    storage,
    parseBackendError,
  });

  controller.saveYoutubeCookieConsent(" chrome ");
  assert.deepEqual(controller.savedYoutubeCookieConsent(), { browserId: "chrome" });
  controller.clearYoutubeCookieConsent();
  assert.equal(controller.savedYoutubeCookieConsent(), null);
});

test("X browser consent stores only the selected browser id", () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  };
  const controller = createInstagramAuthController({
    invoke: async () => null,
    storage,
    parseBackendError,
  });

  controller.saveTwitterCookieConsent(" opera ");
  assert.deepEqual(controller.savedTwitterCookieConsent(), { browserId: "opera" });
  controller.clearTwitterCookieConsent();
  assert.equal(controller.savedTwitterCookieConsent(), null);
});

test("Instagram auth helpers normalize modes and distinguish story URLs", () => {
  assert.equal(cookieBrowserAuthMode(" chrome ", { save: true }), "browser:chrome:save");
  assert.equal(browserIdFromInstagramAuthMode("browser:chrome:save"), "chrome");
  assert.equal(mediaAuthModeAfterSuccessfulAnalysis("browser:chrome:save"), "saved:instagram");
  assert.equal(isInstagramStoryUrl("https://instagram.com/stories/user/42/"), true);
  assert.equal(isInstagramStoryUrl("https://instagram.com/s/token"), true);
  assert.equal(isInstagramStoryUrl("https://instagram.com/stories/highlights/42/"), false);
  assert.equal(isInstagramStoryUrl("https://instagram.com/p/post"), false);
  assert.equal(isInstagramPhotoOrStoryUrl("https://instagram.com/p/post"), true);
});

test("browser recommendation honors installed, preferred and avoided browsers", () => {
  const browsers = [
    normalizeCookieBrowser({ id: "chrome", label: "Chrome", installed: true }),
    normalizeCookieBrowser({ id: "edge", label: "Edge", installed: true, recommended: true }),
    normalizeCookieBrowser({ id: "opera", label: "Opera", installed: false, recommended: true }),
  ];

  assert.equal(recommendedCookieBrowserId(browsers, "chrome"), "chrome");
  assert.equal(recommendedCookieBrowserId(browsers, "chrome", "chrome"), "edge");
  assert.equal(recommendedCookieBrowserId(browsers, "opera"), "edge");
});

test("saved consent resolution rehydrates only a valid backend cookie jar", () => {
  assert.deepEqual(
    resolveSavedInstagramConsent(null, {
      browserId: "chrome",
      hasSavedCookies: true,
      status: "ready",
      updatedAtMs: 123,
    }),
    {
      browserId: "chrome",
      consentedAtMs: 123,
      hasSavedCookies: true,
      shouldRehydrateConsent: true,
    }
  );
  assert.equal(
    resolveSavedInstagramConsent(null, {
      browserId: "chrome",
      hasSavedCookies: false,
      status: "missing",
    }),
    null
  );

  for (const status of ["missing", "expired", "invalid", "browser_locked"]) {
    assert.equal(
      resolveSavedInstagramConsent(
        { allowed: true, browserId: "chrome", consentedAtMs: 456 },
        { browserId: "chrome", hasSavedCookies: true, status }
      ),
      null,
      `${status} must not be treated as a reusable saved session`
    );
  }
});

test("valid saved cookies return saved auth mode without opening a permission prompt", async () => {
  const values = new Map();
  const commands = [];
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  };
  const invoke = async (command) => {
    commands.push(command);
    if (command === "get_instagram_cookie_state") {
      return {
        browserId: "chrome",
        hasSavedCookies: true,
        status: "ready",
        updatedAtMs: 456,
      };
    }
    throw new Error(`unexpected command: ${command}`);
  };
  const controller = createInstagramAuthController({
    invoke,
    storage,
    parseBackendError,
    now: () => 789,
  });

  assert.equal(await controller.instagramCookieAuthMode(), "saved:instagram");
  assert.deepEqual(commands, ["get_instagram_cookie_state"]);
  assert.equal(JSON.parse(values.get("mediadrop_instagram_cookie_consent")).browserId, "chrome");
});

test("typed backend error.code is preserved and drives auth recovery", () => {
  const typed = { code: "instagram_auth_required", message: "Oturum gerekli" };
  const parsed = parseBackendError(typed);
  assert.equal(parsed.code, "instagram_auth_required");
  assert.equal(parsed.debugCode, "instagram_auth_required");

  const controller = createInstagramAuthController({
    invoke: async () => null,
    storage: null,
    parseBackendError,
  });
  assert.equal(controller.isInstagramAuthRecoverableError(typed), true);
  assert.equal(
    controller.isInstagramAuthRecoverableError({
      code: "instagram_schema_error",
      message: "Oturum benzeri ama parser hatası",
    }),
    false
  );
});

function fakeAuthElement({ autoClick = false, onClickListener = null } = {}) {
  const listeners = new Map();
  return {
    disabled: false,
    textContent: "",
    classList: {
      add() {},
      toggle() {},
    },
    setAttribute() {},
    addEventListener(type, listener) {
      const handlers = listeners.get(type) || new Set();
      handlers.add(listener);
      listeners.set(type, handlers);
      if (type === "click") onClickListener?.(this.textContent);
      if (type === "click" && autoClick) {
        queueMicrotask(() => {
          if (listeners.get(type)?.has(listener)) listener();
        });
      }
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
  };
}

function restartDialogElements({ autoApprove = false, confirmations = [] } = {}) {
  return {
    browserRestartOverlay: fakeAuthElement(),
    browserRestartTitle: fakeAuthElement(),
    browserRestartStatus: fakeAuthElement(),
    browserRestartAllowBtn: fakeAuthElement({
      autoClick: autoApprove,
      onClickListener: (label) => confirmations.push(label),
    }),
    browserRestartDenyBtn: fakeAuthElement(),
  };
}

test("running browser first attempts safe cookie preparation without a restart prompt", async () => {
  const prepareCalls = [];
  const confirmations = [];
  const invoke = async (command, args) => {
    if (command === "get_cookie_browser_runtime_state") {
      return { label: "Chrome", running: true };
    }
    if (command === "prepare_instagram_cookie_auth") {
      prepareCalls.push(args);
      return {
        authMode: "saved:instagram",
        saved: true,
        browserId: "chrome",
        label: "Chrome",
      };
    }
    throw new Error(`unexpected command: ${command}`);
  };
  const controller = createInstagramAuthController({
    invoke,
    storage: null,
    parseBackendError,
    elements: restartDialogElements({ autoApprove: true, confirmations }),
  });

  await controller.prepareInstagramCookieAuthFromPermission({
    browserId: "chrome",
    remember: true,
  });

  assert.deepEqual(
    prepareCalls.map(({ restartBrowser, forceClose }) => ({ restartBrowser, forceClose })),
    [{ restartBrowser: false, forceClose: false }]
  );
  assert.deepEqual(confirmations, []);
});

test("typed browser lock gets one graceful restart before an explicit force-close recovery", async () => {
  const prepareCalls = [];
  const confirmations = [];
  const invoke = async (command, args) => {
    if (command === "get_cookie_browser_runtime_state") {
      return { label: "Chrome", running: true };
    }
    if (command !== "prepare_instagram_cookie_auth") {
      throw new Error(`unexpected command: ${command}`);
    }

    prepareCalls.push(args);
    if (prepareCalls.length === 1) {
      throw { code: "browser_restart_required", message: "Cookie database is locked" };
    }
    if (prepareCalls.length === 2) {
      throw { code: "browser_still_running", message: "Browser did not close" };
    }
    return {
      authMode: "saved:instagram",
      saved: true,
      browserId: "chrome",
      label: "Chrome",
    };
  };
  const controller = createInstagramAuthController({
    invoke,
    storage: null,
    parseBackendError,
    elements: restartDialogElements({ autoApprove: true, confirmations }),
  });

  await controller.prepareInstagramCookieAuthFromPermission({
    browserId: "chrome",
    remember: true,
  });

  assert.deepEqual(
    prepareCalls.map(({ restartBrowser, forceClose }) => ({ restartBrowser, forceClose })),
    [
      { restartBrowser: false, forceClose: false },
      { restartBrowser: true, forceClose: false },
      { restartBrowser: true, forceClose: true },
    ]
  );
  assert.equal(confirmations.length, 2);
  assert.match(confirmations[0], /yeniden başlat/i);
  assert.match(confirmations[1], /zorla kapat/i);
});
