# Third-Party Notices

MediaDrop is licensed under the MIT License. The following components are distributed with the Windows application and remain subject to their own licenses. MediaDrop does not relicense these components.

| Component | Bundled version | License | Source |
|---|---:|---|---|
| aria2 | 1.37.0 | GPL-2.0-or-later | https://github.com/aria2/aria2/tree/release-1.37.0 |
| Deno | 2.7.14 | MIT | https://github.com/denoland/deno/tree/v2.7.14 |
| FFmpeg / ffprobe (Gyan full build) | 8.1 | GPL-3.0-or-later; linked libraries retain their licenses | https://github.com/FFmpeg/FFmpeg/tree/n8.1 |
| gallery-dl | 1.32.3 | GPL-2.0-only | https://codeberg.org/mikf/gallery-dl/src/tag/v1.32.3 |
| yt-dlp nightly | 2026.08.18.122307 | Unlicense | https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/tag/2026.08.18.122307 |
| Instaloader | bundled in instaloader-helper | MIT | https://github.com/instaloader/instaloader |
| browser-cookie3 | bundled in instaloader-helper | LGPL-3.0 | https://github.com/borisbabic/browser_cookie3 |
| Python runtime | 3.10 | Python Software Foundation License | https://www.python.org/downloads/release/python-3106/ |
| PyInstaller bootloader | 6.22.2 | GPL-2.0-or-later with PyInstaller bootloader exception | https://github.com/pyinstaller/pyinstaller/tree/v6.22.2 |
| Requests and urllib3 | 2.34.2 / 2.7.0 | Apache-2.0 / MIT | https://pypi.org/project/requests/2.34.2/ |
| pycryptodomex | 3.23.0 | BSD/Public Domain | https://pypi.org/project/pycryptodomex/3.23.0/ |
| pywin32 and pywin32-ctypes | 312 / 0.2.3 | PSF / BSD-3-Clause | https://pypi.org/project/pywin32/312/ |
| NSIS | 3.11 | zlib/libpng | https://github.com/kichik/nsis/tree/v3.11 |
| Microsoft Edge WebView2 bootstrapper | Evergreen | Microsoft software license | https://developer.microsoft.com/microsoft-edge/webview2/ |
| Tauri and official plugins | Cargo.lock | MIT or Apache-2.0 | https://github.com/tauri-apps/tauri |
| rookie | 0.5.6 | MIT | `src-tauri/vendor/rookie-0.5.6/MIT-LICENSE.txt` |
| Instrument Sans | bundled font | SIL Open Font License 1.1 | `src/assets/fonts/InstrumentSans-OFL.txt` |
| Lucide Icons | current SVG sources | ISC | https://lucide.dev/icons/ |

The release page accompanies copyleft binaries with this notice and links to the exact source versions. Requests for the corresponding source code of a distributed build may be submitted through the repository security/contact facilities for at least three years after that build is released.

The authoritative upstream executable hashes and download locations are stored in `src-tauri/binaries/sidecars.lock.json`. The complete locked dependency inventories are `src-tauri/Cargo.lock`, `package-lock.json`, and `tools/instagram-helper/requirements.lock.txt`.
