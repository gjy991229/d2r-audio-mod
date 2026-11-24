"""Package the relevant files from previously prepared local inventory reference Mods."""

import argparse
import gzip
import json
import re
import struct
from pathlib import Path

LAYOUT = Path("data/global/ui/layouts")
SPRITES = Path("data/hd/global/ui")
SPRITE_PREFIX = "d2rhub/jcy_inventory/"
STYLE_PREFIX = "D2RHubJcyInventory"


def read(root, name):
    return json.loads((root / name).read_text(encoding="utf-8-sig"))


def strings(value):
    if isinstance(value, dict):
        for child in value.values():
            yield from strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from strings(child)
    elif isinstance(value, str):
        yield value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path, help="Prepared .mpq directory with 11 page buttons")
    parser.add_argument("nine_pages", type=Path, help="Prepared .mpq directory with 9 page buttons")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.reference
    bag, bank, cube, close = (
        read(root, LAYOUT / filename)
        for filename in (
            "playerinventoryexpansionlayouthd.json",
            "bankexpansionlayouthd.json",
            "JcyMiniCubehd.json",
            "JcyMiniCubeClosehd.json",
        )
    )
    bank_nine = read(args.nine_pages, LAYOUT / "bankexpansionlayouthd.json")
    assert sum(value.startswith("page_button_") for value in strings(bank)) == 11
    assert sum(value.startswith("page_button_") for value in strings(bank_nine)) == 9
    assert any(node["name"] == "OpenJcyMiniCube" for node in bag["children"])
    assert cube["name"] == "JcyMiniCube" and close["name"] == "JcyMiniCubeClose"
    pages = [read(root, LAYOUT / f"JcyStashPage{page}hd.json") for page in range(1, 12)]
    profile = read(root, LAYOUT / "_profilehd.json")
    styles = {
        key: value for key, value in profile.items()
        if key.startswith(STYLE_PREFIX) or key in {
            "RightPanelRect", "RightPanelAnchor", "LeftPanelRect", "LeftPanelAnchor", "ItemCellSize"
        }
    }
    references = set(strings([bag, bank, bank_nine, cube, close, pages, styles]))
    keys = {key for text in references for key in re.findall(r"@([A-Za-z0-9_]+)", text)}
    labels = [
        row for row in read(root, "data/local/lng/strings/ui.json")
        if row["Key"] in keys and row["id"] >= 100_000
    ]
    sprite_stems = sorted(text for text in references if text.startswith(SPRITE_PREFIX))
    sprites = {}
    for name in sprite_stems:
        if not re.fullmatch(r"d2rhub/jcy_inventory/(?:[a-z0-9_-]+/)*[a-z0-9_-]+", name):
            raise ValueError(f"Invalid sprite path: {name}")
        for suffix in (".sprite", ".lowend.sprite"):
            path = name + suffix
            sprites[path] = (root / SPRITES / path).read_bytes()
    if not labels or len(sprites) < 6:
        raise ValueError("Reference Mod is missing the JCY labels or sprites")
    manifest = {
        "bag": bag, "bank": bank, "bank_nine": bank_nine, "cube": cube, "close": close,
        "pages": pages,
        "styles": styles, "labels": labels,
        "sprites": [[path, len(data)] for path, data in sprites.items()],
    }
    header = json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("wb") as output:
        with gzip.GzipFile(fileobj=output, filename="", mode="wb", mtime=0) as bundle:
            bundle.write(struct.pack("<I", len(header)))
            bundle.write(header)
            for data in sprites.values():
                bundle.write(data)
    print(f"{args.output}: {len(sprites)} sprites, {len(labels)} strings, {args.output.stat().st_size:,} bytes")


if __name__ == "__main__":
    main()
