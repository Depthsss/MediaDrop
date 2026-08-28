import argparse
import contextlib
import json
import os
import re
import sys
from datetime import timezone
from urllib.parse import parse_qsl, urlparse

import instaloader
import browser_cookie3
from instaloader import Instaloader, Post

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

SUPPORTED_BROWSER_IDS = {
    "opera_gx": "opera_gx",
    "opera": "opera",
    "chrome": "chrome",
    "edge": "edge",
    "firefox": "firefox",
}

SUPPORTED_BROWSER_LOADERS = {
    "opera_gx": browser_cookie3.opera_gx,
    "opera": browser_cookie3.opera,
    "chrome": browser_cookie3.chrome,
    "edge": browser_cookie3.edge,
    "firefox": browser_cookie3.firefox,
}


def fail(message, code=1):
    print(json.dumps({"ok": False, "error": str(message)}), file=sys.stdout)
    return code


def safe(callable_value, default=None):
    try:
        value = callable_value()
        return default if value is None else value
    except Exception:
        return default


def first_non_empty(*values):
    for value in values:
        if value is None:
            continue
        text = str(value).strip()
        if text:
            return text
    return ""


def shortcode_from_url(url):
    clean = (url or "").strip()
    parsed = urlparse(clean)
    host = (parsed.netloc or "").lower()
    if not host.endswith("instagram.com") and not host.endswith("instagr.am"):
        raise ValueError("Instagram linki degil.")

    parts = [part for part in parsed.path.split("/") if part]
    if len(parts) < 2 or parts[0].lower() not in {"p", "reel", "tv"}:
        raise ValueError("Instagram fotograf/reel gonderi linki okunamadi.")

    shortcode = parts[1].strip()
    if not re.match(r"^[A-Za-z0-9_-]+$", shortcode):
        raise ValueError("Instagram shortcode gecersiz.")
    return shortcode


def extension_from_url(url, media_type):
    parsed = urlparse(url or "")
    for key, value in parse_qsl(parsed.query, keep_blank_values=True):
        lower_key = key.lower()
        lower_value = value.lower()
        if lower_key in {"format", "fm", "ext"}:
            for ext in ("jpg", "jpeg", "png", "webp", "gif", "avif", "mp4", "mov", "webm"):
                if ext in lower_value:
                    return ext
        if lower_key == "stp":
            for ext in ("dst-jpg", "dst-webp", "dst-png"):
                if ext in lower_value:
                    return ext.split("-")[-1]

    path = parsed.path.rsplit("/", 1)[-1]
    if "." in path:
        ext = path.rsplit(".", 1)[-1].lower()
        if ext in {"jpg", "jpeg", "png", "webp", "gif", "avif", "mp4", "mov", "webm"}:
            return ext

    return "mp4" if media_type == "video" else "jpg"


def dimensions_from_node(node):
    dims = (node or {}).get("dimensions") or {}
    width = dims.get("width")
    height = dims.get("height")
    try:
        width = int(width) if width else None
    except Exception:
        width = None
    try:
        height = int(height) if height else None
    except Exception:
        height = None
    return width, height


def display_date(post):
    value = safe(lambda: post.date_utc)
    if not value:
        return None
    if value.tzinfo is None:
        value = value.replace(tzinfo=timezone.utc)
    return value.strftime("%d %b %Y")


def post_metadata(post):
    profile = safe(lambda: post.owner_profile)
    username = first_non_empty(
        safe(lambda: profile.username if profile else ""),
        safe(lambda: post.owner_username),
    )
    fullname = first_non_empty(
        safe(lambda: profile.full_name if profile else ""),
        username,
    )
    avatar = first_non_empty(
        safe(lambda: profile.profile_pic_url if profile else ""),
        safe(lambda: profile.profile_pic_url_no_iphone if profile else ""),
    )
    caption = first_non_empty(safe(lambda: post.caption), safe(lambda: post.title))
    title = caption or f"Instagram gonderisi - {username or 'medya'}"

    return {
        "authorName": fullname or username or "Instagram",
        "authorHandle": f"@{username}" if username else None,
        "avatarUrl": avatar or None,
        "text": caption or None,
        "displayDate": display_date(post),
        "replyCount": safe(lambda: int(post.comments), None),
        "retweetCount": None,
        "likeCount": safe(lambda: int(post.likes), None),
        "viewCount": safe(lambda: int(post.video_view_count), None),
        "title": title,
        "uploader": fullname or username or "Instagram",
    }


def media_item(shortcode, index, media_type, url, node, metadata):
    width, height = dimensions_from_node(node)
    title = metadata["text"] or metadata["title"]
    return {
        "id": f"{shortcode}-{index}",
        "type": media_type,
        "sourceIndex": index,
        "previewUrl": url,
        "width": width,
        "height": height,
        "extension": extension_from_url(url, media_type),
        "isStory": False,
        "title": title,
        "authorName": metadata["authorName"],
        "authorHandle": metadata["authorHandle"],
        "avatarUrl": metadata["avatarUrl"],
        "text": metadata["text"],
        "displayDate": metadata["displayDate"],
        "replyCount": metadata["replyCount"],
        "retweetCount": metadata["retweetCount"],
        "likeCount": metadata["likeCount"],
        "viewCount": metadata["viewCount"],
    }


def post_items(post, shortcode, metadata):
    typename = safe(lambda: post.typename, "")
    items = []

    if typename == "GraphSidecar":
        edges = safe(lambda: post._field("edge_sidecar_to_children", "edges"), []) or []
        sidecar_nodes = list(post.get_sidecar_nodes())
        for index, sidecar in enumerate(sidecar_nodes):
            raw_node = {}
            if index < len(edges):
                raw_node = (edges[index] or {}).get("node") or {}
            media_type = "video" if sidecar.is_video else "photo"
            url = sidecar.video_url if sidecar.is_video else sidecar.display_url
            if not url:
                continue
            items.append(media_item(shortcode, index, media_type, url, raw_node, metadata))
        return items

    node = getattr(post, "_node", {}) or {}
    media_type = "video" if safe(lambda: post.is_video, False) else "photo"
    url = safe(lambda: post.video_url if media_type == "video" else post.url, "")
    if url:
        items.append(media_item(shortcode, 0, media_type, url, node, metadata))
    return items


def content_kind(items):
    if not items:
        return "unknown"
    if len(items) > 1:
        return "carousel"
    return items[0].get("type") or "unknown"


def create_loader(args):
    loader = Instaloader(
        quiet=True,
        download_pictures=False,
        download_videos=False,
        download_video_thumbnails=False,
        download_geotags=False,
        download_comments=False,
        save_metadata=False,
        compress_json=False,
        request_timeout=35.0,
        max_connection_attempts=2,
    )

    username = None
    if args.session and args.session_username:
        loader.load_session_from_file(args.session_username, args.session)
        username = safe(lambda: loader.test_login(), args.session_username) or args.session_username
        loader.context.username = username

    if args.cookie_jar:
        username = import_netscape_cookie_jar(args.cookie_jar, loader)

    if args.browser:
        browser = SUPPORTED_BROWSER_IDS.get(args.browser)
        if not browser:
            raise ValueError("Desteklenmeyen tarayici secildi.")
        with contextlib.redirect_stdout(sys.stderr):
            import_browser_session(
                browser,
                loader,
                cookie_file=args.cookie_file,
                key_file=args.key_file,
            )
        username = loader.context.username or safe(lambda: loader.test_login(), None)

    if args.session_out and loader.context.is_logged_in:
        loader.save_session_to_file(args.session_out)

    return loader, username


def import_netscape_cookie_jar(cookie_jar, loader):
    cookies = {}
    with open(cookie_jar, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            clean = line.strip()
            if not clean or clean.startswith("#"):
                continue
            parts = clean.split("\t")
            if len(parts) < 7:
                continue
            domain, _subdomains, _path, _secure, _expires, name, value = parts[:7]
            if "instagram.com" not in domain.lower():
                continue
            if name and value:
                cookies[name] = value

    if not cookies:
        raise RuntimeError("Kayitli Instagram cookie jar bos veya okunamadi.")

    loader.context.update_cookies(cookies)
    username = loader.test_login()
    if not username:
        raise RuntimeError(
            "Kayitli Instagram cookie'leri okundu ama oturum dogrulanamadi. "
            "Instagram'a giris yaptigin tarayicidan yeniden izin ver."
        )

    loader.context.username = username
    return username


def import_browser_session(browser, loader, cookie_file="", key_file=""):
    cookie_loader = SUPPORTED_BROWSER_LOADERS.get(browser)
    if not cookie_loader:
        raise ValueError("Desteklenmeyen tarayici secildi.")

    kwargs = {"domain_name": "instagram"}
    if cookie_file:
        kwargs["cookie_file"] = cookie_file
    if key_file and browser != "firefox":
        kwargs["key_file"] = key_file

    try:
        browser_cookies = list(cookie_loader(**kwargs))
    except RuntimeError as exc:
        message = str(exc)
        if "DPAPI" in message or "cipher text" in message:
            raise RuntimeError(
                "Secili tarayicinin Instagram cookie anahtari Windows tarafinda acilamadi. "
                "Bu Opera GX/Chromium profilinde korumali cookie sifrelemesi olabilir; "
                "Chrome, Edge veya Firefox'ta Instagram'a giris yapip o tarayiciyi sec."
            ) from exc
        raise
    except Exception as exc:
        message = str(exc)
        if "requires admin" in message.lower() or "shadow" in message.lower():
            raise RuntimeError(
                "Secili tarayicinin cookie veritabani su anda kilitli ve Windows shadow copy "
                "yetkisi olmadan okunamadi. Chrome, Edge veya Firefox'ta Instagram'a giris yapip "
                "o tarayiciyi sec."
            ) from exc
        if "Failed to find cookies" in message:
            raise RuntimeError(
                "Secili tarayicida Instagram cookie dosyasi bulunamadi. "
                "Instagram'a giris yaptigin kurulu tarayiciyi sec."
            ) from exc
        raise

    cookies = {}
    for cookie in browser_cookies:
        domain = (cookie.domain or "").lower()
        if "instagram" in domain and cookie.value:
            cookies[cookie.name] = cookie.value

    if not cookies:
        raise RuntimeError(
            "Secili tarayicida Instagram oturumu bulunamadi. "
            "Instagram'a giris yaptigin tarayiciyi sec."
        )

    loader.context.update_cookies(cookies)
    username = loader.test_login()
    if not username:
        raise RuntimeError(
            "Secili tarayicidan Instagram cookie'leri okundu ama oturum dogrulanamadi. "
            "Instagram'a giris yaptigin baska bir tarayiciyi sec."
        )

    loader.context.username = username
    print(f"{username} has been successfully logged in.")


def analyze(args):
    shortcode = shortcode_from_url(args.url)
    loader, session_username = create_loader(args)
    post = Post.from_shortcode(loader.context, shortcode)
    metadata = post_metadata(post)
    items = post_items(post, shortcode, metadata)

    result = {
        "ok": True,
        "platform": "instagram",
        "contentKind": content_kind(items),
        "title": metadata["title"],
        "uploader": metadata["uploader"],
        "items": items,
        "videoInfo": None,
        "sessionUsername": session_username or loader.context.username,
        "instaloaderVersion": getattr(instaloader, "__version__", ""),
    }
    print(json.dumps(result, ensure_ascii=False), file=sys.stdout)
    return 0


def build_parser():
    parser = argparse.ArgumentParser(prog="instaloader-helper")
    subparsers = parser.add_subparsers(dest="command", required=True)

    for name in ("analyze", "download-item", "download-batch"):
        command = subparsers.add_parser(name)
        command.add_argument("--url", required=True)
        command.add_argument("--browser", default="")
        command.add_argument("--cookie-file", default="")
        command.add_argument("--key-file", default="")
        command.add_argument("--cookie-jar", default="")
        command.add_argument("--session", default="")
        command.add_argument("--session-username", default="")
        command.add_argument("--session-out", default="")
        command.add_argument("--source-index", type=int, default=0)
        command.add_argument("--output-dir", default="")

    return parser


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return analyze(args)
    except Exception as exc:
        return fail(exc)


if __name__ == "__main__":
    raise SystemExit(main())
