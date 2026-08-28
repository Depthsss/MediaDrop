const DOWNLOAD_STATES = new Set(["idle", "downloading", "pausing", "paused", "canceling"]);
const WINDOW_MODES = new Set(["main", "media", "clip"]);

export function createInitialAppState() {
  return {
    analysis: {
      status: "idle",
      token: 0,
      url: "",
      platform: "generic",
      info: null,
      mediaAnalysis: null,
      items: [],
      index: 0,
      error: null,
    },
    auth: {
      mode: null,
      status: "idle",
    },
    preview: {
      token: 0,
      status: "idle",
      itemId: null,
    },
    download: {
      status: "idle",
      jobId: "",
      lastArgs: null,
      lastMediaArgs: null,
    },
    clip: {
      status: "idle",
      selection: null,
    },
    window: {
      mode: "main",
    },
  };
}

function boundedIndex(items, value) {
  if (!items.length) return 0;
  const index = Number(value);
  if (!Number.isFinite(index)) return 0;
  return Math.max(0, Math.min(items.length - 1, Math.trunc(index)));
}

export function appReducer(state, event) {
  switch (event?.type) {
    case "analysis/started":
      return {
        ...state,
        analysis: {
          ...state.analysis,
          status: "loading",
          token: Number(event.token || state.analysis.token + 1),
          url: String(event.url || ""),
          platform: "generic",
          info: null,
          mediaAnalysis: null,
          items: [],
          index: 0,
          error: null,
        },
      };
    case "analysis/succeeded": {
      const items = Array.isArray(event.items) ? [...event.items] : [];
      return {
        ...state,
        analysis: {
          ...state.analysis,
          status: "ready",
          platform: String(event.platform || "generic"),
          info: event.info || null,
          mediaAnalysis: event.mediaAnalysis || null,
          items,
          index: boundedIndex(items, event.index),
          error: null,
        },
      };
    }
    case "analysis/failed":
      return {
        ...state,
        analysis: {
          ...state.analysis,
          status: "error",
          error: event.error || null,
        },
      };
    case "analysis/reset":
      return {
        ...state,
        analysis: {
          ...createInitialAppState().analysis,
          token: state.analysis.token,
        },
        preview: createInitialAppState().preview,
      };
    case "preview/selected":
      return {
        ...state,
        analysis: {
          ...state.analysis,
          index: boundedIndex(state.analysis.items, event.index),
        },
        preview: {
          ...state.preview,
          status: "ready",
          itemId: event.itemId || null,
        },
      };
    case "auth/changed":
      return {
        ...state,
        auth: {
          mode: event.mode || null,
          status: event.status || "idle",
        },
      };
    case "download/status":
      return {
        ...state,
        download: {
          ...state.download,
          status: DOWNLOAD_STATES.has(event.status) ? event.status : state.download.status,
          jobId:
            event.status === "idle" ||
            (event.status === "downloading" && state.download.status === "idle")
              ? ""
              : state.download.jobId,
        },
      };
    case "download/job":
      return {
        ...state,
        download: {
          ...state.download,
          jobId: String(event.jobId || ""),
        },
      };
    case "download/arguments":
      return {
        ...state,
        download: {
          ...state.download,
          lastArgs: event.args || null,
          lastMediaArgs: event.mediaArgs || null,
        },
      };
    case "clip/changed":
      return {
        ...state,
        clip: {
          status: event.status || "idle",
          selection: event.selection || null,
        },
      };
    case "window/mode":
      return {
        ...state,
        window: {
          mode: WINDOW_MODES.has(event.mode) ? event.mode : state.window.mode,
        },
      };
    default:
      return state;
  }
}

export function createAppStore(initialState = createInitialAppState()) {
  let state = initialState;
  const subscribers = new Set();
  return {
    getState() {
      return state;
    },
    dispatch(event) {
      const next = appReducer(state, event);
      if (next !== state) {
        state = next;
        for (const subscriber of subscribers) subscriber(state, event);
      }
      return state;
    },
    subscribe(subscriber) {
      subscribers.add(subscriber);
      return () => subscribers.delete(subscriber);
    },
  };
}
