"""Write a synthetic v1-schema archive into the stopped, dedicated fixture world.

This is not captured from the historical DLL. The outer saved-data envelope,
legacy Archive/Record/Slot fields and item-stack NBT follow the Rust source.
The player UUIDs are the native IDs observed in the passing v2 bot world.
"""

import hashlib
import struct
from pathlib import Path


def i32(value):
    return struct.pack(">i", value)


def i64(value):
    return struct.pack(">q", value)


def string(value):
    data = value.encode("ascii")
    return struct.pack(">H", len(data)) + data


def named(tag, name, value):
    return bytes([tag]) + string(name) + value


def compound(*fields):
    return b"".join(fields) + b"\x00"


def root(*fields):
    return named(10, "", compound(*fields))


def list_of(tag, values):
    return bytes([tag]) + i32(len(values)) + b"".join(values)


def byte_array(data):
    return i32(len(data)) + data


def stack(item, count):
    return root(named(8, "id", string(item)),
                named(3, "count", i32(count)),
                named(10, "components", compound()))


def slot(index, item, count):
    return compound(named(3, "index", i32(index)),
                    named(7, "stack", byte_array(stack(item, count))))


def record(record_id, container, owner, x, slots):
    site = compound(
        named(8, "dimension", string("minecraft:overworld")),
        named(9, "position", list_of(3, [i32(x), i32(100), i32(1025)])),
    )
    return compound(
        named(4, "id", i64(record_id)),
        named(8, "container", string(container)),
        named(10, "site", site),
        named(8, "owner", string(owner.replace("-", ""))),
        named(3, "size", i32(27)),
        named(9, "items", list_of(10, slots)),
    )


def main():
    repo = Path(__file__).resolve().parents[2]
    world = repo / "run" / "lootr-migration-fixture" / "world"
    target = world / "moddedmc" / "saved_data.nbt"
    alice = "2bd806c9-7f0e-00af-1a1f-c3328fa763a9"
    bob = "81b637d8-fcd2-c6da-6359-e6963113a117"
    # Check native identity from the fixture setup, rather than guessing from usernames.
    for owner in (alice, bob):
        if not any((world / "players").rglob(f"{owner}.dat")):
            raise SystemExit(f"missing saved fixture player {owner}")
    a = "00000000000000000000000000000001"
    b = "fc000000000000000000000000000001"
    records = [
        record(1, a, alice, 1025, []),
        record(2, b, alice, 1027, [slot(0, "minecraft:diamond", 7)]),
        record(3, a, bob, 1025,
               [slot(i, "minecraft:wooden_sword", 1) for i in range(1, 27)]),
    ]
    # v1 had next_id and records, without v2's bindings or shard manifest.
    archive = root(named(4, "next_id", i64(3)),
                   named(9, "records", list_of(10, records)))
    data = root(named(7, "lootr:inventories", byte_array(archive)))
    # Do not overwrite a real archive or accidentally rerun against an upgraded save.
    with target.open("xb") as output:
        output.write(data)
    print(f"{target}: {len(data)} bytes; SHA256 {hashlib.sha256(data).hexdigest()}")


if __name__ == "__main__":
    main()
