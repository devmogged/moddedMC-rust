use std::fs;

use serde_json::json;

use super::{Launcher, install, servers};

#[test]
fn official_retries_preserve_existing_records_and_back_up_changed_files() {
    let base = scratch();
    let home = base.path();
    let official = home.join("official");
    let prism = home.join("PrismLauncher");
    fs::create_dir(&official).unwrap();
    fs::create_dir(&prism).unwrap();
    fs::create_dir(prism.join("instances")).unwrap();
    fs::write(prism.join("prismlauncher.cfg"), "").unwrap();
    let profiles = official.join("launcher_profiles.json");
    let original = br#"{"profiles":{"my-world":{"type":"custom","name":"My World"}},"settings":{"keep":true}}"#;
    fs::write(&profiles, original).unwrap();
    let launcher = Launcher::detect(&official, [prism]).unwrap();
    let game = launcher.game_dir();
    fs::create_dir_all(&game).unwrap();
    let old_servers = b"\x0a\0\0\x09\0\x07servers\x0a\0\0\0\x01\x08\0\x04name\0\x03Old\x08\0\x02ip\0\x09localhost\x01\0\x06hidden\0\0\0";
    fs::write(game.join("servers.dat"), old_servers).unwrap();

    let profile = json!({"id":"fabric-loader-0.20.1-26.3","inheritsFrom":"26.3","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient"});
    assert!(install(&launcher, Some(&json!({})), b"companion", "Friend", "host").is_err());
    assert!(!game.join("mods").exists());
    install(
        &launcher,
        Some(&profile),
        b"companion",
        "Friend",
        "host:25565",
    )
    .unwrap();
    install(&launcher, Some(&profile), b"companion", "Friend", "host").unwrap();

    let result: serde_json::Value = serde_json::from_slice(&fs::read(&profiles).unwrap()).unwrap();
    assert_eq!(result["profiles"]["my-world"]["name"], "My World");
    assert_eq!(result["settings"]["keep"], true);
    assert_eq!(
        result["profiles"]["moddedmc-26.3"]["lastVersionId"],
        profile["id"]
    );
    assert_eq!(backups(&profiles), [original.to_vec()]);
    assert_eq!(backups(&game.join("servers.dat")), [old_servers.to_vec()]);

    let bytes = fs::read(game.join("servers.dat")).unwrap();
    let value: fastnbt::Value = fastnbt::from_bytes(&bytes).unwrap();
    let fastnbt::Value::Compound(root) = value else {
        panic!("not a compound")
    };
    let fastnbt::Value::List(list) = &root["servers"] else {
        panic!("not a list")
    };
    assert_eq!(list.len(), 2);
    assert!(servers(Some(b"bad NBT"), "Friend", "host").is_err());
}

#[test]
fn prism_and_zip_have_matching_layouts() {
    let base = scratch();
    let root = base.path();
    let prism = root.join("PrismLauncher");
    fs::create_dir_all(prism.join("instances")).unwrap();
    fs::write(prism.join("prismlauncher.cfg"), "").unwrap();
    let launcher = Launcher::detect(&root.join("missing"), [prism]).unwrap();
    install(&launcher, None, b"companion", "Friend", "example.org").unwrap();
    let game = launcher.game_dir();
    assert!(game.join("mods/moddedmc-companion.jar").is_file());
    assert!(game.parent().unwrap().join("mmc-pack.json").is_file());

    let dest = root.join("join.zip");
    super::export(&dest, &game).unwrap();
    let archive = zip::ZipArchive::new(fs::File::open(dest).unwrap()).unwrap();
    for file in [
        "instance.cfg",
        "mmc-pack.json",
        ".minecraft/servers.dat",
        ".minecraft/mods/moddedmc-companion.jar",
    ] {
        assert!(archive.index_for_name(file).is_some(), "{file}");
    }
}

#[test]
fn multimc_uses_its_instance_folder_and_preserves_extra_components() {
    let base = scratch();
    let root = base.path();
    fs::write(root.join("multimc.cfg"), "InstanceDir=custom\n").unwrap();
    let launcher = Launcher::detect(&root.join("missing"), [root.to_path_buf()]).unwrap();
    assert_eq!(
        launcher.game_dir(),
        root.join("custom/moddedmc-26.3/.minecraft")
    );
    let instance = launcher.game_dir();
    let directory = instance.parent().unwrap();
    fs::create_dir_all(directory).unwrap();
    let path = directory.join("mmc-pack.json");
    fs::write(&path, br#"{"formatVersion":1,"components":[{"uid":"example.extra","version":"2"},{"uid":"net.minecraft","version":"26.2"}]}"#).unwrap();
    install(&launcher, None, b"companion", "Friend", "example.org").unwrap();
    let result: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(result["components"][0]["version"], "2");
    assert_eq!(result["components"][1]["version"], "26.3");
    assert_eq!(result["components"][2]["version"], "0.20.1");
}

fn scratch() -> tempfile::TempDir {
    let run = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../run");
    fs::create_dir_all(&run).unwrap();
    tempfile::Builder::new()
        .prefix("join-test-")
        .tempdir_in(run)
        .unwrap()
}

fn backups(path: &std::path::Path) -> Vec<Vec<u8>> {
    let prefix = format!(".{}.backup-", path.file_name().unwrap().to_str().unwrap());
    fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|file| {
            file.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(&prefix)
        })
        .map(|file| fs::read(file).unwrap())
        .collect()
}
