# Mods

## For server owners

Put mod files in your server folder's `mods/` and start the server. That is all.

```
myserver/
  mods/
    wayshrine.mmc     a Rust mod
    sethome.jar       an unchanged Fabric mod
```

- **`.mmc`**: a moddedMC mod. One file holds builds for every platform the author packed; the server picks the one for your machine. It loads in milliseconds.
- **`.jar`**: a Fabric mod. The first start with a new jar converts it to native code, which takes a few minutes and needs the converter tools (see [recompiler/README.md](../recompiler/README.md)). The result is cached in `mods/.cache/`, so later starts are instant. Most Fabric mods use Mixin and cannot be converted yet; the server logs why a jar was skipped.
- To remove a mod, delete its file and restart. `mods/.cache/` can be deleted at any time; it is rebuilt.
- A mod's own files live in `config/<mod id>/`. Fabric mods use `config/` as on a Fabric server.
- The server log shows each mod it loaded (`moddedmc: loaded wayshrine 0.2.0 from mods\wayshrine.mmc`) and why any mod was skipped. One broken mod never stops the others or the server.

A mod built for this server's mod ABI (currently **1**) runs on any server version with the same ABI. Mods are trusted code: only install mods you trust, as with any Minecraft server plugin.

## For mod authors (Rust)

A mod is a `cdylib` crate that depends on `moddedmc-api` (in `api/`) and implements `moddedmc_api::Mod`.

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
moddedmc-api = { path = "../../api" }
```

```rust
use moddedmc_api::{Command, Mod, RegistrationError, Registry};

struct Hello;

impl Mod for Hello {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("hello", "Says hello", 0)
    }

    fn command(&self, _tag: u64, command: Command<'_>) -> i32 {
        command.reply("Hello!");
        1
    }
}

moddedmc_api::export_mod!(Hello);
```

Build and pack it:

```powershell
scripts\pack-mod.ps1 -Crate hello    # -> target\mods\hello.mmc, ready for mods/
```

What the SDK offers:

| | |
| --- | --- |
| `Mod::register` | Once, on the first server tick: `add_block_entity`, `add_block`, `add_command`. |
| `Mod::tick` | Due block entities of one dimension; `set_state(index, bytes)` saves a new state. |
| `Mod::use_block`, `place_block`, `break_block` | A player used, placed or broke one of your blocks. |
| `Mod::command` | One of your commands ran. `reply` answers the player or the console. |
| `Mod::menu_click` | A player picked a button of a menu (`Player::show_menu`). Every mod sees every click: check the action id is yours. |
| `Player` | `name`, `dimension`, `position`, `send_message`, `action_bar`, `give_block`, `teleport`, `show_menu`. Valid only during the call that handed it to you; store the name, not the player. |
| free functions | `info`, `warn`, `error` (server log), `data_dir()` (`config/<id>/`), `tick_count()`, `dimensions()`. |

Rules:

- The mod id is the package name (or `export_mod!(M, id = "...")`): lowercase letters, digits, `_`, `-`, `.`. It names the data folder. Block ids are `namespace:path` and may use any namespace except `minecraft`.
- Calls can come from several threads at once, so the mod type must be `Send + Sync`.
- A panic is caught by the SDK, logged, and treated as "did nothing". It never crashes the server.
- Block entity state is opaque bytes, at most 64 KiB, saved with the chunk. Bump `state_version` when its layout changes; the server will not hand old-layout state to the new code.
- You do not need the server's source, compiler version or build to build a mod. Any Rust toolchain that builds `api/` works.

`mods/counter` (a ticking block) and `mods/wayshrine` (blocks, a menu, teleport, saved data) are complete examples.

## The ABI

The contract between the server and a mod library is plain C: `#[repr(C)]` structs and `extern "C"` functions, defined in [`api/src/abi.rs`](../api/src/abi.rs). Any language that can build a shared library with C exports can implement it; the Rust SDK is one wrapper over it.

A mod library exports three symbols:

| Symbol | Type | |
| --- | --- | --- |
| `MODDEDMC_ABI` | `u32` | Major ABI version. The server refuses a library whose version differs. |
| `MODDEDMC_ID` | `Str` | The mod id. Read before the mod starts, so the server can create its data folder. |
| `moddedmc_mod` | `extern "C" fn(&'static HostApi) -> *const ModApi` | Starts the mod. Returns null to refuse (wrong host, failed start). |

`HostApi` is the server's table of functions (log, registration, player actions, ticks, dimensions); `ModApi` is the mod's table of callbacks. Both live until the process exits.

Ownership and lifetimes:

- `Str` is a borrowed pointer and length, valid only for the call it is passed in, unless documented as static (mod id and version, data folder, dimension names).
- A `PlayerRef` is valid only during the call that passed it. Null means "no player".
- Bytes going back to the server (new states, messages) go through a `Sink`; the server copies them before `put` returns.
- Neither side frees memory the other allocated. Each library has its own allocator and standard library; that is fine because nothing allocated crosses.

Versioning:

- `ABI_VERSION` changes only for breaking changes (a changed signature, a removed function, different meaning). Old mods then need a rebuild.
- New host functions and new mod callbacks are appended to the end of their table, and `size` (the table's byte size) grows. The SDK reads the original ABI 1 prefix on any ABI 1 host. A mod that requires an unavailable extension refuses to start.

## Interfaces

`HostApi::get_interface(name, version)` returns a process-lifetime table for the exact namespaced name and version, or null. Each table is `#[repr(C)]`, begins with its byte size (`usize`), and uses C ABI functions. Append fields without changing existing offsets or behavior. A breaking change gets a new interface version, and the host can serve both versions.

The Rust SDK checks the host's size before reading `get_interface` and checks the returned table's size before using its functions. Require interfaces in `Mod::start` to refuse startup with a log line naming the missing interface.

Packaging (`.mmc`): a zip with

- `mod.json`: `{"id": "wayshrine", "version": "0.2.0", "abi": 1}`
- one library per platform, named `<os>-<arch>.<ext>`: `windows-x86_64.dll`, `windows-aarch64.dll`, `linux-x86_64.so`, `linux-aarch64.so`, `macos-aarch64.dylib`, `macos-x86_64.dylib`.

`scripts/pack-mod.ps1` writes the manifest and the build for the current machine. Builds for other platforms (from CI or cross-compiling) go into the same zip under their names.
