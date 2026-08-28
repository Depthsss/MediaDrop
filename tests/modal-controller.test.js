import test from "node:test";
import assert from "node:assert/strict";

import {
  createModalController,
  modalCanDismiss,
  nextModalFocusIndex,
} from "../src/features/accessibility/modal-controller.js";

function fakeElement({ focusable = false } = {}) {
  const attributes = new Map();
  const classes = new Set(["is-hidden"]);
  const listeners = new Map();
  const element = {
    hidden: true,
    disabled: false,
    isConnected: true,
    classList: {
      add: (name) => classes.add(name),
      remove: (name) => classes.delete(name),
      contains: (name) => classes.has(name),
    },
    focus() {
      element.focusCount += 1;
    },
    focusCount: 0,
    getAttribute: (name) => attributes.get(name) ?? null,
    setAttribute: (name, value) => attributes.set(name, String(value)),
    querySelectorAll: () => (focusable ? [element] : []),
    addEventListener(type, listener) {
      listeners.set(type, listener);
    },
    removeEventListener(type, listener) {
      if (listeners.get(type) === listener) listeners.delete(type);
    },
  };
  return element;
}

function fakeDocument(activeElement = null) {
  const listeners = new Map();
  const bodyClasses = new Set();
  return {
    activeElement,
    body: {
      classList: {
        toggle(name, enabled) {
          if (enabled) bodyClasses.add(name);
          else bodyClasses.delete(name);
        },
        contains: (name) => bodyClasses.has(name),
      },
    },
    addEventListener(type, listener) {
      listeners.set(type, listener);
    },
    removeEventListener(type, listener) {
      if (listeners.get(type) === listener) listeners.delete(type);
    },
    dispatchKey(key, options = {}) {
      const event = {
        key,
        shiftKey: Boolean(options.shiftKey),
        defaultPrevented: false,
        preventDefault() {
          this.defaultPrevented = true;
        },
        stopPropagation() {},
      };
      listeners.get("keydown")?.(event);
      return event;
    },
    listenerCount: () => listeners.size,
  };
}

function trackedElement(documentRef, { disabled = false, focusMoves = true, children = [] } = {}) {
  const attributes = new Map();
  const element = {
    disabled,
    hidden: false,
    isConnected: true,
    ownerDocument: documentRef,
    children,
    classList: { add() {}, remove() {}, contains: () => false },
    focusCount: 0,
    focus() {
      element.focusCount += 1;
      if (focusMoves) documentRef.activeElement = element;
    },
    getAttribute: (name) => attributes.get(name) ?? null,
    setAttribute: (name, value) => attributes.set(name, String(value)),
    querySelectorAll: () => element.children,
    contains: (target) => target === element || element.children.includes(target),
  };
  return element;
}

test("modal focus index wraps in both Tab directions", () => {
  assert.equal(nextModalFocusIndex(2, 3), 0);
  assert.equal(nextModalFocusIndex(0, 3, true), 2);
  assert.equal(nextModalFocusIndex(-1, 3, true), 2);
  assert.equal(nextModalFocusIndex(0, 0), -1);
});

test("modal dismiss policy blocks busy and explicitly non-dismissible dialogs", () => {
  assert.equal(modalCanDismiss({}), true);
  assert.equal(modalCanDismiss({ busy: true }), false);
  assert.equal(modalCanDismiss({ escapeEnabled: false }), false);
});

test("opening and closing toggles the native hidden state and restores the opener", () => {
  const opener = fakeElement({ focusable: true });
  opener.hidden = false;
  const documentRef = fakeDocument(opener);
  const modal = fakeElement();
  const controller = createModalController({ documentRef, scheduleFocus: (callback) => callback() });
  controller.register("quality", { element: modal });

  controller.open("quality");
  assert.equal(modal.hidden, false);
  assert.equal(modal.getAttribute("aria-hidden"), "false");
  assert.equal(modal.focusCount, 1);

  controller.close("quality");
  assert.equal(modal.hidden, true);
  assert.equal(opener.focusCount, 1);
});

test("only the topmost dialog receives Escape and restores underlying focus", () => {
  const documentRef = fakeDocument();
  const first = fakeElement();
  const second = fakeElement();
  const closeCalls = [];
  const controller = createModalController({ documentRef, scheduleFocus: (callback) => callback() });
  controller.register("history", { element: first, onRequestClose: () => closeCalls.push("history") });
  controller.register("cookie", { element: second, onRequestClose: () => closeCalls.push("cookie") });
  controller.attach();
  controller.open("history", { focus: false });
  controller.open("cookie", { focus: false });

  documentRef.dispatchKey("Escape");
  assert.deepEqual(closeCalls, ["cookie"]);
  controller.close("cookie");
  assert.equal(first.focusCount, 1);
  controller.detach();
});

test("document stays dimmed until the last stacked dialog closes", () => {
  const documentRef = fakeDocument();
  const controller = createModalController({ documentRef });
  controller.register("history", { element: fakeElement() });
  controller.register("cookie", { element: fakeElement() });

  controller.open("history", { focus: false });
  controller.open("cookie", { focus: false });
  controller.close("cookie", { restoreFocus: false });
  assert.equal(documentRef.body.classList.contains("has-modal-open"), true);

  controller.close("history", { restoreFocus: false });
  assert.equal(documentRef.body.classList.contains("has-modal-open"), false);
});

test("Escape does not dismiss a busy dialog", () => {
  const documentRef = fakeDocument();
  const modal = fakeElement();
  let closeCalls = 0;
  const controller = createModalController({ documentRef });
  controller.register("tools", {
    element: modal,
    isBusy: () => true,
    onRequestClose: () => {
      closeCalls += 1;
    },
  });
  controller.attach();
  controller.open("tools", { focus: false });

  assert.equal(documentRef.dispatchKey("Escape").defaultPrevented, true);
  assert.equal(closeCalls, 0);
  controller.detach();
});

test("attach, repeated open-close, and dispose do not leak document listeners", () => {
  const documentRef = fakeDocument();
  const modal = fakeElement();
  const controller = createModalController({ documentRef });
  controller.register("history", { element: modal });
  controller.attach();
  controller.attach();
  controller.open("history", { focus: false });
  controller.close("history", { restoreFocus: false });
  controller.open("history", { focus: false });
  controller.close("history", { restoreFocus: false });
  assert.equal(documentRef.listenerCount(), 1);
  controller.dispose();
  assert.equal(documentRef.listenerCount(), 0);
});

test("disabled or unfocusable initial targets fall back to the dialog container", () => {
  const documentRef = fakeDocument();
  const disabled = trackedElement(documentRef, { disabled: true });
  const stalled = trackedElement(documentRef, { focusMoves: false });
  const modal = trackedElement(documentRef);
  const controller = createModalController({ documentRef, scheduleFocus: (callback) => callback() });
  controller.register("restart", { element: modal, initialFocus: disabled });

  controller.open("restart");
  assert.equal(disabled.focusCount, 0);
  assert.equal(documentRef.activeElement, modal);

  controller.close("restart", { restoreFocus: false });
  controller.unregister("restart");
  controller.register("restart", { element: modal, initialFocus: stalled });
  controller.open("restart");
  assert.equal(stalled.focusCount, 1);
  assert.equal(documentRef.activeElement, modal);
});

test("closing a stacked dialog restores its recorded opener inside the underlying dialog", () => {
  const documentRef = fakeDocument();
  const opener = trackedElement(documentRef);
  const underlyingInitial = trackedElement(documentRef);
  const underlying = trackedElement(documentRef, { children: [opener, underlyingInitial] });
  const topmost = trackedElement(documentRef);
  const controller = createModalController({ documentRef, scheduleFocus: (callback) => callback() });
  controller.register("history", { element: underlying, initialFocus: underlyingInitial });
  controller.register("cookie", { element: topmost });
  controller.open("history", { focus: false });
  documentRef.activeElement = opener;
  controller.open("cookie", { focus: false });

  controller.close("cookie");
  assert.equal(documentRef.activeElement, opener);
  assert.equal(underlyingInitial.focusCount, 0);
});

test("Tab and Shift+Tab wrap only within the topmost dialog", () => {
  const documentRef = fakeDocument();
  const firstA = trackedElement(documentRef);
  const firstB = trackedElement(documentRef);
  const secondA = trackedElement(documentRef);
  const secondB = trackedElement(documentRef);
  const first = trackedElement(documentRef, { children: [firstA, firstB] });
  const second = trackedElement(documentRef, { children: [secondA, secondB] });
  const controller = createModalController({ documentRef, scheduleFocus: (callback) => callback() });
  controller.register("history", { element: first });
  controller.register("cookie", { element: second });
  controller.attach();
  controller.open("history", { focus: false });
  controller.open("cookie", { focus: false });

  documentRef.activeElement = secondB;
  documentRef.dispatchKey("Tab");
  assert.equal(documentRef.activeElement, secondA);
  documentRef.dispatchKey("Tab", { shiftKey: true });
  assert.equal(documentRef.activeElement, secondB);
  assert.equal(firstA.focusCount + firstB.focusCount, 0);
  controller.detach();
});
