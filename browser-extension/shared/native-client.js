const PROTOCOL_VERSION = 1;
const REQUEST_TIMEOUT_MS = 45_000;

function bridgeError(code, message = code, details = {}) {
  return Object.assign(new Error(message), details, { code });
}

export class NativeClient {
  constructor(runtime, hostName, randomUuid = () => crypto.randomUUID(), onStatus = () => {}) {
    this.runtime = runtime;
    this.hostName = hostName;
    this.randomUuid = randomUuid;
    this.onStatus = onStatus;
    this.port = null;
    this.hello = null;
    this.pending = new Map();
  }

  async connect() {
    if (this.hello) return this.hello;
    const extensionVersion = this.runtime.getManifest?.().version
      || globalThis.chrome?.runtime?.getManifest?.().version
      || "0.0.0";
    const port = this.runtime.connectNative(this.hostName);
    this.port = port;
    port.onMessage.addListener((message) => this.#onMessage(port, message));
    port.onDisconnect.addListener(() => this.#onDisconnect(port));
    this.hello = this.#send(
      "hello",
      {
        extensionVersion,
        supportedProtocol: { min: PROTOCOL_VERSION, max: PROTOCOL_VERSION },
      },
      this.randomUuid(),
    )
      .then((response) => {
        if (response.status !== "accepted" || response.payload?.selectedProtocol !== PROTOCOL_VERSION) {
          throw bridgeError(response.error?.code || "version_mismatch", response.error?.message, {
            action: response.error?.action,
            expectedExtensionVersion: response.payload?.expectedExtensionVersion,
          });
        }
        if (response.payload?.appVersion !== extensionVersion) {
          throw bridgeError(
            "version_mismatch",
            "MediaDrop uygulaması ve tarayıcı eklentisi sürümleri uyuşmuyor.",
            { action: "update_app_or_extension", expectedExtensionVersion: extensionVersion },
          );
        }
        return response;
      })
      .catch((error) => {
        this.close();
        throw error;
      });
    return this.hello;
  }

  async call(command, payload = {}, requestId = this.randomUuid()) {
    await this.connect();
    return this.#send(command, payload, requestId);
  }

  close() {
    this.port?.disconnect();
    this.port = null;
    this.hello = null;
  }

  #send(command, payload, requestId) {
    if (!this.port) return Promise.reject(bridgeError("pipe_disconnected"));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(bridgeError("native_timeout"));
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(requestId, { resolve, reject, timer });
      this.port.postMessage({
        messageType: "request",
        protocolVersion: PROTOCOL_VERSION,
        requestId,
        command,
        payload,
      });
    });
  }

  #onMessage(port, message) {
    if (port !== this.port) return;
    if (message?.messageType === "event") {
      this.onStatus(message);
      return;
    }
    const pending = this.pending.get(message?.requestId);
    if (!pending) return;
    this.pending.delete(message.requestId);
    clearTimeout(pending.timer);
    pending.resolve(message);
  }

  #onDisconnect(port) {
    if (port !== this.port) return;
    const message = this.runtime.lastError?.message || "MediaDrop native host bağlantısı koptu.";
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(bridgeError("native_host_disconnected", message));
    }
    this.pending.clear();
    this.port = null;
    this.hello = null;
  }
}
