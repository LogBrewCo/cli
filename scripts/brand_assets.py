#!/usr/bin/env python3
"""Generate and verify CLI brand derivatives from the canonical SDK assets."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import sys
import zlib
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = REPO_ROOT / "wix" / "brand-assets.json"
README_PATH = REPO_ROOT / "README.md"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
IMAGE_SUFFIXES = {".ico", ".icns", ".jpeg", ".jpg", ".png", ".svg"}
IGNORED_DIRECTORIES = {".git", "node_modules", "target"}


def fail(message: str) -> None:
    raise ValueError(message)


def sha256_bytes(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def load_manifest() -> dict[str, object]:
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    if set(manifest) != {"schemaVersion", "canonicalSource", "outputs"}:
        fail("brand manifest top-level fields drifted")
    if manifest["schemaVersion"] != 1:
        fail("brand manifest has an unsupported schemaVersion")
    return manifest


def png_contract(content: bytes) -> tuple[int, int, int]:
    if content[:8] != PNG_SIGNATURE:
        fail("canonical app icon is not a PNG")
    if len(content) < 33:
        fail("canonical app icon PNG is truncated")
    length = struct.unpack(">I", content[8:12])[0]
    kind = content[12:16]
    payload = content[16 : 16 + length]
    expected_crc = struct.unpack(">I", content[16 + length : 20 + length])[0]
    if kind != b"IHDR" or length != 13:
        fail("canonical app icon PNG is missing its IHDR")
    if zlib.crc32(kind + payload) & 0xFFFFFFFF != expected_crc:
        fail("canonical app icon PNG header checksum mismatch")
    width, height, depth, color_type, compression, filtering, interlace = struct.unpack(
        ">IIBBBBB", payload
    )
    if (depth, compression, filtering, interlace) != (8, 0, 0, 0):
        fail("canonical app icon PNG encoding drifted")
    return width, height, color_type


def build_ico(png: bytes) -> bytes:
    if png_contract(png) != (256, 256, 2):
        fail("canonical installer icon must be a 256px RGB PNG")
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 22)
    return header + entry + png


def embedded_png(ico: bytes) -> bytes:
    if len(ico) < 22:
        fail("tracked installer icon is truncated")
    if struct.unpack("<HHH", ico[:6]) != (0, 1, 1):
        fail("tracked installer icon directory drifted")
    width, height, colors, reserved, planes, bits, size, offset = struct.unpack(
        "<BBBBHHII", ico[6:22]
    )
    if (width, height, colors, reserved, planes, bits, offset) != (
        0,
        0,
        0,
        0,
        1,
        32,
        22,
    ):
        fail("tracked installer icon entry drifted")
    if size != len(ico) - offset:
        fail("tracked installer icon payload length drifted")
    return ico[offset:]


def source_url(source: dict[str, object], asset: dict[str, object]) -> str:
    revision = str(source["revision"])
    if re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        fail("canonical SDK source revision must be an exact commit")
    repository = str(source["repository"])
    if repository != "https://github.com/LogBrewCo/sdk":
        fail("canonical SDK brand repository drifted")
    return (
        "https://raw.githubusercontent.com/LogBrewCo/sdk/"
        f"{revision}/{asset['path']}"
    )


def check_manifest_asset(
    asset: dict[str, object],
    *,
    expected_path: str,
    expected_sha256: str,
    expected_size: int,
    expected_color_type: str,
    expected_presentation: str,
) -> None:
    expected = {
        "path": expected_path,
        "sha256": expected_sha256,
        "width": expected_size,
        "height": expected_size,
        "colorType": expected_color_type,
        "presentation": expected_presentation,
    }
    if asset != expected:
        fail(f"canonical brand contract drifted for {expected_path}")


def tracked_image_inventory() -> set[str]:
    found: set[str] = set()
    for path in REPO_ROOT.rglob("*"):
        if not path.is_file() or path.suffix.lower() not in IMAGE_SUFFIXES:
            continue
        relative = path.relative_to(REPO_ROOT)
        if any(part in IGNORED_DIRECTORIES for part in relative.parts):
            continue
        found.add(relative.as_posix())
    return found


def check_repository() -> None:
    manifest = load_manifest()
    source = manifest["canonicalSource"]
    if not isinstance(source, dict) or set(source) != {
        "repository",
        "revision",
        "appIcon",
        "inlineLogo",
    }:
        fail("canonical brand source fields drifted")
    app_icon = source["appIcon"]
    inline_logo = source["inlineLogo"]
    if not isinstance(app_icon, dict) or not isinstance(inline_logo, dict):
        fail("canonical brand assets must be objects")
    check_manifest_asset(
        app_icon,
        expected_path="assets/brand/app-icon-256.png",
        expected_sha256="4547f1d32a87f90177bd430b70b4471e2aa5aceb99181c039e8233acbc561152",
        expected_size=256,
        expected_color_type="rgb",
        expected_presentation="espresso_app_icon",
    )
    check_manifest_asset(
        inline_logo,
        expected_path="assets/brand/logbrew-logo-transparent-512.png",
        expected_sha256="a98dd05599862a0ba52c065c83fc60d386d04fa8b1efe46d11b0b6d4b5c4c8bc",
        expected_size=512,
        expected_color_type="rgba",
        expected_presentation="transparent_inline",
    )

    inline_url = source_url(source, inline_logo)
    readme = README_PATH.read_text(encoding="utf-8")
    image_urls = re.findall(r'<img\s+src="([^"]+)"', readme)
    if image_urls != [inline_url]:
        fail("README must use only the exact canonical transparent SDK logo")

    outputs = manifest["outputs"]
    if not isinstance(outputs, list) or len(outputs) != 1:
        fail("CLI brand manifest must declare exactly one local output")
    output = outputs[0]
    if not isinstance(output, dict) or set(output) != {
        "path",
        "sha256",
        "kind",
        "width",
        "height",
        "presentation",
    }:
        fail("CLI brand output fields drifted")
    if (
        output["path"],
        output["kind"],
        output["width"],
        output["height"],
        output["presentation"],
    ) != ("wix/Product.ico", "ico_png", 256, 256, "espresso_app_icon"):
        fail("CLI installer icon contract drifted")

    icon_path = REPO_ROOT / str(output["path"])
    if not icon_path.is_file():
        fail("tracked CLI installer icon is missing")
    ico = icon_path.read_bytes()
    if sha256_bytes(ico) != output["sha256"]:
        fail("tracked CLI installer icon digest drifted")
    png = embedded_png(ico)
    if sha256_bytes(png) != app_icon["sha256"]:
        fail("tracked CLI installer icon is not the canonical SDK app icon")
    if png_contract(png) != (256, 256, 2):
        fail("tracked CLI installer icon pixels have the wrong contract")
    if build_ico(png) != ico:
        fail("tracked CLI installer icon is not reproducible")
    if tracked_image_inventory() != {"wix/Product.ico"}:
        fail("CLI image inventory contains a duplicate or unapproved brand asset")


def write_output(source_path: Path) -> str:
    manifest = load_manifest()
    source = manifest["canonicalSource"]
    if not isinstance(source, dict) or not isinstance(source.get("appIcon"), dict):
        fail("canonical app icon manifest entry is invalid")
    app_icon = source["appIcon"]
    png = source_path.read_bytes()
    if sha256_bytes(png) != app_icon["sha256"]:
        fail("provided source is not the exact canonical SDK app icon")
    ico = build_ico(png)
    output_path = REPO_ROOT / "wix" / "Product.ico"
    output_path.write_bytes(ico)
    return sha256_bytes(ico)


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    parser.add_argument("--source", type=Path)
    args = parser.parse_args()
    try:
        if args.check:
            if args.source is not None:
                fail("--source is only valid with --write")
            check_repository()
            print("CLI brand assets ok")
        else:
            if args.source is None:
                fail("--write requires --source")
            digest = write_output(args.source)
            print(f"generated wix/Product.ico sha256={digest}")
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"CLI brand asset check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
