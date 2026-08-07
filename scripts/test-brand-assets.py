#!/usr/bin/env python3
"""Focused tests for the CLI brand provenance and ICO generator."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "brand_assets",
    ROOT / "scripts" / "brand_assets.py",
)
assert SPEC is not None and SPEC.loader is not None
brand_assets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(brand_assets)


class BrandAssetTests(unittest.TestCase):
    def test_repository_brand_contract_is_current(self) -> None:
        brand_assets.check_repository()

    def test_installer_icon_round_trips_the_exact_embedded_png(self) -> None:
        tracked = (ROOT / "wix" / "Product.ico").read_bytes()
        png = brand_assets.embedded_png(tracked)
        self.assertEqual(brand_assets.build_ico(png), tracked)
        self.assertEqual(
            brand_assets.sha256_bytes(png),
            "4547f1d32a87f90177bd430b70b4471e2aa5aceb99181c039e8233acbc561152",
        )


if __name__ == "__main__":
    unittest.main()
