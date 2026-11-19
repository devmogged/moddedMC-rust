use std::{collections::HashSet, net::Ipv6Addr, path::Path};

use reqwest::{Client, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{file_error, launcher};

const MAX_MANIFEST: usize = 2 * 1024 * 1024;
const MAX_ASSET: usize = 8 * 1024 * 1024;
const MAX_PACK: usize = 256 * 1024 * 1024;

#[derive(Deserialize)]
struct Advert {
    protocol: u16,
    pack_url: String,
    manifest_sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    pack_version: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    path: String,
    sha256: String,
    size: usize,
}

pub async fn prefetch(
    client: &Client,
    value: serde_json::Value,
    host: &str,
    cache: &Path,
) -> Result<(), String> {
    let advert: Advert =
        serde_json::from_value(value).map_err(|_| "Invalid asset advertisement.")?;
    if advert.protocol != 1 || !hash_string(&advert.manifest_sha256) {
        return Err("Unsupported server asset protocol.".into());
    }
    let host = if host.parse::<Ipv6Addr>().is_ok() {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let base =
        Url::parse(&advert.pack_url.replace("{host}", &host)).map_err(|_| "Invalid pack URL.")?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.cannot_be_a_base()
    {
        return Err("Invalid pack URL.".into());
    }
    let key = format!("manifest-{}.json", advert.manifest_sha256);
    let path = cache.join(&key);
    let manifest_bytes = if let Some(bytes) = cached(&path, &advert.manifest_sha256, MAX_MANIFEST) {
        bytes
    } else {
        get(
            client,
            base.join(&key).map_err(|err| err.to_string())?,
            MAX_MANIFEST,
        )
        .await?
    };
    if !verified(&manifest_bytes, &advert.manifest_sha256) {
        return Err("Pack manifest hash mismatch.".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| "Invalid pack manifest.")?;
    if !hash_string(&manifest.pack_version) || manifest.assets.len() > 10_000 {
        return Err("Invalid pack manifest.".into());
    }
    let mut paths = HashSet::new();
    let mut total = 0;
    for asset in &manifest.assets {
        if !valid_path(&asset.path)
            || !paths.insert(asset.path.as_str())
            || !hash_string(&asset.sha256)
            || asset.size > MAX_ASSET
        {
            return Err("Invalid asset in the pack manifest.".into());
        }
        total += asset.size;
        if total > MAX_PACK {
            return Err("The asset pack is too large.".into());
        }
    }
    if !paths.contains("pack.mcmeta") {
        return Err("The pack has no metadata.".into());
    }

    for asset in manifest.assets {
        let key = format!("assets/{}", asset.sha256);
        let path = cache.join(&key);
        if cached(&path, &asset.sha256, asset.size).is_some_and(|bytes| bytes.len() == asset.size) {
            continue;
        }
        let bytes = get(
            client,
            base.join(&key).map_err(|err| err.to_string())?,
            asset.size,
        )
        .await?;
        if bytes.len() != asset.size || !verified(&bytes, &asset.sha256) {
            return Err(format!("Pack asset failed verification. {}", asset.path));
        }
        launcher::write(&path, &bytes).map_err(file_error)?;
    }
    launcher::write(&path, &manifest_bytes).map_err(file_error)
}

pub async fn get(client: &Client, url: Url, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "Could not reach the download server.")?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("Download server returned {}.", response.status()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Download was interrupted.")?
    {
        if bytes.len() + chunk.len() > limit {
            return Err("Download exceeded its allowed size.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn cached(path: &Path, hash: &str, limit: usize) -> Option<Vec<u8>> {
    use std::{fs::File, io::Read as _};
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > limit as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit && verified(&bytes, hash)).then_some(bytes)
}

fn verified(bytes: &[u8], hash: &str) -> bool {
    format!("{:x}", Sha256::digest(bytes)) == hash
}

fn hash_string(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_path(path: &str) -> bool {
    path == "pack.mcmeta"
        || (path.len() <= 512
            && path.starts_with("assets/")
            && path.split('/').all(|part| {
                !matches!(part, "" | "." | "..")
                    && part.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
                    })
            }))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        path::Path,
        time::Duration,
    };

    use serde_json::json;
    use sha2::{Digest, Sha256};

    use super::prefetch;

    #[tokio::test]
    async fn prefetches_once_then_uses_verified_cache_offline() {
        let run = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../run");
        fs::create_dir_all(&run).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("join-assets-")
            .tempdir_in(run)
            .unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let blob = b"{}";
        let hash = format!("{:x}", Sha256::digest(blob));
        let manifest = serde_json::to_vec(&json!({
            "pack_version":"0".repeat(64),
            "assets":[{"path":"pack.mcmeta","sha256":hash,"size":blob.len()}]
        }))
        .unwrap();
        let manifest_sha256 = format!("{:x}", Sha256::digest(&manifest));
        let advertise = |port| {
            json!({
                "protocol":1,
                "pack_url":format!("http://{{host}}:{port}/pack.zip"),
                "manifest_sha256":manifest_sha256,
            })
        };
        let responses = [
            (
                format!("/manifest-{manifest_sha256}.json"),
                manifest.clone(),
            ),
            (format!("/assets/{hash}"), blob.to_vec()),
        ];
        let worker = std::thread::spawn(move || {
            for (path, bytes) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with(&format!("GET {path} ")), "{line}");
                while !line.is_empty() && line != "\r\n" {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .unwrap();
                stream.write_all(&bytes).unwrap();
            }
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .build()
            .unwrap();
        prefetch(&client, advertise(port), "127.0.0.1", dir.path())
            .await
            .unwrap();
        worker.join().unwrap();
        assert_eq!(
            fs::read(dir.path().join(format!("assets/{hash}"))).unwrap(),
            blob
        );
        assert_eq!(
            fs::read(dir.path().join(format!("manifest-{manifest_sha256}.json"))).unwrap(),
            manifest
        );
        prefetch(&client, advertise(0), "127.0.0.1", dir.path())
            .await
            .unwrap();
    }
}
