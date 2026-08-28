# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Windows users who want to save media from the browser quickly without learning command-line tools or managing separate download engines.

## Product Purpose

MediaDrop downloads, converts, clips, validates, and reveals media from supported services through one local desktop workflow. Success means the user can move from a supported page to a trustworthy local result with minimal interruption.

## Positioning

MediaDrop combines a lightweight Chromium companion with one local Tauri, yt-dlp, gallery-dl, and FFmpeg pipeline. The browser remains a quick control surface while the desktop process owns authentication, processing, validation, progress, and the single-active-job rule.

## Operating Context

- Windows 10 and Windows 11 x64 desktop computers.
- Opera, Opera GX, Chrome, and Edge companion setup.
- Turkish-first consumer-facing interface with English fallback where required by packaging tools.
- Quick downloads use the default MediaDrop downloads directory; advanced controls remain in the desktop app.

## Capabilities and Constraints

- Supports YouTube, Instagram, X/Twitter, and TikTok media workflows.
- Uses one active download job; there is no hidden queue or second download engine.
- The browser extension never reads cookies, authorization headers, or signed media tokens.
- The Windows installer must work offline once downloaded and must not silently install a browser extension or modify browser policy.
- The opt-in setup action opens a local, browser-specific sideload guide; it never changes browser policy or automates protected browser UI.

## Brand Commitments

- Product name: MediaDrop.
- Official gold MD mark from `src-tauri/icons/icon.png`.
- Dark, focused product surfaces with precise high-contrast controls.
- Direct, practical copy without hype or invented performance claims.

## Evidence on Hand

- Working desktop application and browser companion in this repository.
- Existing product tokens, fonts, logo assets, and tested setup/package scripts.
- No testimonials, customer counts, benchmark claims, or commercial proof should be invented.

## Product Principles

1. Keep the fastest common action obvious.
2. Keep private media data local and out of diagnostics.
3. Reuse one proven processing pipeline instead of duplicating engines.
4. Explain blockers and recovery actions in plain language.
5. Prefer a small reliable workflow over hidden automation.

## Accessibility & Inclusion

The setup flow must remain keyboard operable, maintain readable contrast, support reduced motion, avoid color-only status communication, and keep controls usable with increased text size.
