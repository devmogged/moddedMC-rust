"""Verify that a Buja mods.zip archive and loose mods/ JARs are identical by SHA-256.

No JAR bytes are copied, modified, stored or committed. Results can be
reproduced on any machine given the same source folder and archive.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import zipfile


def digest(stream) -> str:
    sha = hashlib.sha256()
    while True:
        data = stream.read(1024 * 1024)
        if not data:
            break
        sha.update(data)
    return sha.hexdigest()


def verify(root: Path, expected_count: int) -> dict:
    loose_dir = root / "mods"
    archive = root / "mods.zip"
    loose = {p.name.casefold(): p for p in loose_dir.glob("*.jar") if p.is_file()}
    if len(loose) != expected_count:
        raise ValueError(f"expected {expected_count} loose JARs, found {len(loose)}")

    matches = []
    with zipfile.ZipFile(archive) as zf:
        zipped = {
            PurePosixPath(info.filename).name.casefold(): info
            for info in zf.infolist()
            if not info.is_dir() and info.filename.lower().endswith(".jar")
        }
        if len(zipped) != expected_count:
            raise ValueError(f"expected {expected_count} zipped JARs, found {len(zipped)}")
        if set(loose) != set(zipped):
            raise ValueError(
                f"mismatched names: archive-only {set(zipped)-set(loose)}, "
                f"loose-only {set(loose)-set(zipped)}"
            )
        for name in sorted(loose):
            source = loose[name]
            entry = zipped[name]
            if entry.file_size != source.stat().st_size:
                raise ValueError(f"byte-length mismatch in {name}")
            with source.open("rb") as f:
                loose_hash = digest(f)
            with zf.open(entry) as f:
                archived_hash = digest(f)
            if loose_hash != archived_hash:
                raise ValueError(f"SHA-256 content mismatch in {name}")
            matches.append({"filename": source.name, "sha256": loose_hash, "bytes": entry.file_size})

    index = "".join(f"{e['filename'].casefold()}\t{e['sha256']}\n" for e in matches)
    fingerprint = hashlib.sha256(index.encode("utf-8")).hexdigest()
    with archive.open("rb") as stream:
        archive_sha256 = digest(stream)
    return {
        "archive": str(archive),
        "loose_dir": str(loose_dir),
        "count": len(matches),
        "index_sha256": fingerprint,
        "zip_sha256": archive_sha256,
        "files": matches,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("server_root", type=Path)
    parser.add_argument("--expected-count", type=int, default=65)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = verify(args.server_root, args.expected_count)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"PASS: {report['count']} JARs have identical bytes in mods.zip and mods/")
    print(f"PACK_INDEX_SHA256={report['index_sha256']}")
    print(f"ZIP_SHA256={report['zip_sha256']}")


if __name__ == "__main__":
    main()
