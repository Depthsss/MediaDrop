import test from "node:test";
import assert from "node:assert/strict";

import { createModalController } from "../src/features/accessibility/modal-controller.js";
import { createInstagramAuthController } from "../src/features/instagram/auth-controller.js";

function createFakeDocument() {
  const listeners = new Map();
  const documentRef = {
    activeElement: null,
    addEventListener(type, listener) {
      const handlers = listeners.get(type) || new Set();
      handlers.add(listener);
      listeners.set(type, handlers);
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
    createElement() {
      return createFakeElement(documentRef);
    },
    dispatchKey(key) {
      const event = { key, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, stopPropagation() {} };
      listeners.get("keydown")?.forEach((listener) => listener(event));
      return event;
    },
    listenerCount(type) {
      return listeners.get(type)?.size || 0;
    },
  };
  return documentRef;
}

function createFakeElement(documentRef) {
  const attributes = new Map();
  const listeners = new Map();
  const classes = new Set();
  const element = {
    ownerDocument: documentRef,
    disabled: false,
    hidden: false,
    isConnected: true,
    checked: true,
    textContent: "",
    dataset: {},
    children: [],
    parent: null,
    className: "",
    classList: {
      add: (name) => classes.add(name),
      remove: (name) => classes.delete(name),
      toggle: (name, enabled) => (enabled ? classes.add(name) : classes.delete(name)),
      contains: (name) => classes.has(name),
    },
    focus() {
      documentRef.activeElement = element;
    },
    setAttribute(name, value) {
      attributes.set(name, String(value));
    },
    getAttribute(name) {
      return attributes.get(name) ?? null;
    },
    append(...nodes) {
      nodes.forEach((node) => element.appendChild(node));
    },
    appendChild(node) {
      node.parent = element;
      element.children.push(node);
    },
    replaceChildren() {
      element.children = [];
    },
    querySelectorAll(selector) {
      if (selector === ".cookie-browser-option") {
        return element.children.filter((child) => child.className === "cookie-browser-option");
      }
      return element.children;
    },
    contains(target) {
      return target === element || element.children.some((child) => child.contains?.(target) || child === target);
    },
    closest(selector) {
      let current = element.parent;
      while (current) {
        if (selector.includes("aria-hidden") && current.getAttribute("aria-hidden") === "true") return current;
        if (selector.includes(".is-hidden") && current.classList.contains("is-hidden")) return current;
        current = current.parent;
      }
      return null;
    },
    addEventListener(type, listener) {
      const handlers = listeners.get(type) || new Set();
      handlers.add(listener);
      listeners.set(type, handlers);
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
    listenerCount(type) {
      return listeners.get(type)?.size || 0;
    },
  };
  return element;
}

function setupAuthDialog() {
  const documentRef = createFakeDocument();
  const elements = {
    cookieAuthOverlay: createFakeElement(documentRef),
    cookieAuthStatus: createFakeElement(documentRef),
    cookieBrowserSelect: createFakeElement(documentRef),
    cookieBrowserSelectedName: createFakeElement(documentRef),
    cookieBrowserSelectedDetail: createFakeElement(documentRef),
    cookieBrowserList: createFakeElement(documentRef),
    cookieRememberCheck: createFakeElement(documentRef),
    cookieAllowBtn: createFakeElement(documentRef),
    cookieDenyBtn: createFakeElement(documentRef),
    browserRestartOverlay: createFakeElement(documentRef),
    browserRestartTitle: createFakeElement(documentRef),
    browserRestartStatus: createFakeElement(documentRef),
    browserRestartAllowBtn: createFakeElement(documentRef),
    browserRestartDenyBtn: createFakeElement(documentRef),
  };
  elements.cookieAuthOverlay.children = [elements.cookieBrowserSelect, elements.cookieBrowserList, elements.cookieDenyBtn, elements.cookieAllowBtn];
  elements.cookieAuthOverlay.children.forEach((child) => { child.parent = elements.cookieAuthOverlay; });
  elements.browserRestartOverlay.children = [elements.browserRestartDenyBtn, elements.browserRestartAllowBtn];
  elements.browserRestartOverlay.children.forEach((child) => { child.parent = elements.browserRestartOverlay; });

  const modalController = createModalController({ documentRef, scheduleFocus: (callback) => callback() });
  const auth = createInstagramAuthController({
    invoke: async (command) => (command === "list_cookie_browsers" ? [{ id: "chrome", installed: true }] : null),
    documentRef,
    elements,
    modalController,
  });
  modalController.register("instagram-cookie-permission", {
    element: elements.cookieAuthOverlay,
    initialFocus: elements.cookieDenyBtn,
    onRequestClose: () => auth.cancelActiveDialog(),
  });
  modalController.register("instagram-browser-restart", {
    element: elements.browserRestartOverlay,
    initialFocus: elements.browserRestartDenyBtn,
    onRequestClose: () => auth.cancelActiveDialog(),
  });
  modalController.attach();
  return { auth, documentRef, elements, modalController };
}

test("Escape through the real controller resolves cookie and restart Promises without listener leaks", async () => {
  const { auth, documentRef, elements, modalController } = setupAuthDialog();

  for (let attempt = 0; attempt < 2; attempt += 1) {
    const permission = auth.requestInstagramCookiePermission();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(elements.cookieBrowserList.getAttribute("aria-hidden"), "true");
    assert.equal(documentRef.dispatchKey("Escape").defaultPrevented, true);
    assert.deepEqual(await permission, { allowed: false, browserId: "", remember: false });
    assert.equal(elements.cookieAllowBtn.listenerCount("click"), 0);
    assert.equal(elements.cookieDenyBtn.listenerCount("click"), 0);
    assert.equal(documentRef.listenerCount("click"), 0);
  }

  const confirmation = auth.requestBrowserRestartConfirmation();
  assert.equal(documentRef.dispatchKey("Escape").defaultPrevented, true);
  assert.equal(await confirmation, false);
  assert.equal(elements.browserRestartAllowBtn.listenerCount("click"), 0);
  assert.equal(elements.browserRestartDenyBtn.listenerCount("click"), 0);
  assert.equal(documentRef.listenerCount("click"), 0);
  modalController.dispose();
  assert.equal(documentRef.listenerCount("keydown"), 0);
});
