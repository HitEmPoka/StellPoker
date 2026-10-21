#!/usr/bin/env python3
"""Download and unpack the official Freighter Chromium extension.

The real-extension wallet smoke (issue #651) needs an unpacked extension
directory, but Freighter is distributed through the Chrome Web Store as a CRX
archive. This helper fetches the CRX for the pinned extension ID, strips the
CRX header, and extracts the ZIP payload.

Usage:
    python3 scripts/fetch-freighter-extension.py --out /tmp/freighter-extension

    cd app
    REAL_FREIGHTER_SMOKE=1 \
    FREIGHTER_EXTENSION_PATH=/tmp/freighter-extension \
    npx playwright test --config=e2e/playwright.config.ts \
      --project=chromium tests/wallet-real-extension.spec.ts
"""

from __future__ import annotations

import argparse
import io
import shutil
import struct
import sys
import urllib.request
import zipfile
from pathlib import Path

# Official Freighter extension ID used by the frontend integrity check.
DEFAULT_EXTENSION_ID = "bcacfldlkkdogcmkkibnjlakofdplcbk"
CRX_URL = (
    "https://clients2.google.com/service/update2/crx"
    "?response=redirect&prodversion=121.0&acceptformat=crx2,crx3"
    "&x=id%3D{extension_id}%26uc"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("/tmp/freighter-extension"),
        help="Directory to extract the extension into",
    )
    parser.add_argument(
        "--extension-id",
        default=DEFAULT_EXTENSION_ID,
        help="Chrome Web Store extension ID",
    )
    parser.add_argument(
        "--url",
        default=None,
        help="Override the CRX download URL",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Replace an existing output directory",
    )
    return parser.parse_args()


def extract_zip_payload(crx: bytes) -> bytes:
    """Return the ZIP payload from a CRX2/CRX3 archive."""
    if not crx.startswith(b"Cr24"):
        raise ValueError("download is not a CRX archive (bad magic)")

    version = struct.unpack("<I", crx[4:8])[0]
    if version == 3:
        header_length = struct.unpack("<I", crx[8:12])[0]
        payload_offset = 12 + header_length
    elif version == 2:
        public_key_length, signature_length = struct.unpack("<II", crx[8:16])
        payload_offset = 16 + public_key_length + signature_length
    else:
        raise ValueError(f"unsupported CRX version {version}")

    if payload_offset >= len(crx):
        raise ValueError("CRX header claims a payload beyond the download")
    return crx[payload_offset:]


def safe_extract(archive: zipfile.ZipFile, destination: Path) -> None:
    destination = destination.resolve()
    for member in archive.infolist():
        target = (destination / member.filename).resolve()
        if target != destination and destination not in target.parents:
            raise ValueError(f"refusing to extract outside {destination}: {member.filename}")
    archive.extractall(destination)


def main() -> int:
    args = parse_args()
    url = args.url or CRX_URL.format(extension_id=args.extension_id)

    if args.out.exists():
        if not args.force:
            raise SystemExit(
                f"ERROR: {args.out} already exists; pass --force to replace it."
            )
        shutil.rmtree(args.out)

    print(f"Downloading Freighter extension from {url}")
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": (
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
                "(KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"
            )
        },
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        crx = response.read()

    payload = extract_zip_payload(crx)
    args.out.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
        safe_extract(archive, args.out)

    manifest = args.out / "manifest.json"
    if not manifest.is_file():
        raise SystemExit(f"ERROR: extracted extension has no manifest.json in {args.out}")

    print(f"Extracted Freighter extension to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
