const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "audio[controls]",
  "video[controls]",
  '[contenteditable="true"]',
  '[tabindex]:not([tabindex="-1"])',
].join(",");

export function nextModalFocusIndex(currentIndex, count, shiftKey = false) {
  if (!Number.isInteger(count) || count <= 0) return -1;

  if (!Number.isInteger(currentIndex) || currentIndex < 0 || currentIndex >= count) {
    return shiftKey ? count - 1 : 0;
  }

  return (currentIndex + (shiftKey ? -1 : 1) + count) % count;
}

export function modalCanDismiss({ busy = false, escapeEnabled = true } = {}) {
  return !Boolean(busy) && escapeEnabled !== false;
}

function elementIsHidden(element) {
  if (!element || element.disabled || element.hidden) return true;
  if (element.inert || element.getAttribute?.("inert") !== null) return true;
  if (element.getAttribute?.("aria-hidden") === "true") return true;
  if (element.closest?.('[aria-hidden="true"], [inert], .is-hidden')) return true;
  return false;
}

export function focusableModalElements(container) {
  if (!container?.querySelectorAll) return [];

  return [...container.querySelectorAll(FOCUSABLE_SELECTOR)].filter(
    (element) => typeof element?.focus === "function" && !elementIsHidden(element)
  );
}

function resolveValue(value, fallback = null) {
  if (typeof value === "function") return value();
  return value ?? fallback;
}

function focusElement(element) {
  if (
    !element ||
    typeof element.focus !== "function" ||
    element.isConnected === false ||
    elementIsHidden(element)
  ) {
    return false;
  }

  try {
    element.focus({ preventScroll: true });
  } catch {
    try {
      element.focus();
    } catch {
      return false;
    }
  }

  const ownerDocument = element.ownerDocument;
  return !ownerDocument || ownerDocument.activeElement === element;
}

export function createModalController({
  documentRef = globalThis.document,
  scheduleFocus = (callback) => queueMicrotask(callback),
  onError = (error) => console.error(error),
} = {}) {
  const registrations = new Map();
  const openStack = [];
  let attached = false;

  const activeId = () => openStack.at(-1) || "";
  const activeRegistration = () => registrations.get(activeId()) || null;
  const syncDocumentModalState = () => {
    documentRef?.body?.classList?.toggle("has-modal-open", openStack.length > 0);
  };

  const reportError = (error) => {
    try {
      onError?.(error);
    } catch {}
  };

  const focusRegistration = (registration) => {
    if (!registration?.element) return;

    let target = null;
    try {
      target = resolveValue(registration.initialFocus);
    } catch (error) {
      reportError(error);
    }

    const focusables = focusableModalElements(registration.element);
    focusElement(target) || focusElement(focusables[0]) || focusElement(registration.element);
  };

  const register = (id, options = {}) => {
    const key = String(id || "").trim();
    if (!key) throw new TypeError("Modal id is required.");
    if (!options.element) throw new TypeError(`Modal element is required for ${key}.`);

    registrations.set(key, {
      id: key,
      element: options.element,
      initialFocus: options.initialFocus || null,
      onRequestClose: options.onRequestClose || null,
      isBusy: options.isBusy || (() => false),
      escapeEnabled: options.escapeEnabled !== false,
      onKeydown: options.onKeydown || null,
      restoreTarget: null,
    });

    return () => unregister(key);
  };

  const unregister = (id) => {
    const key = String(id || "");
    const index = openStack.lastIndexOf(key);
    if (index >= 0) openStack.splice(index, 1);
    syncDocumentModalState();
    return registrations.delete(key);
  };

  const open = (id, { restoreTarget = documentRef?.activeElement, focus = true } = {}) => {
    const key = String(id || "");
    const registration = registrations.get(key);
    if (!registration) return false;

    if (!openStack.includes(key)) {
      registration.restoreTarget = restoreTarget || null;
      openStack.push(key);
    } else if (activeId() !== key) {
      openStack.splice(openStack.indexOf(key), 1);
      openStack.push(key);
    }

    registration.element.classList?.remove("is-hidden");
    registration.element.hidden = false;
    registration.element.setAttribute?.("aria-hidden", "false");
    syncDocumentModalState();

    if (focus) {
      scheduleFocus(() => {
        if (activeId() === key) focusRegistration(registration);
      });
    }

    return true;
  };

  const close = (id, { restoreFocus = true } = {}) => {
    const key = String(id || "");
    const registration = registrations.get(key);
    if (!registration) return false;

    registration.element.classList?.add("is-hidden");
    registration.element.hidden = true;
    registration.element.setAttribute?.("aria-hidden", "true");

    const index = openStack.lastIndexOf(key);
    const wasActive = index === openStack.length - 1;
    if (index >= 0) openStack.splice(index, 1);
    syncDocumentModalState();

    const restoreTarget = registration.restoreTarget;
    registration.restoreTarget = null;

    if (wasActive && restoreFocus) {
      scheduleFocus(() => {
        const nextRegistration = activeRegistration();
        if (nextRegistration) {
          const restoreInsideNextDialog =
            restoreTarget &&
            (nextRegistration.element === restoreTarget ||
              nextRegistration.element.contains?.(restoreTarget));
          if (restoreInsideNextDialog && focusElement(restoreTarget)) return;
          focusRegistration(nextRegistration);
          return;
        }
        focusElement(restoreTarget);
      });
    }

    return true;
  };

  const handleKeydown = (event) => {
    const registration = activeRegistration();
    if (!registration || event.defaultPrevented) return;

    if (event.key === "Escape") {
      let busy = false;
      try {
        busy = Boolean(resolveValue(registration.isBusy, false));
      } catch (error) {
        reportError(error);
        busy = true;
      }

      event.preventDefault();
      event.stopPropagation();
      if (!modalCanDismiss({ busy, escapeEnabled: registration.escapeEnabled })) return;

      try {
        registration.onRequestClose?.();
      } catch (error) {
        reportError(error);
      }
      return;
    }

    if (event.key === "Tab") {
      const focusables = focusableModalElements(registration.element);
      const currentIndex = focusables.indexOf(documentRef?.activeElement);
      const nextIndex = nextModalFocusIndex(currentIndex, focusables.length, event.shiftKey);

      event.preventDefault();
      event.stopPropagation();
      if (nextIndex >= 0) {
        focusElement(focusables[nextIndex]);
      } else {
        focusElement(registration.element);
      }
      return;
    }

    try {
      registration.onKeydown?.(event);
    } catch (error) {
      reportError(error);
    }
  };

  const attach = () => {
    if (attached || !documentRef?.addEventListener) return;
    documentRef.addEventListener("keydown", handleKeydown, true);
    attached = true;
  };

  const detach = () => {
    if (!attached || !documentRef?.removeEventListener) return;
    documentRef.removeEventListener("keydown", handleKeydown, true);
    attached = false;
  };

  const dispose = () => {
    detach();
    openStack.length = 0;
    syncDocumentModalState();
    registrations.clear();
  };

  return {
    activeId,
    attach,
    close,
    detach,
    dispose,
    hasActiveModal: () => Boolean(activeId()),
    isOpen: (id) => openStack.includes(String(id || "")),
    open,
    register,
    unregister,
  };
}
