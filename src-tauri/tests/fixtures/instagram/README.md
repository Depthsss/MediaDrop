# Instagram gallery-dl fixtures

These fixtures model the JSON and JSONL shapes consumed by
`gallery_stdout_to_items`. They contain only synthetic identities and invented,
unsigned CDN paths. They must never contain cookies, browser data, query-string
signatures, or copied production URLs.

## Successful normalization cases

- `root-owner-commenter-trap.json`: one photo whose root `owner` must win over
  the nested comment author and avatar.
- `extensionless-avatar.json`: one photo with a trusted-CDN avatar URL that has
  no filename extension. The explicit `profile_pic_url` field establishes its
  avatar role.
- `photo-story.json`: one portrait photo marked as Story content by
  `subcategory: stories`.
- `video-story-has-audio.json`: one portrait MP4 Story with `has_audio: true`,
  AAC metadata, and a duration expressed in seconds.
- `mixed-unsorted-stories.jsonl`: three valid gallery-dl records deliberately
  ordered late, early, middle. Story finalization should order them by
  `taken_at` as early, middle, late while preserving photo/video and audio
  metadata.

## Share-link owner resolution cases

- `share-resolved-target.jsonl`: the resolved Story target has media ID `1002`
  and canonical root owner `fixture_owner`. Nested commenter and tagged-user
  identities are deliberate traps and must never become the Story owner.
- `share-owner-active-stories.jsonl`: the canonical owner's active Story list
  is deliberately emitted as `1003`, `1001`, `1002`; chronological sorting
  should produce `1001`, `1002`, `1003`. It mixes portrait photos with an AAC
  audio video.
- `share-owner-mismatch.jsonl`: media ID `1002` belongs to
  `fixture_other_owner` and must be rejected when resolving
  `fixture_owner`'s share target.

## Expected no-media/error cases

- `empty.json`: a valid empty gallery result.
- `expired.json`: a valid JSON status payload with no downloadable media.
- `schema-error.json`: valid JSON with deliberately wrong field types and no
  usable media URL. This tests schema rejection separately from JSON syntax
  errors.

For `.jsonl`, validate and parse each non-empty line independently. All other
fixture files are complete JSON documents.
