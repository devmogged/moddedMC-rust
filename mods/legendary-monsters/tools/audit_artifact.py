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
    assert len(registrations) == 26, f"expected 26 selected items; got {len(registrations)}"
    equipment = (MOD / "src/equipment.rs").read_text(encoding="utf-8")
    equipment_names = re.findall(r'register_item\(\s*defaults,\s*registry,\s*"([a-z0-9_]+)"', equipment)
    assert set(equipment_names) == {
        "enderitium_sword", "enderitium_pickaxe", "enderitium_axe",
        "enderitium_shovel", "enderitium_hoe", "enderitium_upgrade_smithing_template"
    }
    names = [part[0] for part in registrations] + equipment_names
    block_names = ["enderitium_block", "enderitium_ore"]
    assert len(set(names)) == len(names), "duplicate Rust item IDs"
    assert all(carrier for _, _, carrier in registrations)
    with ZipFile(jar_path) as original:
        archive = set(original.namelist())
        models = [
            f"assets/{NAMESPACE}/models/item/{name}.json"
            for name in names + block_names
        ]
        assert set(models) <= archive, "original model reference missing: " + str(set(models) - archive)
        for name in block_names:
            assert f"assets/{NAMESPACE}/models/block/{name}.json" in archive
        # The original models and textures are intentionally never copied.
        recipes = sorted((MOD / "resources/data/legendary_monsters/recipe").glob("*.json"))
        assert len(recipes) == 15, f"unexpected locally shipped recipes: {len(recipes)}"
        for recipe in recipes:
            archive_name = "data/legendary_monsters/recipe/" + recipe.name
            assert archive_name in archive, f"unknown source recipe {archive_name}"
            expected = json.loads(original.read(archive_name))
            actual = json.loads(recipe.read_text(encoding="utf-8"))
            assert actual == expected, f"recipe mismatch: {recipe.name}"
            assert actual["type"] in ("minecraft:crafting_shaped", "minecraft:crafting_shapeless", "minecraft:smithing_transform")
            text = json.dumps(actual)
            for found in re.findall(r'legendary_monsters:[a-z0-9_]+', text):
                assert found.split(":", 1)[1] in names + block_names, f"recipe references unported item: {found}"
        for name in block_names:
            path = f"data/{NAMESPACE}/loot_table/blocks/{name}.json"
            assert json.loads((MOD / "resources" / path).read_text(encoding="utf-8")) == json.loads(original.read(path))
        for name in ["mineable/pickaxe", "needs_diamond_tool"]:
            path = f"data/minecraft/tags/block/{name}.json"
            actual = json.loads((MOD / "resources" / path).read_text(encoding="utf-8"))
            assert actual == {"replace": False, "values": [f"{NAMESPACE}:{b}" for b in block_names]}
            assert set(actual["values"]) <= set(json.loads(original.read(path))["values"])
        tags = sorted((MOD / "resources/data/minecraft/tags/item").glob("*.json"))
        assert len(tags) == 7, f"unexpected locally shipped tags: {len(tags)}"
        for tag in tags:
            name = "data/minecraft/tags/item/" + tag.name
            assert name in archive, f"missing source tag: {name}"
            expected = json.loads(original.read(name))
            actual = json.loads(tag.read_text(encoding="utf-8"))
            assert actual.get("replace", False) is False, "never replace base vanilla tags"
            assert set(actual["values"]) <= set(expected["values"]), f"tag differs from original: {tag.name}"
            assert all(v.split(":", 1)[1] in names for v in actual["values"])
        # Host resolves item defaults before custom IDs exist; this equivalent
        # synthetic singleton tag is needed to defer the repair ingredient lookup.
        repair = MOD / "resources/data/legendary_monsters/tags/item/enderitium_repair.json"
        assert json.loads(repair.read_text(encoding="utf-8")) == {
            "replace": False, "values": ["legendary_monsters:enderitium_ingot"]
        }, "repair ingredient tag must contain only the source tier's ingot"
        assert not (MOD / "resources/assets").exists(), "restricted original visuals may not be bundled"
        print(f"Verified {len(names)} original item model paths, {len(recipes)} recipes, {len(tags)} source tags")
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
            assert not any(x in ctor for x in (".food(", ".jukeboxPlayable(")), name
            if name in ("portal_shard", "eye_crystal"):
                assert ".rarity(Rarity.EPIC)" in ctor, name
                assert ".stacksTo(" not in ctor or ".stacksTo(64)" in ctor, name
            else:
                assert not any(x in ctor for x in (".rarity(", ".stacksTo(")), name
            resistant = ".fireResistant()" in ctor
            assert resistant == (carrier == "netherite_ingot"), f"wrong fire-resistant carrier for {name}"
            if resistant:
                fires.append(name)
        assert len(fires) == 5, f"unexpected fire-resistant item count: {fires}"
        assert "new SwordItem(ModToolTiers.ENDIRITIUM" in entries["enderitium_sword"]
        assert "new PickaxeItem(ModToolTiers.ENDIRITIUM" in entries["enderitium_pickaxe"]
        assert "new AxeItem(ModToolTiers.ENDIRITIUM" in entries["enderitium_axe"]
        assert "new ShovelItem(ModToolTiers.ENDIRITIUM" in entries["enderitium_shovel"]
        assert "new HoeItem(ModToolTiers.ENDIRITIUM" in entries["enderitium_hoe"]
        assert "HoeItem.createAttributes(ModToolTiers.ENDIRITIUM, -3.0F, 0.0F)" in entries["enderitium_hoe"]
        assert "new EnderitiumSmithingTemplate" in entries["enderitium_upgrade_smithing_template"]
        print(f"Verified source registrations and vanilla property flags for {len(names)} items (5 fire-resistant)")
    print("PASS: exact 2.2.4 artifact audit")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", required=True, type=Path, help="original 2.2.4 NeoForge JAR")
    parser.add_argument("--decompiled", type=Path, help="decompiled 2.2.4 ModItems.java")
    opts = parser.parse_args()
    verify(opts.jar, opts.decompiled)
