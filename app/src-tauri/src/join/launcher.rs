use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use fastnbt::Value as Nbt;
use serde_json::{Value, json};
use tempfile::Builder;
use zip::{ZipWriter, write::SimpleFileOptions};

pub const GAME_VERSION: &str = "26.3";
pub const LOADER: &str = "0.20.1";
const PROFILE: &str = "moddedmc-26.3";
const MOD_PATH: &str = "mods/moddedmc-companion.jar";
const MAX_LOCAL_CONFIG: u64 = 16 * 1024 * 1024;

pub enum Launcher {
    Official {
        root: PathBuf,
        profiles: Vec<PathBuf>,
    },
    Instance {
        root: PathBuf,
        name: &'static str,
    },
}

impl Launcher {
    pub fn detect(official: &Path, candidates: impl IntoIterator<Item = PathBuf>) -> Option<Self> {
        let profiles: Vec<_> = [
            "launcher_profiles.json",
            "launcher_profiles_microsoft_store.json",
        ]
        .into_iter()
        .map(|name| official.join(name))
        .filter(|path| path.is_file())
        .collect();
        if !profiles.is_empty() {
            return Some(Self::Official {
                root: official.into(),
                profiles,
            });
        }
        for dir in candidates {
            for (name, config) in [
                ("Prism Launcher", "prismlauncher.cfg"),
                ("MultiMC", "multimc.cfg"),
            ] {
                if let Ok(settings) = fs::read_to_string(dir.join(config)) {
                    let instances = settings
                        .lines()
                        .find_map(|line| line.strip_prefix("InstanceDir="))
                        .filter(|path| !path.trim().is_empty())
                        .unwrap_or("instances");
                    return Some(Self::Instance {
                        root: dir.join(instances).join(PROFILE),
                        name,
                    });
                }
            }
        }
        None
    }

    pub fn game_dir(&self) -> PathBuf {
        match self {
            Self::Official { root, .. } => root.join("moddedmc").join(PROFILE),
            Self::Instance { root, .. } if root.join("minecraft").is_dir() => {
                root.join("minecraft")
            }
            Self::Instance { root, .. } => root.join(".minecraft"),
        }
    }
}

pub fn detect_installed() -> Option<Launcher> {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)?;
    let data = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
    };
    let official = if cfg!(windows) {
        data.join(".minecraft")
    } else if cfg!(target_os = "macos") {
        data.join("minecraft")
    } else {
        home.join(".minecraft")
    };
    // Portable launcher installs can be found via PATH or default Scoop/Flatpak roots.
    let mut candidates = vec![
        data.join("PrismLauncher"),
        home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"),
        home.join("scoop/persist/prismlauncher"),
        data.join("MultiMC"),
        home.join("MultiMC"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path));
    }
    Launcher::detect(&official, candidates)
}

pub fn install(
    launcher: &Launcher,
    loader: Option<&Value>,
    jar: &[u8],
    name: &str,
    address: &str,
) -> io::Result<()> {
    let game = launcher.game_dir();
    let servers_path = game.join("servers.dat");
    let original = read_config(&servers_path)?;
    let entries = servers(original.as_deref(), name, address)?;
    let mut edits = Vec::new();
    match launcher {
        Launcher::Official { root, profiles } => {
            let id = format!("fabric-loader-{LOADER}-{GAME_VERSION}");
            let loader = loader
                .filter(|value| {
                    value["id"] == id
                        && value["inheritsFrom"] == GAME_VERSION
                        && value["mainClass"] == "net.fabricmc.loader.impl.launch.knot.KnotClient"
                })
                .ok_or_else(|| invalid("Unexpected Fabric version profile"))?;
            edits.push((
                root.join("versions").join(&id).join(format!("{id}.json")),
                serde_json::to_vec_pretty(loader)?,
            ));
            for path in profiles {
                let original = read_config(path)?
                    .ok_or_else(|| invalid("Launcher profile file disappeared"))?;
                edits.push((path.clone(), official_profile(&original, &game)?));
            }
        }
        Launcher::Instance { root, .. } => {
            let pack = root.join("mmc-pack.json");
            let components = instance_profile(read_config(&pack)?.as_deref())?;
            edits.push((pack, components));
            let config = root.join("instance.cfg");
            if !config.is_file() {
                edits.push((config, b"InstanceType=OneSix\nname=moddedMC\n".to_vec()));
            }
        }
    }
    write(&game.join(MOD_PATH), jar)?;
    write(&servers_path, &entries)?;
    for (path, updated) in edits {
        write(&path, &updated)?;
    }
    Ok(())
}

fn instance_profile(original: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let mut data: Value = match original {
        Some(bytes) => serde_json::from_slice(bytes).map_err(invalid)?,
        None => json!({"formatVersion":1, "components":[]}),
    };
    let components = data
        .get_mut("components")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("Launcher JSON is missing its components list"))?;
    for (id, version) in [
        ("net.minecraft", GAME_VERSION),
        ("net.fabricmc.fabric-loader", LOADER),
    ] {
        if let Some(component) = components
            .iter_mut()
            .find(|component| component["uid"] == id)
        {
            component["version"] = json!(version);
        } else {
            let mut component = json!({"uid":id, "version":version});
            if id == "net.minecraft" {
                component["important"] = json!(true);
            }
            components.push(component);
        }
    }
    serde_json::to_vec_pretty(&data).map_err(invalid)
}

fn official_profile(original: &[u8], game: &Path) -> io::Result<Vec<u8>> {
    let mut data: Value = serde_json::from_slice(original).map_err(invalid)?;
    let profiles = data
        .get_mut("profiles")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("Launcher JSON is missing its profiles object"))?;
    let entry = profiles
        .entry(PROFILE)
        .or_insert_with(|| json!({"name":"moddedMC", "type":"custom"}));
    let entry = entry
        .as_object_mut()
        .ok_or_else(|| invalid("Invalid moddedMC installation profile"))?;
    let dir = game
        .to_str()
        .ok_or_else(|| invalid("Launcher path is not Unicode"))?;
    let previous_dir = entry.get("gameDir").and_then(Value::as_str);
    if previous_dir.is_some_and(|existing| Path::new(existing) != game) {
        return Err(invalid(
            "The moddedMC installation uses a different game directory",
        ));
    }
    entry.insert("gameDir".into(), json!(dir));
    entry.insert(
        "lastVersionId".into(),
        json!(format!("fabric-loader-{LOADER}-{GAME_VERSION}")),
    );
    serde_json::to_vec_pretty(&data).map_err(invalid)
}

fn servers(original: Option<&[u8]>, name: &str, address: &str) -> io::Result<Vec<u8>> {
    let mut root: HashMap<String, Nbt> = match original {
        Some(bytes) => {
            fastnbt::from_bytes_with_opts(bytes, fastnbt::DeOpts::new().max_seq_len(1 << 20))
                .map_err(invalid)?
        }
        None => HashMap::new(),
    };
    let list = root
        .entry("servers".into())
        .or_insert_with(|| Nbt::List(Vec::new()));
    let Nbt::List(list) = list else {
        return Err(invalid("servers.dat has no server list"));
    };
    let (host, port) = crate::ping::split_address(address).map_err(invalid)?;
    for entry in list.iter_mut() {
        let Nbt::Compound(entry) = entry else {
            return Err(invalid("Unexpected servers.dat entry"));
        };
        let Some(Nbt::String(ip)) = entry.get("ip") else {
            continue;
        };
        if crate::ping::split_address(ip).is_ok_and(|(old_host, old_port)| {
            old_host.eq_ignore_ascii_case(&host) && old_port == port
        }) {
            let same_name =
                matches!(entry.get("name"), Some(Nbt::String(old_name)) if old_name == name);
            let hidden = matches!(entry.get("hidden"), Some(Nbt::Byte(value)) if *value != 0);
            if same_name && !hidden {
                return Ok(original.unwrap_or_default().to_vec());
            }
            entry.insert("name".into(), Nbt::String(name.into()));
            entry.insert("hidden".into(), Nbt::Byte(0));
            return fastnbt::to_bytes(&root).map_err(invalid);
        }
    }
    list.push(Nbt::Compound(HashMap::from([
        ("name".into(), Nbt::String(name.into())),
        ("ip".into(), Nbt::String(address.into())),
        ("hidden".into(), Nbt::Byte(0)),
    ])));
    fastnbt::to_bytes(&root).map_err(invalid)
}

fn read_config(path: &Path) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read as _;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_LOCAL_CONFIG + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_LOCAL_CONFIG {
        return Err(invalid("Launcher file is too large"));
    }
    Ok(Some(bytes))
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() == bytes.len() as u64)
        && fs::read(path)? == bytes
    {
        return Ok(());
    }
    let mut temp = temporary(path)?;
    temp.write_all(bytes)?;
    publish(temp, path)
}

fn temporary(path: &Path) -> io::Result<tempfile::NamedTempFile> {
    let dir = path
        .parent()
        .ok_or_else(|| invalid("Invalid destination path"))?;
    fs::create_dir_all(dir)?;
    Builder::new().prefix(".moddedmc-").tempfile_in(dir)
}

fn publish(temp: tempfile::NamedTempFile, path: &Path) -> io::Result<()> {
    temp.as_file().sync_all()?;
    if path.exists() {
        let dir = path
            .parent()
            .ok_or_else(|| invalid("Invalid destination path"))?;
        let name = path
            .file_name()
            .ok_or_else(|| invalid("Invalid destination name"))?
            .to_string_lossy();
        let mut backup = Builder::new()
            .prefix(&format!(".{name}.backup-"))
            .tempfile_in(dir)?;
        io::copy(&mut File::open(path)?, &mut backup)?;
        backup.as_file().sync_all()?;
        backup.keep().map_err(|error| error.error)?;
    }
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn export(dest: &Path, game: &Path) -> io::Result<()> {
    let instance = game
        .parent()
        .ok_or_else(|| invalid("Invalid instance folder"))?;
    let mut temp = temporary(dest)?;
    let mut zip = ZipWriter::new(temp.as_file_mut());
    let mut add = |name: &str, file: &Path| -> io::Result<()> {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )?;
        io::copy(&mut File::open(file)?, &mut zip)?;
        Ok(())
    };
    for name in ["instance.cfg", "mmc-pack.json"] {
        add(name, &instance.join(name))?;
    }
    for name in [MOD_PATH, "servers.dat"] {
        add(&format!(".minecraft/{name}"), &game.join(name))?;
    }
    for subdir in ["moddedmc", "moddedmc/assets"] {
        let dir = game.join(subdir);
        if dir.is_dir() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    add(
                        &format!(
                            ".minecraft/{subdir}/{}",
                            entry.file_name().to_string_lossy()
                        ),
                        &entry.path(),
                    )?;
                }
            }
        }
    }
    zip.finish()?;
    publish(temp, dest)
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
#[path = "launcher_tests.rs"]
mod tests;
