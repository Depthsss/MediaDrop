import test from "node:test";
import assert from "node:assert/strict";

import { NativeClient } from "../shared/native-client.js";

function eventHook() {
  const listeners = [];
  return {
    addListener(listener) {
      listeners.push(listener);
    },
    emit(value) {
      for (const listener of listeners) listener(value);
    },
  };
}

function fakeRuntime() {
  const ports = [];
  return {
    ports,
    connectNative(name) {
      const onMessage = eventHook();
      const onDisconnect = eventHook();
      const port = {
        name,
        onMessage,
        onDisconnect,
        sent: [],
        postMessage(message) {
          this.sent.push(message);
          queueMicrotask(() =>
            onMessage.emit({
              messageType: "response",
              protocolVersion: 1,
              requestId: message.requestId,
              command: message.command,
              status: "accepted",
              payload: message.command === "hello" ? { selectedProtocol: 1 } : {},
              capabilities: {},
              error: null,
            }),
          );
        },
        disconnect() {
          onDisconnect.emit();
        },
      };
      ports.push(port);
      return port;
    },
  };
}

test("native client negotiates hello before the first command and never sends browser origin", async () => {
  const runtime = fakeRuntime();
  let sequence = 0;
  const client = new NativeClient(runtime, "com.mab.mediadrop", () => `00000000-0000-4000-8000-${String(++sequence).padStart(12, "0")}`);
  await client.call(
    "analyze_source",
    { pageUrl: "https://www.youtube.com/watch?v=one" },
    "11111111-1111-4111-8111-111111111111",
  );

  assert.equal(runtime.ports.length, 1);
  assert.deepEqual(runtime.ports[0].sent.map((message) => message.command), ["hello", "analyze_source"]);
  assert.equal(runtime.ports[0].sent[1].requestId, "11111111-1111-4111-8111-111111111111");
  assert.equal("clientOrigin" in runtime.ports[0].sent[1], false);
});

test("disconnect drops pending state and the next call performs a fresh handshake", async () => {
  const runtime = fakeRuntime();
  let sequence = 0;
  const client = new NativeClient(runtime, "com.mab.mediadrop", () => `00000000-0000-4000-8000-${String(++sequence).padStart(12, "0")}`);
  await client.call("get_state", {});
  runtime.ports[0].disconnect();
  await client.call("get_state", {});

  assert.equal(runtime.ports.length, 2);
  assert.deepEqual(runtime.ports[1].sent.map((message) => message.command), ["hello", "get_state"]);
});
