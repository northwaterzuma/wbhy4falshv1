#!/usr/bin/env python3
"""Check repository fixtures and JSON syntax without assigning acceptance results."""

import hashlib
import json
from pathlib import Path
import zipfile


ROOT = Path(__file__).resolve().parent.parent


def main():
    json_files = [
        *sorted((ROOT / "contracts").glob("*.json")),
        *sorted((ROOT / "examples").glob("*.json")),
    ]
    for path in json_files:
        json.loads(path.read_text(encoding="utf-8"))

    path = ROOT / "tests/fixtures/manifest.json"
    fixtures = json.loads(path.read_text(encoding="utf-8"))
    for entry in fixtures["files"]:
        fixture_path = ROOT / entry["path"]
        data = fixture_path.read_bytes()
        if len(data) != entry["size_bytes"]:
            raise ValueError(f"Unexpected length: {fixture_path}")
        if hashlib.sha256(data).hexdigest() != entry["sha256"]:
            raise ValueError(f"SHA-256 mismatch: {fixture_path}")
        if "entries" in entry:
            with zipfile.ZipFile(fixture_path) as archive:
                if archive.namelist() != entry["entries"] or archive.testzip() is not None:
                    raise ValueError(f"ZIP entries or CRC mismatch: {fixture_path}")
    for entry in fixtures["media_manifests"]:
        if not (ROOT / entry["path"]).is_file():
            raise FileNotFoundError(entry["path"])

    print(
        f"Parsed {len(json_files)} JSON files and checked "
        f"{len(fixtures['files'])} binary fixtures plus "
        f"{len(fixtures['media_manifests'])} media manifest paths."
    )
    print("Run the Rust tests and the workflows in ACCEPTANCE.md for behavioral evidence.")


if __name__ == "__main__":
    main()
