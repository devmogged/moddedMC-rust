#!/usr/bin/env python3
"""Validate the local source-derived port against the official release JAR.

No original source files or copyrighted asset bytes are copied into git.
Usage: py -3 tools/audit_artifact.py --jar N:/.../legendary_monsters-2.2.4.jar
Optionally pass --decompiled D:/mc-ref/.../item/ModItems.java to verify constructors.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path
from zipfile import ZipFile

PINNED_SHA256 = "5a659453d7f439f36a1cdf0cb74eb29cfa98d2b6c693424157e2b67ecb16e136"
NAMESPACE = "legendary_monsters"
MOD = Path(__file__).resolve().parents[1]
REGISTRATION_PATTERN = re.compile(
    r'\(\s*"([a-z0-9_]+)"\s*,\s*"([^"]+)"\s*,\s*"minecraft:([^"]+)"\s*,?\s*\)'
)


def verify(jar_path: Path, java_path: Path | None) -> None:
    raw = jar_path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    assert digest == PINNED_SHA256, (
        "Wrong original release: expected SHA-256 " + PINNED_SHA256 + " but got " + digest
    )
    source = (MOD / "src/lib.rs").read_text(encoding="utf-8")
    registrations = REGISTRATION_PATTERN.findall(source)
    assert len(registrations) == 24, f"expected 24 selected items; got {len(registrations)}"
    names = [part[0] for part in registrations]
    assert len(set(names)) == len(names), "duplicate Rust item IDs"
    assert all(carrier for _, _, carrier in registrations)
    guide_book = re.search(
        r'id: "legendary_monsters:guide_book",\s*display_name: "Guide Book",'
        r'\s*carrier: "minecraft:writable_book",\s*model: "legendary_monsters:item/guide_book"',
        source,
    )
    assert guide_book, "guide_book must delegate to vanilla writable_book with its original model"
    with ZipFile(jar_path) as original:
        archive = set(original.namelist())
        models = [
            f"assets/{NAMESPACE}/models/item/{name}.json"
            for name in names
        ]
        models.append("assets/legendary_monsters/models/item/guide_book.json")
        assert set(models) <= archive, "original model reference missing: " + str(set(models) - archive)
        original_english = json.loads(original.read("assets/legendary_monsters/lang/en_us.json"))
        for name, display_name, _ in registrations:
            translation_key = f"item.legendary_monsters.{name}"
            original_display = original_english.get(translation_key, translation_key)
            assert display_name == original_display, (
                f"{name}: display {display_name!r} differs from original {original_display!r}"
            )
        assert original_english.get("item.legendary_monsters.guide_book") == "Guide Book"
        # The original models and textures are intentionally never copied.
        recipes = sorted((MOD / "resources/data/legendary_monsters/recipe").glob("*.json"))
        assert len(recipes) == 2, f"unexpected locally shipped recipes: {len(recipes)}"
        for recipe in recipes:
            archive_name = "data/legendary_monsters/recipe/" + recipe.name
            assert archive_name in archive, f"unknown source recipe {archive_name}"
            expected = json.loads(original.read(archive_name))
            actual = json.loads(recipe.read_text(encoding="utf-8"))
            assert actual == expected, f"recipe mismatch: {recipe.name}"
            assert actual["type"] in ("minecraft:crafting_shaped", "minecraft:crafting_shapeless")
            text = json.dumps(actual)
            for found in re.findall(r'legendary_monsters:[a-z0-9_]+', text):
                assert found.split(":", 1)[1] in names, f"recipe references unported item: {found}"
        tags = sorted((MOD / "resources/data/minecraft/tags/item").glob("*.json"))
        assert len(tags) == 2, f"unexpected locally shipped tags: {len(tags)}"
        for tag in tags:
            name = "data/minecraft/tags/item/" + tag.name
            assert name in archive, f"missing source tag: {name}"
            expected = json.loads(original.read(name))
            actual = json.loads(tag.read_text(encoding="utf-8"))
            assert actual.get("replace", False) is False, "never replace base vanilla tags"
            assert set(actual["values"]) <= set(expected["values"]), f"tag differs from original: {tag.name}"
            assert all(v.split(":", 1)[1] in names for v in actual["values"])
        assert not (MOD / "resources/assets").exists(), "restricted original visuals may not be bundled"
        print(f"Verified {len(models)} original item model paths, {len(recipes)} recipes, {len(tags)} source tags")
    if java_path:
        java = java_path.read_text(encoding="utf-8")
        matches = list(re.finditer(r'ITEMS\.register\(\s*"([^"]+)"', java))
        entries = {
            m.group(1): java[m.end():matches[i + 1].start() if i + 1 < len(matches) else len(java)]
            for i, m in enumerate(matches)
        }
        assert len(entries) >= 132, f"expected >= 132 explicit registrations, found {len(entries)}"
        fires = []
        for name, _, carrier in registrations:
            assert name in entries, f"missing source registration: {name}"
            ctor = entries[name].split(";", 1)[0]
            assert "new Item(new Properties()" in ctor, f"item has custom class or constructor: {name}"
            assert not any(x in ctor for x in (".food(", ".rarity(", ".stacksTo(", ".jukeboxPlayable(")), name
            resistant = ".fireResistant()" in ctor
            assert resistant == (carrier == "netherite_ingot"), f"wrong fire-resistant carrier for {name}"
            if resistant:
                fires.append(name)
        assert len(fires) == 5, f"unexpected fire-resistant item count: {fires}"
        guide_ctor = entries.get("guide_book", "").split(";", 1)[0]
        assert "new WritableBookItem(new Properties())" in guide_ctor, (
            "the original guide_book must be an unmodified vanilla WritableBookItem"
        )
        print(
            f"Verified source registrations and vanilla property flags for {len(names)} "
            "plain items (5 fire-resistant) plus the vanilla WritableBookItem"
        )
    print("PASS: exact 2.2.4 artifact audit")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", required=True, type=Path, help="original 2.2.4 NeoForge JAR")
    parser.add_argument("--decompiled", type=Path, help="decompiled 2.2.4 ModItems.java")
    opts = parser.parse_args()
    verify(opts.jar, opts.decompiled)
