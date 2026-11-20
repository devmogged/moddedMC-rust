"""Extract exactly the 1.21.1 Farmer's Delight FoodValues reference, never guess unparsed fields.

Usage: py -3 tools/extract_food_values.py [--check]
Requires tests/fixtures/FoodValues.java copied unchanged from upstream dd8b9b0.
This is a source-data fixture, not a runtime 26.3 item-component conversion.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "tests/fixtures/FoodValues.java"
TARGET = ROOT / "food_values.json"
DURATIONS = {"BRIEF_DURATION": 600, "SHORT_DURATION": 1200,
             "MEDIUM_DURATION": 3600, "LONG_DURATION": 6000}
BUILDER = re.compile(
    r"public static final FoodProperties\s+([A-Z_]+)\s*="
    r"\s*\(new FoodProperties\.Builder\(\)\)(.*?)\.build\(\);",
    re.S,
)


def split_args(arg: str) -> list[str]:
    result = []
    depth = 0
    start = 0
    for i, c in enumerate(arg):
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        elif c == "," and depth == 0:
            result.append(arg[start:i].strip())
            start = i + 1
    assert depth == 0, arg
    result.append(arg[start:].strip())
    return result


def scan_calls(expr: str) -> list[tuple[str, str]]:
    calls = []
    position = 0
    while position < len(expr):
        while position < len(expr) and expr[position].isspace():
            position += 1
        if position == len(expr):
            break
        match = re.match(r"\.([A-Za-z][A-Za-z0-9]*)\(", expr[position:])
        if not match:
            raise ValueError(f"unknown food builder property: {expr[position:]!r}")
        name = match.group(1)
        start = position + match.end()
        depth = 1
        cursor = start
        while cursor < len(expr) and depth:
            depth += (expr[cursor] == "(") - (expr[cursor] == ")")
            cursor += 1
        if depth:
            raise ValueError(f"unbalanced builder: {expr!r}")
        calls.append((name, expr[start:cursor - 1].strip()))
        position = cursor
    return calls


def parse_effect(raw: str) -> dict:
    left, chance_literal = split_args(raw)
    chance_literal = chance_literal.strip()
    effect = {
        "chance": float(chance_literal.rstrip("fFdD")),
        "chance_literal": chance_literal,
        "lambda": left,
    }
    match = re.fullmatch(r"\(\)\s*->\s*nourishment\(([A-Z_]+)\)", left)
    if match:
        duration = match.group(1)
        if duration not in DURATIONS:
            raise ValueError(duration)
        effect.update(id="farmersdelight:nourishment",
                      duration_ticks=DURATIONS[duration], amplifier=0,
                      ambient=False, show_particles=False)
        return effect
    match = re.fullmatch(
        r"\(\)\s*->\s*new MobEffectInstance\(MobEffects\.([A-Z_]+),\s*(.*?)\)", left
    )
    if not match:
        raise ValueError(f"unknown effect expression {left!r}")
    args = split_args(match.group(2))
    if len(args) not in (2, 4):
        raise ValueError(f"unhandled MobEffectInstance args {args!r}")
    duration, amplifier = int(args[0]), int(args[1])
    effect.update(id="minecraft:" + match.group(1).lower(),
                  duration_ticks=duration, amplifier=amplifier,
                  ambient=args[2] == "true" if len(args) == 4 else None,
                  show_particles=args[3] == "true" if len(args) == 4 else None)
    return effect


def parse_builder(name: str, expression: str) -> dict:
    profile = {"name": name, "source": "FoodProperties.Builder",
               "nutrition": None, "saturation_modifier": None,
               "saturation_literal": None, "fast": False,
               "always_edible": False, "using_converts_to": None,
               "effects": []}
    seen = set()
    for key, arg in scan_calls(expression):
        if key in ("nutrition", "saturationModifier", "fast", "alwaysEdible",
                   "usingConvertsTo"):
            if key in seen:
                raise ValueError(f"{name}: repeated {key}")
            seen.add(key)
        if key == "nutrition":
            profile["nutrition"] = int(arg)
        elif key == "saturationModifier":
            profile["saturation_modifier"] = float(arg.rstrip("fFdD"))
            profile["saturation_literal"] = arg
        elif key == "fast":
            assert not arg, (name, arg)
            profile["fast"] = True
        elif key == "alwaysEdible":
            assert not arg, (name, arg)
            profile["always_edible"] = True
        elif key == "usingConvertsTo":
            match = re.fullmatch(r"Items\.([A-Z_]+)", arg)
            if not match:
                raise ValueError((name, arg))
            profile["using_converts_to"] = "minecraft:" + match.group(1).lower()
        elif key == "effect":
            profile["effects"].append(parse_effect(arg))
        else:
            raise ValueError(f"{name}: unsupported builder call {key}({arg})")
    return profile


def generate() -> dict:
    source = SOURCE.read_text(encoding="utf-8")
    declarations = re.findall(r"public static final FoodProperties\s+([A-Z_]+)\s*=", source)
    profiles = [parse_builder(name, expr) for name, expr in BUILDER.findall(source)]
    if len(declarations) != 74 or len(profiles) != 73:
        raise ValueError(f"unexpected food declaration counts {len(declarations)}, {len(profiles)}")
    unparsed = set(declarations) - {p["name"] for p in profiles}
    if unparsed != {"KELP_ROLL"}:
        raise ValueError(f"unparsed food values: {unparsed}")
    match = re.search(r"public static final FoodProperties\s+KELP_ROLL\s*=\s*(new FoodProperties\(.*?\));", source)
    if not match:
        raise ValueError("explicit KELP_ROLL constructor missing")
    profiles.append({
        "name": "KELP_ROLL",
        "source": "explicit_constructor",
        "constructor_java": match.group(1),
        "requires_manual_java_1_21_1_record_semantic_review": True,
    })
    soup_map = re.findall(
        r"\.put\(Items\.([A-Z_]+),\s*\(new FoodProperties\.Builder\(\)\)"
        r"(.*?)\.build\(\)\)", source, re.S)
    if len(soup_map) != 3:
        raise ValueError(f"unexpected vanilla soup rules: {len(soup_map)}")
    soups = []
    for name, expr in soup_map:
        entry = parse_builder(name, expr)
        entry["item_id"] = "minecraft:" + name.lower()
        soups.append(entry)
    profiles.sort(key=lambda x: x["name"])
    soups.sort(key=lambda x: x["name"])
    assert len(set(p["name"] for p in profiles)) == 74
    return {
        "upstream_repo": "https://github.com/vectorwing/FarmersDelight",
        "upstream_commit": "dd8b9b0",
        "upstream_game_version": "1.21.1",
        "upstream_mod_version": "1.3.4",
        "reference_only": True,
        "warning": "Java 1.21.1 FoodProperties, not Minecraft 26.3 data components or registered food items.",
        "profiles": profiles,
        "vanilla_soup_overrides": soups,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    output = json.dumps(generate(), indent=2, ensure_ascii=False) + "\n"
    if args.check:
        current = TARGET.read_text(encoding="utf-8")
        if current != output:
            raise SystemExit("FAILED: food_values.json differs from pinned original source")
        print("PASS: all 74 food values and 3 vanilla soup overrides match pinned Java source")
    else:
        TARGET.write_text(output, encoding="utf-8", newline="\n")
        print("GENERATED", TARGET, len(json.loads(output)["profiles"]), "FoodValues; 3 vanilla soup overrides")


if __name__ == "__main__":
    main()
