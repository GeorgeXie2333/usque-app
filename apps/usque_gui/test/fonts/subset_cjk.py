#!/usr/bin/env python3
"""Rebuild the checked-in Linux golden CJK subsets from verified originals."""

import argparse
import hashlib
from pathlib import Path

import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont

SOURCE_SHA256 = {
    "Regular": "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
    "Bold": "b5f0d1a190a7f9b43c310a8850630af12553df32c4c050543f9059732d9b4c0a",
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_directory", type=Path)
    parser.add_argument("--output-directory", type=Path, default=Path(__file__).parent)
    args = parser.parse_args()
    if fontTools.__version__ != "4.60.1":
        raise SystemExit("Regeneration requires FontTools 4.60.1.")
    codepoints = [
        int(line, 16)
        for line in Path(__file__).with_name("cjk-codepoints.txt").read_text().splitlines()
        if line and not line.startswith("#")
    ]
    args.output_directory.mkdir(parents=True, exist_ok=True)
    for weight, expected in SOURCE_SHA256.items():
        source = args.source_directory / f"NotoSansCJKsc-{weight}.otf"
        if hashlib.sha256(source.read_bytes()).hexdigest() != expected:
            raise SystemExit(f"Source SHA-256 mismatch: {source.name}")
        font = TTFont(source, recalcTimestamp=False)
        options = subset.Options()
        options.recalc_timestamp = False
        options.name_IDs = ["*"]
        options.name_legacy = True
        options.name_languages = ["*"]
        options.layout_features = ["*"]
        subsetter = subset.Subsetter(options=options)
        subsetter.populate(unicodes=codepoints)
        subsetter.subset(font)
        destination = args.output_directory / f"NotoSansCJKsc-{weight}.subset.otf"
        font.save(destination)
        print(f"{hashlib.sha256(destination.read_bytes()).hexdigest()}  {destination.name}")


if __name__ == "__main__":
    main()
