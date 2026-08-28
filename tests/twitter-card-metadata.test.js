import test from "node:test";
import assert from "node:assert/strict";

import {
  formatTwitterCompactCount,
  isTwitterPostDownloadIntent,
  normalizeTwitterHandle,
  normalizeTwitterPostMetadata,
  normalizeTwitterQuoteContext,
  twitterHandleFromUrl,
  twitterPostCountMetadata,
  twitterProfileImageCandidate,
  twitterTextPostAvailable,
  twitterDateFromMetadata,
} from "../src/features/twitter-card/metadata.js";
import {
  twitterPostTemplateValues,
  wrapFullCanvasText,
} from "../src/features/twitter-card/renderer.js";
import * as twitterCardRenderer from "../src/features/twitter-card/renderer.js";

const ROOT_AVATAR = "https://pbs.twimg.com/profile_images/100/root_normal.jpg";
const OWNER_AVATAR = "https://pbs.twimg.com/profile_images/200/owner_normal.jpg";
const COMMENTER_AVATAR = "https://pbs.twimg.com/profile_images/300/commenter_normal.jpg";

test("naive X timestamps are interpreted as UTC before local display", () => {
  assert.equal(
    twitterDateFromMetadata("2026-08-24 21:03:46")?.toISOString(),
    "2026-08-24T21:03:46.000Z",
  );
  assert.equal(
    twitterDateFromMetadata("2026-08-24T21:03:46+00:00")?.toISOString(),
    "2026-08-24T21:03:46.000Z",
  );
  assert.equal(twitterDateFromMetadata("not-a-date"), null);
});

test("photo post template values do not depend on bootstrap globals", () => {
  const values = twitterPostTemplateValues({
    text: "Uzunluğu ne olursa olsun gönderi metni",
    authorName: "MediaDrop",
    quality: "Fotoğraf",
  });

  assert.equal(values.tweet_text, "Uzunluğu ne olursa olsun gönderi metni");
  assert.equal(values.quality_label, "Fotoğraf");
  assert.equal(values.duration_label, "");
});

test("video post template formats a positive duration without bootstrap globals", () => {
  const values = twitterPostTemplateValues({
    text: "Süreli video",
    authorName: "MediaDrop",
    duration: 42,
  });

  assert.equal(values.duration_label, "0:42");
});

test("companion handoff auto-download accepts only the Twitter post intent", () => {
  assert.equal(isTwitterPostDownloadIntent("download_twitter_post"), true);
  assert.equal(isTwitterPostDownloadIntent("download_video"), false);
  assert.equal(isTwitterPostDownloadIntent(null), false);
});

test("quote context requires a real outer-to-quoted identity relationship", () => {
  const quote = normalizeTwitterQuoteContext({
    outer: {
      id: "200",
      authorName: "Dış Yazar",
      authorHandle: "dis_yazar",
      text: "Dış tweet yorumu",
    },
    quoted: {
      id: "100",
      authorName: "Alıntı Yazarı",
      authorHandle: "@alinti_yazari",
      text: "Alıntılanan tweet",
    },
    quotedMediaIndexes: [1, "2", 2, -1, 99.5],
  });

  assert.equal(quote.outer.authorHandle, "@dis_yazar");
  assert.equal(quote.quoted.authorHandle, "@alinti_yazari");
  assert.equal(quote.quoted.text, "Alıntılanan tweet");
  assert.deepEqual(quote.quotedMediaIndexes, [1, 2]);
  assert.equal(normalizeTwitterQuoteContext({ outer: { id: "200" } }), null);
  assert.equal(
    normalizeTwitterQuoteContext({ outer: { id: "200" }, quoted: { id: "200" } }),
    null
  );
});

test("quote template values preserve both posts while ordinary values stay flat", () => {
  const ordinary = twitterPostTemplateValues({
    text: "Normal tweet",
    authorName: "Yazar",
  });
  assert.equal(ordinary.quoted_post, null);

  const quoted = twitterPostTemplateValues({
    text: "Dış tweet",
    authorName: "Dış",
    quotedPost: {
      id: "100",
      text: "Alıntılanan tweet",
      authorName: "Alıntı",
      authorHandle: "@alinti",
    },
  });
  assert.equal(quoted.tweet_text, "Dış tweet");
  assert.equal(quoted.quoted_post.tweet_text, "Alıntılanan tweet");
  assert.equal(quoted.quoted_post.display_name, "Alıntı");
});

test("downloaded Twitter cards wrap every post text line without truncation", () => {
  const context = { measureText: (value) => ({ width: String(value).length }) };
  const lines = wrapFullCanvasText(
    context,
    "aa bb cc dd ee ff gg hh ii jj kk ll mm nn",
    2
  );

  assert.deepEqual(lines, [
    "aa", "bb", "cc", "dd", "ee", "ff", "gg",
    "hh", "ii", "jj", "kk", "ll", "mm", "nn",
  ]);
  assert.deepEqual(
    wrapFullCanvasText(context, "abcdefgh", 3),
    ["abc", "def", "gh"]
  );
});

test("downloaded Twitter cards keep raw link text while previews stay cleaned", () => {
  const ordinary = normalizeTwitterPostMetadata({
    description: "Normal tweet https://example.com/full-path",
  });
  const quote = normalizeTwitterQuoteContext({
    outer: { id: "200", text: "Dış tweet https://example.com/outer" },
    quoted: { id: "100", text: "Alıntı https://example.com/quoted" },
  });
  const values = twitterPostTemplateValues({
    ...quote.outer,
    quotedPost: quote.quoted,
  });

  assert.equal(ordinary.text, "Normal tweet");
  assert.equal(
    twitterPostTemplateValues(ordinary).tweet_text,
    "Normal tweet https://example.com/full-path"
  );
  assert.equal(values.tweet_text, "Dış tweet https://example.com/outer");
  assert.equal(values.quoted_post.tweet_text, "Alıntı https://example.com/quoted");
});

test("text-only tweets remain downloadable when no media target exists", () => {
  assert.equal(twitterTextPostAvailable({ text: "Sadece metin içeren tweet" }, false), true);
  assert.equal(twitterTextPostAvailable({ text: "Sadece metin içeren tweet" }, true), false);
  assert.equal(twitterTextPostAvailable({ text: "" }, false), false);
  assert.equal(
    twitterTextPostAvailable({ text: "twitter video #2090384899064700955" }, false),
    false
  );
});

test("Twitter handle normalization accepts X URLs and rejects reserved paths", () => {
  assert.equal(normalizeTwitterHandle("@@media_drop/status/42"), "@media_drop");
  assert.equal(twitterHandleFromUrl("x.com/media_drop/status/42"), "@media_drop");
  assert.equal(twitterHandleFromUrl("https://twitter.com/i/web/status/42"), "");
  assert.equal(twitterHandleFromUrl("https://example.com/media_drop"), "");
});

test("root post avatar wins over owner and recursive commenter candidates", () => {
  const info = {
    profile_image_url_https: ROOT_AVATAR,
    author: { profile_image_url_https: OWNER_AVATAR },
    comments: [
      { user: { profile_image_url_https: COMMENTER_AVATAR } },
    ],
  };

  assert.equal(twitterProfileImageCandidate(info), ROOT_AVATAR);
});

test("known root owner is used and arbitrary comment branches are never avatar fallbacks", () => {
  assert.equal(
    twitterProfileImageCandidate({
      author: { profile_image_url_https: OWNER_AVATAR },
      comments: [{ user: { profile_image_url_https: COMMENTER_AVATAR } }],
    }),
    OWNER_AVATAR
  );

  assert.equal(
    twitterProfileImageCandidate({
      comments: [{ user: { profile_image_url_https: COMMENTER_AVATAR } }],
      thumbnail: "https://pbs.twimg.com/media/post-photo.jpg",
    }),
    ""
  );
});

test("post metadata keeps root counts, owner identity and compact normalization", () => {
  const info = {
    description: "Merhaba dünya https://t.co/example",
    uploader: "Media Drop",
    uploader_id: "media_drop",
    profile_image_url_https: ROOT_AVATAR,
    comment_count: "12",
    retweet_count: "1.2K",
    favorite_count: "2,5K",
    view_count: "1.5M",
    comments: [{ comment_count: 999 }],
    verified: "true",
  };

  const metadata = normalizeTwitterPostMetadata(info, "https://x.com/media_drop/status/42");

  assert.equal(metadata.text, "Merhaba dünya");
  assert.equal(metadata.authorName, "Media Drop");
  assert.equal(metadata.authorHandle, "@media_drop");
  assert.equal(metadata.avatarUrl, ROOT_AVATAR);
  assert.equal(metadata.replyCount, 12);
  assert.equal(metadata.retweetCount, 1200);
  assert.equal(metadata.likeCount, 2500);
  assert.equal(metadata.viewCount, 1_500_000);
  assert.equal(metadata.isVerified, true);
  assert.equal(formatTwitterCompactCount(metadata.viewCount), "1.5M");
  assert.deepEqual(twitterPostCountMetadata(info), {
    replyCount: 12,
    retweetCount: 1200,
    likeCount: 2500,
    viewCount: 1_500_000,
  });
});

test("downloaded Twitter cards preserve the exact view count", () => {
  assert.equal(typeof twitterCardRenderer.twitterScreenshotMetaText, "function");
  assert.equal(
    twitterCardRenderer.twitterScreenshotMetaText(
      { viewCount: 156_432 },
      { date: "27 Ağu 2026" },
    ),
    "27 Ağu 2026 · 156.432 görüntülenme",
  );
});
