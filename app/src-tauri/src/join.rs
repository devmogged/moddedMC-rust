mod assets;
mod launcher;

use std::{fs, path::Path, time::Duration};

use reqwest::{Client, Url, redirect::Policy};
use tauri::{AppHandle, Manager, State};

use crate::{ping, store::Store};
use launcher::{GAME_VERSION, LOADER, Launcher};

#[derive(Default)]
pub struct JoinLock(tokio::sync::Mutex<()>);

#[tauri::command]
pub async fn friend_join(
    app: AppHandle,
    store: State<'_, Store>,
    lock: State<'_, JoinLock>,
    id: String,
) -> Result<String, String> {
    let _guard = lock
        .0
        .try_lock()
        .map_err(|_| "Another Join is already preparing a launcher.")?;
    let friend = store
        .friends()
        .into_iter()
        .find(|friend| friend.id == id)
        .ok_or("This server is no longer on your Join list.")?;
    let status = ping::ping(friend.address.clone()).await?;
    if status.protocol != Some(777) {
        return Err("This server needs a Minecraft version other than 26.3.".into());
    }

    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| err.to_string())?;
    let jar = companion_jar(&app)?;
    let launcher = launcher::detect_installed();
    let (temporary, launcher) = match launcher {
        Some(launcher) => (None, launcher),
        None => {
            let dir = tempfile::tempdir().map_err(file_error)?;
            let launcher = Launcher::Instance {
                root: dir.path().into(),
                name: "Prism Launcher",
            };
            (Some(dir), launcher)
        }
    };
    let game = launcher.game_dir();
    if let Some(value) = status.moddedmc {
        let (host, _) = ping::split_address(&friend.address)?;
        assets::prefetch(&client, value, &host, &game.join("moddedmc")).await?;
    }

    let profile = if matches!(launcher, Launcher::Official { .. }) {
        let url = Url::parse(&format!(
            "https://meta.fabricmc.net/v2/versions/loader/{GAME_VERSION}/{LOADER}/profile/json"
        ))
        .map_err(|error| error.to_string())?;
        let bytes = assets::get(&client, url, 512 * 1024)
            .await
            .map_err(|err| format!("Could not get the Fabric launcher profile. {err}"))?;
        Some(
            serde_json::from_slice(&bytes)
                .map_err(|err| format!("Invalid Fabric launcher profile. {err}"))?,
        )
    } else {
        None
    };
    let zip = if temporary.is_some() {
        let directory = app
            .path()
            .download_dir()
            .or_else(|_| app.path().app_data_dir())
            .map_err(|error| error.to_string())?;
        Some(directory.join(format!("moddedMC-{}.zip", friend.id)))
    } else {
        None
    };
    tokio::task::spawn_blocking(move || {
        let _temporary = temporary;
        launcher::install(
            &launcher,
            profile.as_ref(),
            &jar,
            &friend.name,
            &friend.address,
        )
        .map_err(file_error)?;
        if let Some(zip) = zip {
            launcher::export(&zip, &game).map_err(file_error)?;
            return Ok(format!(
                "Import {} in Prism Launcher or MultiMC, then press Play.",
                zip.display()
            ));
        }
        Ok(match launcher {
            Launcher::Official { .. } => {
                "Select the moddedMC installation in Minecraft Launcher and press Play.".into()
            }
            Launcher::Instance { name, .. } => {
                format!("Open the moddedMC instance in {name} and press Play.")
            }
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

fn companion_jar(app: &AppHandle) -> Result<Vec<u8>, String> {
    if let Ok(resources) = app.path().resource_dir()
        && let Ok(jar) = fs::read(resources.join("moddedmc-companion.jar"))
    {
        return Ok(jar);
    }
    if cfg!(debug_assertions) {
        let built = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../companion/build/libs/moddedmc-companion-0.1.0.jar");
        if let Ok(jar) = fs::read(built) {
            return Ok(jar);
        }
    }
    Err("The companion is missing from this app build.".into())
}

fn file_error(error: std::io::Error) -> String {
    format!("Could not write the launcher files. {error}")
}
