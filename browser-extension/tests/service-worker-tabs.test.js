import test from "node:test";
import assert from "node:assert/strict";

function extensionEvent() {
  const listeners = [];
  return {
    listeners,
    addListener(listener) {
      listeners.push(listener);
    },
  };
}

test("analysis badges stay scoped to the analyzed tab and its current link", async () => {
  const tabs = new Map([
    [11, { id: 11, url: "https://www.youtube.com/watch?v=first" }],
    [22, { id: 22, url: "https://x.com/example/status/22" }],
    [33, { id: 33, url: "https://example.com/" }],
  ]);
  const perTabBadges = new Map();
  const analysisRequestsByUrl = new Map();
  let globalBadge = "";
  let activeTabId = 11;
  const contextClicked = extensionEvent();
  const navigationCommitted = extensionEvent();
  const historyUpdated = extensionEvent();
  const tabActivated = extensionEvent();
  const tabRemoved = extensionEvent();
  const nativeMessages = extensionEvent();
  const nativeDisconnect = extensionEvent();
  const runtimeMessages = extensionEvent();
  const previousChrome = globalThis.chrome;
  const previousSetTimeout = globalThis.setTimeout;
  globalThis.setTimeout = (callback, delay, ...args) => {
    const timer = previousSetTimeout(callback, delay, ...args);
    timer.unref?.();
    return timer;
  };

  const responseFor = (message) => ({
    messageType: "response",
    protocolVersion: 1,
    requestId: message.requestId,
    command: message.command,
    status: message.command === "hello" ? "accepted" : "ready",
    stateRevision: 1,
    payload: message.command === "hello"
      ? { selectedProtocol: 1 }
      : { analysisRequestId: message.requestId },
    capabilities: {},
    error: null,
  });
  const nativeCommands = [];
  const port = {
    onMessage: nativeMessages,
    onDisconnect: nativeDisconnect,
    postMessage(message) {
      nativeCommands.push(message.command);
      if (message.command === "analyze_source") {
        analysisRequestsByUrl.set(message.payload.pageUrl, message.requestId);
      }
      queueMicrotask(() => {
        for (const listener of nativeMessages.listeners) listener(responseFor(message));
      });
    },
    disconnect() {},
  };

  globalThis.chrome = {
    runtime: {
      id: "service-worker-tab-test",
      lastError: null,
      getManifest: () => ({ version: "1.0.0" }),
      getURL: (path) => `chrome-extension://service-worker-tab-test/${path}`,
      connectNative: () => port,
      sendMessage: (_message, callback) => callback?.(),
      onInstalled: extensionEvent(),
      onStartup: extensionEvent(),
      onMessage: runtimeMessages,
    },
    action: {
      async setBadgeBackgroundColor() {},
      async setBadgeText(details) {
        if (Number.isInteger(details.tabId)) perTabBadges.set(details.tabId, details.text);
        else globalBadge = details.text;
      },
      async enable() {},
      async disable() {},
      async setTitle() {},
      async setIcon() {},
      async openPopup() {},
    },
    contextMenus: {
      create: (_details, callback) => callback?.(),
      removeAll: (callback) => callback?.(),
      onClicked: contextClicked,
    },
    tabs: {
      query: async (query) => query?.active ? [tabs.get(activeTabId)] : [...tabs.values()],
      onActivated: tabActivated,
      onRemoved: tabRemoved,
    },
    webNavigation: {
      getFrame: async ({ tabId }) => ({ url: tabs.get(tabId)?.url || "" }),
      onCommitted: navigationCommitted,
      onHistoryStateUpdated: historyUpdated,
    },
    scripting: {
      executeScript: async () => [],
    },
  };

  const flush = async () => {
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
  };
  const clickContextMenu = async (tabId) => {
    const tab = tabs.get(tabId);
    for (const listener of contextClicked.listeners) {
      listener({ menuItemId: "mediadrop-download", pageUrl: tab.url, mediaType: "video" }, tab);
    }
    await flush();
  };
  const visibleBadge = (tabId) => perTabBadges.has(tabId) ? perTabBadges.get(tabId) : globalBadge;
  const sendRuntimeMessage = (message) => new Promise((resolve) => {
    runtimeMessages.listeners[0](message, { id: "service-worker-tab-test" }, resolve);
  });

  try {
    await import(`../service-worker.js?tab-state-test=${Date.now()}`);
    await flush();

    for (const listener of globalThis.chrome.runtime.onInstalled.listeners) {
      listener({ reason: "install" });
    }
    await flush();
    assert.deepEqual(nativeCommands.slice(0, 2), ["hello", "get_state"]);

    await clickContextMenu(11);
    assert.equal(visibleBadge(11), "1");
    assert.equal(visibleBadge(22), "");

    await clickContextMenu(22);
    assert.equal(visibleBadge(11), "1");
    assert.equal(visibleBadge(22), "1");

    activeTabId = 22;
    await sendRuntimeMessage({
      type: "clear_badge",
      analysisRequestId: analysisRequestsByUrl.get("https://www.youtube.com/watch?v=first"),
    });
    assert.equal(visibleBadge(11), "");
    assert.equal(visibleBadge(22), "1");

    await clickContextMenu(11);
    tabs.get(11).url = "https://www.youtube.com/watch?v=replaced";
    for (const listener of historyUpdated.listeners) {
      listener({ tabId: 11, frameId: 0, url: tabs.get(11).url });
    }
    await flush();
    assert.equal(visibleBadge(11), "");
    assert.equal(visibleBadge(22), "1");

    tabs.delete(22);
    perTabBadges.delete(22);
    for (const listener of tabRemoved.listeners) listener(22, { isWindowClosing: false, windowId: 1 });
    activeTabId = 33;
    assert.equal(visibleBadge(33), "");
  } finally {
    globalThis.chrome = previousChrome;
    globalThis.setTimeout = previousSetTimeout;
  }
});
