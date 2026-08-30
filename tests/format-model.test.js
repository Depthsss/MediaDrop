import test from "node:test";
import assert from "node:assert/strict";

import {
  buildFormatCards,
  codecPriority,
  qualityHeightFromLabel,
  qualityLabelFromHeight,
  SOCIAL_COMPATIBLE_FORMAT_ID,
} from "../src/features/quality/format-model.js";

test("codec preference keeps Windows-friendly AVC ahead of modern fallbacks", () => {
  assert.ok(codecPriority({ vcodec: "avc1.640028" }) < codecPriority({ vcodec: "av01.0.08M" }));
  assert.ok(codecPriority({ vcodec: "vp9" }) < codecPriority({ vcodec: "hvc1.1.6" }));
});

test("YouTube cards select one best codec per height and retain audio-only choice", () => {
  const cards = buildFormatCards(
    {
      formats: [
        { format_id: "vp9", height: 1080, vcodec: "vp9", acodec: "none", protocol: "https" },
        { format_id: "avc", height: 1080, vcodec: "avc1", acodec: "none", protocol: "https" },
        { format_id: "audio", vcodec: "none", acodec: "mp4a", ext: "m4a", abr: 128 },
      ],
    },
    "youtube",
  );

  assert.deepEqual(cards.map((card) => card.id), ["avc", "audio"]);
  assert.equal(cards[0].autoSelect, true);
});

test("YouTube HLS video remains selectable so clip controls stay available", () => {
  const cards = buildFormatCards(
    {
      formats: [
        {
          format_id: "hls-720",
          height: 720,
          vcodec: "avc1.4d401f",
          acodec: "mp4a.40.2",
          protocol: "m3u8_native",
          ext: "mp4",
        },
        { format_id: "audio", vcodec: "none", acodec: "mp4a", ext: "m4a", abr: 128 },
      ],
    },
    "youtube",
  );

  assert.equal(cards[0].id, "hls-720");
  assert.equal(cards[0].type, "video");
  assert.equal(cards[0].autoSelect, true);
});

test("YouTube DASH video remains the auto-selected quality instead of falling back to MP3", () => {
  const cards = buildFormatCards(
    {
      formats: [
        {
          format_id: "137",
          height: 1080,
          vcodec: "avc1.640028",
          acodec: "none",
          protocol: "http_dash_segments",
          ext: "mp4",
        },
        { format_id: "140", vcodec: "none", acodec: "mp4a.40.2", ext: "m4a", abr: 128 },
      ],
    },
    "youtube",
  );

  assert.equal(cards[0].type, "video");
  assert.equal(cards[0].id, "137");
  assert.equal(cards[0].autoSelect, true);
});

test("large file estimates never override the preferred codec at the same height", () => {
  const cards = buildFormatCards(
    {
      formats: [
        {
          format_id: "av1-large",
          height: 2160,
          vcodec: "av01.0.12M",
          acodec: "none",
          protocol: "https",
          filesize: 5 * 1024 ** 3,
        },
        {
          format_id: "avc-compatible",
          height: 2160,
          vcodec: "avc1.640033",
          acodec: "none",
          protocol: "https",
          filesize: 500 * 1024 ** 2,
        },
      ],
    },
    "youtube",
  );

  assert.equal(cards[0].id, "avc-compatible");
});

test("social format remains a single deterministic compatible card", () => {
  const cards = buildFormatCards({ formats: [] }, "instagram");
  assert.equal(cards.length, 1);
  assert.equal(cards[0].id, SOCIAL_COMPATIBLE_FORMAT_ID);
  assert.equal(qualityLabelFromHeight(2160), "4K");
  assert.equal(qualityHeightFromLabel("2K"), 1440);
});
