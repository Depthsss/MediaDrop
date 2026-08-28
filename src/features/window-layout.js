export const WINDOW_HEIGHT_PRESETS = Object.freeze({
  main: 650,
  media: 840,
  clip: 830,
  min: 520,
  max: 980,
});

export function clampWindowHeight(
  value,
  fallback = WINDOW_HEIGHT_PRESETS.main,
  min = WINDOW_HEIGHT_PRESETS.min,
  max = WINDOW_HEIGHT_PRESETS.max
) {
  const numeric = Number(value);
  const safe = Number.isFinite(numeric) ? numeric : Number(fallback);
  return Math.max(min, Math.min(max, Math.round(safe)));
}

export function measureWindowContentHeight({
  scrollHeight = 0,
  rectHeight = 0,
  outerPadding = 20,
  fallbackHeight = WINDOW_HEIGHT_PRESETS.main,
  minHeight = WINDOW_HEIGHT_PRESETS.min,
  maxHeight = WINDOW_HEIGHT_PRESETS.max,
} = {}) {
  const contentHeight = Math.max(Number(scrollHeight) || 0, Number(rectHeight) || 0);
  if (contentHeight <= outerPadding) {
    return clampWindowHeight(fallbackHeight, fallbackHeight, minHeight, maxHeight);
  }
  return clampWindowHeight(
    Math.ceil(contentHeight + outerPadding),
    fallbackHeight,
    minHeight,
    maxHeight
  );
}

export function isDuplicateWindowHeight(nextHeight, lastHeight, tolerance = 2) {
  const next = Number(nextHeight);
  const previous = Number(lastHeight);
  return Number.isFinite(next) && Number.isFinite(previous) && Math.abs(next - previous) < tolerance;
}

export function isLikelyProgrammaticResize({
  now = Date.now(),
  lastRequestAt = 0,
  graceMs = 900,
} = {}) {
  const elapsed = Number(now) - Number(lastRequestAt);
  return Number(lastRequestAt) > 0 && elapsed >= 0 && elapsed <= graceMs;
}

export function createWindowLayoutCoordinator({
  initialMode = "main",
  debounceMs = 55,
  manualHoldMs = 1800,
  manualResizeMode = "suspend",
  programmaticGraceMs = 900,
  measureHeight,
  requestHeight,
  requestFrame = (callback) => requestAnimationFrame(callback),
  setTimer = (callback, delay) => setTimeout(callback, delay),
  clearTimer = (timer) => clearTimeout(timer),
  now = () => Date.now(),
  onError = () => {},
} = {}) {
  if (typeof measureHeight !== "function" || typeof requestHeight !== "function") {
    throw new TypeError("Window layout coordinator requires measureHeight and requestHeight callbacks.");
  }

  const state = {
    mode: initialMode,
    lastRequestedHeight: 0,
    lastRequestAt: 0,
    manualHoldUntil: 0,
    autoResizeSuspended: false,
  };
  let resizeTimer = null;
  let resizeObserver = null;
  let attachedWindow = null;

  const waitForLayout = () => new Promise((resolve) => {
    requestFrame(() => requestFrame(resolve));
  });

  const canAutoResize = (force = false) => {
    if (force) return true;
    if (state.autoResizeSuspended) return false;
    return now() >= state.manualHoldUntil;
  };

  async function resizeNow(height, { force = false } = {}) {
    if (!canAutoResize(force)) return false;
    const safeHeight = clampWindowHeight(height);
    if (isDuplicateWindowHeight(safeHeight, state.lastRequestedHeight)) return false;

    state.lastRequestedHeight = safeHeight;
    state.lastRequestAt = now();
    try {
      await requestHeight(safeHeight);
      return true;
    } catch (error) {
      state.lastRequestedHeight = 0;
      onError(error);
      return false;
    }
  }

  function schedule({ mode = state.mode, fallbackHeight = null, force = false } = {}) {
    state.mode = mode || state.mode;
    if (!canAutoResize(force)) return false;
    if (resizeTimer !== null) clearTimer(resizeTimer);

    resizeTimer = setTimer(async () => {
      resizeTimer = null;
      await waitForLayout();
      if (!canAutoResize(force)) return;
      const measured = measureHeight(state.mode, fallbackHeight);
      await resizeNow(measured || fallbackHeight || WINDOW_HEIGHT_PRESETS.main, { force });
    }, debounceMs);
    return true;
  }

  function resumeAutoResize() {
    state.autoResizeSuspended = false;
    state.manualHoldUntil = 0;
  }

  function setMode(mode, fallbackHeight = null) {
    const changed = Boolean(mode && mode !== state.mode);
    if (changed) resumeAutoResize();
    state.mode = mode || state.mode;
    return schedule({ mode: state.mode, fallbackHeight, force: changed });
  }

  function handleWindowResize() {
    if (isLikelyProgrammaticResize({
      now: now(),
      lastRequestAt: state.lastRequestAt,
      graceMs: programmaticGraceMs,
    })) {
      return "programmatic";
    }

    state.manualHoldUntil = now() + manualHoldMs;
    state.autoResizeSuspended = manualResizeMode === "suspend";
    if (resizeTimer !== null) {
      clearTimer(resizeTimer);
      resizeTimer = null;
    }
    return "manual";
  }

  function attach({ windowTarget, ResizeObserverClass, observedElements = [], fontsReady } = {}) {
    disposeBindings();
    attachedWindow = windowTarget || null;
    attachedWindow?.addEventListener?.("resize", handleWindowResize);

    if (typeof ResizeObserverClass === "function") {
      resizeObserver = new ResizeObserverClass(() => schedule());
      observedElements.filter(Boolean).forEach((element) => resizeObserver.observe(element));
    }

    Promise.resolve(fontsReady).then(() => schedule()).catch(() => {});
  }

  function disposeBindings() {
    resizeObserver?.disconnect?.();
    resizeObserver = null;
    attachedWindow?.removeEventListener?.("resize", handleWindowResize);
    attachedWindow = null;
  }

  function dispose() {
    disposeBindings();
    if (resizeTimer !== null) clearTimer(resizeTimer);
    resizeTimer = null;
  }

  return {
    attach,
    dispose,
    getMode: () => state.mode,
    getState: () => ({ ...state }),
    handleWindowResize,
    refresh: () => schedule(),
    resizeNow,
    resumeAutoResize,
    schedule,
    setMode,
  };
}
