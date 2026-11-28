use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
    fs::{self, File},
    io::{self, Cursor, Read, Write},
    net::{SocketAddr, TcpListener},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use axum::{Router, extract::Path, http::StatusCode, routing::get};
use bytes::Bytes;
use pumpkin_config::resource_pack::JavaResourcePackConfig;
use pumpkin_data::{
    Block, BlockStateId, item::Item, item_stack::ItemStack, packet::CURRENT_MC_VERSION,
};
use pumpkin_protocol::{
    VarInt, codec::item_stack_seralizer::ItemStackSerializer, ser::NetworkWriteExt,
};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::content_panes;
use crate::content_walls::{self, WallSelector};

const EMPTY_MODEL: &str = "moddedmc:block/empty";
const PLACEHOLDER: &str = "moddedmc:block/placeholder";
const CREATIVE_CATALOG: &str = "assets/moddedmc/moddedmc/creative-v1.bin";
const MAX_CREATIVE_BYTES: usize = 512 * 1024;
const MAX_CREATIVE_ENTRIES: usize = 4096;
const MAX_ASSET: u64 = 8 * 1024 * 1024;

pub static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Folders holding a mod's own `assets/`, checked before local jars.
static ASSET_ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

pub fn add_asset_root(root: PathBuf) {
    ASSET_ROOTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(root);
}

pub struct Catalog {
    pub manifest: [u8; 32],
    pub zip_sha1: [u8; 20],
    pub zip_url: String,
}

type CatalogFiles = (Vec<u8>, HashMap<String, Bytes>);

pub struct BlockAssets {
    pub id: String,
    pub item_model: String,
    pub states: Vec<StateAssets>,
}

pub struct ItemAssets {
    pub id: String,
    pub model: String,
}

pub struct StateAssets {
    pub model: String,
    pub rotating_model: String,
    pub x: i16,
    pub y: i16,
    pub orientation: [f32; 4],
    pub bounded: bool,
    pub uvlock: bool,
    pub straight_stair_display: bool,
    pub block_display_carrier: Option<BlockStateId>,
    pub scaffolding: Option<bool>,
    pub wall: Option<WallSelector>,
    pub pane: bool,
}

pub struct OverlayModel {
    pub id: String,
    pub orientation: [f32; 4],
    pub bounded: bool,
    pub wall: Option<WallSelector>,
}

pub fn valid_model(name: &str) -> bool {
    name.is_empty()
        || name.split_once(':').is_some_and(|(namespace, path)| {
            const fn byte(c: u8) -> bool {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.')
            }
            !matches!(namespace, "" | "." | "..")
                && namespace.bytes().all(byte)
                && path.split('/').all(|piece| {
                    !piece.is_empty() && piece != "." && piece != ".." && piece.bytes().all(byte)
                })
        })
}

pub fn configure(
    address: &SocketAddr,
    config: &mut JavaResourcePackConfig,
    blocks: &[BlockAssets],
    items: &[ItemAssets],
) -> io::Result<HashMap<u16, OverlayModel>> {
    if blocks.is_empty() && items.is_empty() {
        return Ok(HashMap::new());
    }
    let (pack, display_models) = build_pack(blocks, items)?;
    let bytes = pack.zip()?;
    let zip_sha1: [u8; 20] = Sha1::digest(&bytes).into();
    let pack_version = hex(&Sha256::digest(&bytes));
    let hash = hex(&zip_sha1);
    fs::create_dir_all("cache/moddedmc")?;
    fs::write(format!("cache/moddedmc/resource-pack-{hash}.zip"), &bytes)?;

    let default_bind = SocketAddr::new(
        address.ip(),
        address
            .port()
            .checked_add(1)
            .ok_or_else(|| io::Error::other("no adjacent resource pack port"))?,
    );
    let bind = std::env::var("MODDEDMC_PACK_BIND").map_or(Ok(default_bind), |value| {
        value.parse().map_err(io::Error::other)
    })?;
    let listener = TcpListener::bind(bind)?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let path = format!("/resource-pack-{hash}.zip");
    let zip = Bytes::from(bytes);
    let mut app = Router::new().route(
        &path,
        get(move || async move { ([("content-type", "application/zip")], zip) }),
    );
    let zip_url = std::env::var("MODDEDMC_PACK_URL")
        .unwrap_or_else(|_| format!("http://{{host}}:{port}{path}"));
    if let Some((manifest, blobs)) = pack.catalog(&pack_version)? {
        let manifest_hash: [u8; 32] = Sha256::digest(&manifest).into();
        let manifest = Bytes::from(manifest);
        let blobs = Arc::new(blobs);
        app = app
            .route(
                &format!("/manifest-{}.json", hex(&manifest_hash)),
                get(move || async move { ([("content-type", "application/json")], manifest) }),
            )
            .route(
                "/assets/{hash}",
                get(move |Path(hash): Path<String>| async move {
                    blobs.get(&hash).cloned().ok_or(StatusCode::NOT_FOUND)
                }),
            );
        let _ = CATALOG.set(Catalog {
            manifest: manifest_hash,
            zip_sha1,
            zip_url: zip_url.clone(),
        });
    }
    let listener = tokio::net::TcpListener::from_std(listener)?;
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!("moddedmc: resource pack HTTP server stopped: {error}");
        }
    });
    config.enabled = true;
    config.force = true;
    config.url = zip_url;
    config.sha1 = hash;
    tracing::info!(
        "moddedmc: serving resource pack on port {port} (SHA-1 {})",
        config.sha1
    );
    Ok(display_models)
}

pub fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

fn build_pack(
    blocks: &[BlockAssets],
    items: &[ItemAssets],
) -> io::Result<(Pack, HashMap<u16, OverlayModel>)> {
    let mut pack = Pack::default();
    pack.json(
        "pack.mcmeta",
        &json!({"pack": {
            "description": "moddedMC content",
            "min_format": [97, 1],
            "max_format": [97, 1],
        }}),
    )?;
    pack.fallback_models()?;

    let mut carrier_variants = CarrierVariants::new();
    let mut display_models = HashMap::new();
    pack.namespace_assets(blocks, items)?;
    pack.action_translations();
    pack.standalone_items(items)?;
    for block in blocks {
        let Some(registered) = Block::from_name(&block.id) else {
            continue;
        };
        let Some((namespace, path)) = block.id.split_once(':') else {
            continue;
        };
        pack.item(
            &format!("assets/{namespace}/items/{path}.json"),
            &block.item_model,
        )?;
        if let Some(first) = block.states.first().filter(|assets| assets.wall.is_some()) {
            let id = pack.wall_item(namespace, path, &first.model)?;
            extend_wall_overlays(&mut display_models, registered, &block.states, &id);
            continue;
        }
        if block.states.first().is_some_and(|assets| assets.pane) {
            let carrier = registered.default_state.id.vanilla_view().to_block();
            if let Some(ids) = pack.pane_models(namespace, path, carrier)? {
                extend_pane_overlays(&mut display_models, registered, &ids);
            }
            continue;
        }
        for (index, (state, assets)) in registered.states.iter().zip(&block.states).enumerate() {
            if assets.model.is_empty() && assets.rotating_model.is_empty() {
                continue;
            }
            let carrier = state.id.vanilla_view();
            let block = carrier.to_block();
            let Some(key) = block_variant(block, carrier) else {
                continue;
            };
            let model = if assets.model.is_empty() {
                EMPTY_MODEL
            } else {
                pack.model(&assets.model)
            };
            record_carrier_model(&mut carrier_variants, block, key, model, assets);
            if !assets.rotating_model.is_empty() {
                if let Some(carrier) = assets.block_display_carrier
                    && let Some(key) = block_variant(carrier.to_block(), carrier)
                {
                    let model = pack.model(&assets.rotating_model);
                    record_carrier_model(
                        &mut carrier_variants,
                        carrier.to_block(),
                        key,
                        model,
                        assets,
                    );
                }
                let model_id = format!("{namespace}:moddedmc/display/{path}/{index}");
                let item_path =
                    format!("assets/{namespace}/items/moddedmc/display/{path}/{index}.json");
                let orientation = if assets.straight_stair_display
                    && pack.oriented_stair_item(&item_path, &model_id, assets)?
                {
                    [0.0, 0.0, 0.0, 1.0]
                } else {
                    pack.item(&item_path, &assets.rotating_model)?;
                    assets.orientation
                };
                display_models.insert(
                    state.id.as_u16(),
                    OverlayModel {
                        id: model_id,
                        orientation,
                        bounded: assets.bounded,
                        wall: None,
                    },
                );
            }
        }
    }
    pack.scaffolding_catalog(blocks)?;
    for (name, variants) in carrier_variants {
        if let Some(block) = Block::from_name(name) {
            pack.carrier_variants(block, variants)?;
        }
    }
    if pack.missing {
        tracing::warn!("moddedmc: some content pack assets are missing; using a placeholder");
    }
    Ok((pack, display_models))
}

fn extend_pane_overlays(
    display_models: &mut HashMap<u16, OverlayModel>,
    block: &Block,
    ids: &[String; 16],
) {
    display_models.extend(block.states.iter().map(|state| {
        let mask = usize::from(content_panes::side_mask(state.id.vanilla_view()));
        (
            state.id.as_u16(),
            OverlayModel {
                id: ids[mask].clone(),
                orientation: [0.0, 0.0, 0.0, 1.0],
                bounded: true,
                wall: None,
            },
        )
    }));
}

fn extend_wall_overlays(
    models: &mut HashMap<u16, OverlayModel>,
    block: &Block,
    assets: &[StateAssets],
    id: &str,
) {
    models.extend(block.states.iter().zip(assets).map(|(state, assets)| {
        (
            state.id.as_u16(),
            OverlayModel {
                id: id.to_owned(),
                orientation: [0.0, 0.0, 0.0, 1.0],
                bounded: true,
                wall: assets.wall,
            },
        )
    }));
}

fn encode_creative_catalog(items: &[&'static Item]) -> io::Result<Option<Vec<u8>>> {
    if items.is_empty() {
        return Ok(None);
    }
    if items.len() > MAX_CREATIVE_ENTRIES {
        tracing::warn!("moddedmc: creative catalog exceeds the item limit");
        return Ok(None);
    }
    let mut bytes = Vec::new();
    bytes
        .write_var_int(&VarInt(
            i32::try_from(items.len()).map_err(io::Error::other)?,
        ))
        .map_err(io::Error::other)?;
    let mut entry = Vec::new();
    for item in items {
        let stack = ItemStack::new(1, item);
        entry.clear();
        ItemStackSerializer(Cow::Borrowed(&stack))
            .write_untrusted_with_version(&mut entry, &CURRENT_MC_VERSION)
            .map_err(io::Error::other)?;
        bytes
            .write_var_int(&VarInt(
                i32::try_from(entry.len()).map_err(io::Error::other)?,
            ))
            .map_err(io::Error::other)?;
        bytes.extend_from_slice(&entry);
        if bytes.len() > MAX_CREATIVE_BYTES {
            tracing::warn!("moddedmc: creative catalog exceeds the byte limit");
            return Ok(None);
        }
    }
    Ok(Some(bytes))
}

fn block_variant(block: &Block, state: pumpkin_data::BlockStateId) -> Option<String> {
    let properties = block.properties(state)?;
    Some(
        properties
            .to_props()
            .iter()
            .map(|prop| format!("{}={}", prop.key_str(), prop.value_str()))
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn stock_carrier_model(block: &Block, state: pumpkin_data::BlockStateId, variant: &str) -> Value {
    if let Some(model) = crate::content_slab::carrier_model(block, variant) {
        return json!({"model": model});
    }
    if let Some((model, x, y)) = crate::content_stairs::carrier_model(block, state) {
        return json!({"model": model, "x": x, "y": y, "uvlock": x != 0 || y != 0});
    }
    if block == &Block::BEEHIVE || block == &Block::BEE_NEST {
        let mut honey = false;
        let mut rotation = 0;
        for prop in variant.split(',') {
            match prop {
                "honey_level=5" => honey = true,
                "facing=east" => rotation = 90,
                "facing=south" => rotation = 180,
                "facing=west" => rotation = 270,
                _ => {}
            }
        }
        let fill = if honey { "honey" } else { "empty" };
        json!({"model": format!("minecraft:block/{}_{fill}", block.name), "y": rotation})
    } else {
        // Note blocks and targets each draw the same model at every power value.
        json!({"model": format!("minecraft:block/{}", block.name)})
    }
}

#[derive(Default)]
struct Pack {
    files: BTreeMap<String, Vec<u8>>,
    jars: HashMap<String, Option<ZipArchive<File>>>,
    missing: bool,
    visited: HashSet<String>,
}

type CarrierVariants = BTreeMap<&'static str, BTreeMap<String, Value>>;

fn record_carrier_model(
    variants: &mut CarrierVariants,
    block: &Block,
    key: String,
    model: &str,
    assets: &StateAssets,
) {
    variants.entry(block.name).or_default().insert(
        key,
        json!({
            "model": model,
            "x": assets.x.rem_euclid(360),
            "y": assets.y.rem_euclid(360),
            "uvlock": assets.uvlock,
        }),
    );
}

impl Pack {
    fn fallback_models(&mut self) -> io::Result<()> {
        self.json(
            "assets/moddedmc/models/block/empty.json",
            &json!({"elements": []}),
        )?;
        self.json(
            "assets/moddedmc/models/block/placeholder.json",
            &json!({
                "parent": "minecraft:block/cube_all",
                "textures": {"all": "moddedmc:block/placeholder"}
            }),
        )?;
        self.files.insert(
            "assets/moddedmc/textures/block/placeholder.png".to_owned(),
            PLACEHOLDER_PNG.to_vec(),
        );
        Ok(())
    }

    fn pane_models(
        &mut self,
        namespace: &str,
        path: &str,
        carrier: &Block,
    ) -> io::Result<Option<[String; 16]>> {
        let key = format!("assets/{namespace}/blockstates/{path}.json");
        let parts = self.asset(namespace, &key).and_then(|bytes| {
            let root: Value = serde_json::from_slice(&bytes).ok()?;
            root.get("multipart")?
                .as_array()?
                .iter()
                .map(content_panes::PanePart::read)
                .collect::<Option<Vec<_>>>()
        });
        let Some(parts) = parts else {
            self.missing = true;
            return Ok(None);
        };
        let carrier_path = format!("assets/minecraft/blockstates/{}.json", carrier.name);
        if carrier != content_panes::DISPLAY_CARRIER.0 {
            let entries: Vec<_> = parts
                .into_iter()
                .map(|part| {
                    let apply = json!({"model": self.model(&part.model), "y": part.y});
                    if part.when.is_empty() {
                        json!({"apply": apply})
                    } else {
                        let when: serde_json::Map<String, Value> = part
                            .when
                            .into_iter()
                            .map(|(index, connected)| {
                                (
                                    content_panes::SIDES[index].to_owned(),
                                    json!(connected.to_string()),
                                )
                            })
                            .collect();
                        json!({"when": when, "apply": apply})
                    }
                })
                .collect();
            self.json(&carrier_path, &json!({"multipart": entries}))?;
            return Ok(None);
        }
        self.json(
            &carrier_path,
            &json!({"variants": {"": {"model": EMPTY_MODEL}}}),
        )?;
        let model_kind = if path.ends_with("_bars") {
            "bars"
        } else {
            "pane"
        };
        let mut models = Vec::with_capacity(parts.len());
        for (index, part) in parts.into_iter().enumerate() {
            let original = self.model(&part.model).to_owned();
            let part_path = format!("moddedmc/{model_kind}/{path}/part_{index}");
            let oriented = self.oriented_pane_model(&original, part.y);
            let id = if let Some(model) = oriented {
                self.json(
                    &format!("assets/{namespace}/models/{part_path}.json"),
                    &model,
                )?;
                format!("{namespace}:{part_path}")
            } else {
                self.missing = true;
                original
            };
            models.push((part, id));
        }

        let ids =
            std::array::from_fn(|mask| format!("{namespace}:moddedmc/{model_kind}/{path}/{mask}"));
        for (mask, _) in ids.iter().enumerate() {
            let components: Vec<_> = models
                .iter()
                .filter(|(part, _)| part.visible(mask))
                .map(|(_, id)| json!({"type": "minecraft:model", "model": id}))
                .collect();
            self.json(
                &format!("assets/{namespace}/items/moddedmc/{model_kind}/{path}/{mask}.json"),
                &json!({"model": {"type": "minecraft:composite", "models": components}}),
            )?;
        }
        Ok(Some(ids))
    }

    fn oriented_pane_model(&self, original: &str, y: i32) -> Option<Value> {
        let read = |id: &str| {
            let (namespace, path) = id.split_once(':')?;
            self.files
                .get(&format!("assets/{namespace}/models/{path}.json"))
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        };
        let model = read(original)?;
        let base = read(model.get("parent")?.as_str()?)?;
        let elements = content_panes::orient_elements(base.get("elements")?.as_array()?, y)?;
        Some(json!({"parent": original, "elements": elements}))
    }

    fn wall_item(&mut self, namespace: &str, path: &str, post: &str) -> io::Result<String> {
        let post = self.model(post);
        let arm_prefix = format!("{namespace}:moddedmc/wall/{path}");
        for (index, side) in content_walls::SIDES.into_iter().enumerate() {
            for (shape, top) in [("low", 14), ("tall", 16)] {
                self.json(
                    &format!("assets/{namespace}/models/moddedmc/wall/{path}_{side}_{shape}.json"),
                    &content_walls::arm_model(post, index, top),
                )?;
            }
        }
        self.json(
            &format!("assets/{namespace}/items/moddedmc/wall/{path}.json"),
            &content_walls::item_model(post, &arm_prefix),
        )?;
        Ok(arm_prefix)
    }

    fn carrier_variants(
        &mut self,
        block: &Block,
        mut variants: BTreeMap<String, Value>,
    ) -> io::Result<()> {
        for state in block.states {
            if let Some(key) = block_variant(block, state.id) {
                variants
                    .entry(key)
                    .or_insert_with_key(|key| stock_carrier_model(block, state.id, key));
            }
        }
        self.json(
            &format!("assets/minecraft/blockstates/{}.json", block.name),
            &json!({"variants": variants}),
        )
    }

    fn json(&mut self, path: &str, value: &Value) -> io::Result<()> {
        self.files
            .insert(path.to_owned(), serde_json::to_vec(value)?);
        Ok(())
    }

    fn action_translations(&mut self) {
        for namespace in crate::player_actions::namespaces() {
            let path = format!("assets/{namespace}/lang/en_us.json");
            if let Some(bytes) = shipped_asset(&path).and_then(|file| read_shipped_asset(&file)) {
                self.files.insert(path, bytes);
            }
        }
    }

    fn namespace_assets(&mut self, blocks: &[BlockAssets], items: &[ItemAssets]) -> io::Result<()> {
        let registered: HashSet<_> = blocks
            .iter()
            .map(|block| block.id.as_str())
            .chain(items.iter().map(|item| item.id.as_str()))
            .collect();
        let namespaces: BTreeSet<_> = registered
            .iter()
            .filter_map(|id| id.split_once(':').map(|(namespace, _)| namespace))
            .collect();
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for namespace in namespaces {
            self.sounds(namespace);
            self.display_parts(namespace)?;
            self.screen_layouts(namespace)?;
            let path = format!("assets/{namespace}/moddedmc/creative.json");
            let Some(bytes) = shipped_asset(&path).and_then(|file| read_shipped_asset(&file))
            else {
                continue;
            };
            let Ok(paths) = serde_json::from_slice::<Vec<String>>(&bytes) else {
                tracing::warn!("moddedmc: invalid creative catalog {path}");
                continue;
            };
            for path in paths {
                let id = format!("{namespace}:{path}");
                if registered.contains(id.as_str())
                    && seen.insert(id.clone())
                    && let Some(item) = Item::from_registry_key(&id)
                {
                    entries.push(item);
                }
            }
        }
        let catalog = encode_creative_catalog(&entries)?;
        if let Some(catalog) = catalog {
            self.files.insert(CREATIVE_CATALOG.to_owned(), catalog);
        }
        Ok(())
    }

    /// Companion screen definitions are data, alongside the textures they reference.
    fn screen_layouts(&mut self, namespace: &str) -> io::Result<()> {
        let path = format!("assets/{namespace}/moddedmc/screens.json");
        let Some(bytes) = shipped_asset(&path).and_then(|file| read_shipped_asset(&file)) else {
            return Ok(());
        };
        let Ok(mut layouts) = serde_json::from_slice::<BTreeMap<String, Value>>(&bytes) else {
            tracing::warn!("moddedmc: invalid screen catalog {path}");
            return Ok(());
        };
        layouts.retain(|_, layout| {
            let Some(texture) = layout.get("texture").and_then(Value::as_str) else {
                return false;
            };
            let mut textures = vec![texture];
            if let Some(atlases) = layout.get("atlases").and_then(Value::as_object) {
                for atlas in atlases.values() {
                    let Some(texture) = atlas.get("texture").and_then(Value::as_str) else {
                        return false;
                    };
                    textures.push(texture);
                }
            }
            textures.into_iter().all(|texture| {
                if !texture.starts_with("textures/")
                    || !texture.ends_with(".png")
                    || !texture.split('/').all(|part| {
                        !part.is_empty()
                            && part != "."
                            && part != ".."
                            && part.bytes().all(|b| {
                                b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b)
                            })
                    })
                {
                    return false;
                }
                let key = format!("assets/{namespace}/{texture}");
                if let Some(bytes) = self.read_asset(namespace, &key) {
                    self.files.insert(key, bytes);
                    true
                } else {
                    tracing::warn!("moddedmc: missing screen texture {key}");
                    false
                }
            })
        });
        self.files.insert(path, serde_json::to_vec(&layouts)?);
        Ok(())
    }

    fn sounds(&mut self, namespace: &str) {
        let path = format!("assets/{namespace}/sounds.json");
        let Some(bytes) = shipped_asset(&path).and_then(|file| read_shipped_asset(&file)) else {
            return;
        };
        let Ok(events) = serde_json::from_slice::<BTreeMap<String, Value>>(&bytes) else {
            tracing::warn!("moddedmc: invalid resource pack sound definitions in {path}");
            return;
        };
        for event in events.values() {
            let Some(sounds) = event.get("sounds").and_then(Value::as_array) else {
                continue;
            };
            for sound in sounds {
                if sound.get("type").and_then(Value::as_str) == Some("event") {
                    continue;
                }
                let Some(id) = sound.as_str().or_else(|| sound.get("name")?.as_str()) else {
                    continue;
                };
                let (source, name) = id.split_once(':').unwrap_or(("minecraft", id));
                if !valid_model(&format!("{source}:{name}")) {
                    continue;
                }
                let key = format!("assets/{source}/sounds/{name}.ogg");
                if !self.files.contains_key(&key)
                    && let Some(audio) =
                        shipped_asset(&key).and_then(|file| read_shipped_asset(&file))
                {
                    self.files.insert(key, audio);
                }
            }
        }
        self.files.insert(path, bytes);
    }

    fn display_parts(&mut self, namespace: &str) -> io::Result<()> {
        let path = format!("assets/{namespace}/moddedmc/display_parts.json");
        let Some(bytes) = shipped_asset(&path).and_then(|file| read_shipped_asset(&file)) else {
            return Ok(());
        };
        let Ok(parts) = serde_json::from_slice::<BTreeMap<String, String>>(&bytes) else {
            tracing::warn!("moddedmc: invalid display parts in {path}");
            return Ok(());
        };
        for (name, model) in parts {
            let id = format!("{namespace}:moddedmc/parts/{name}");
            if valid_model(&id) && !model.is_empty() && valid_model(&model) {
                self.item(
                    &format!("assets/{namespace}/items/moddedmc/parts/{name}.json"),
                    &model,
                )?;
            }
        }
        Ok(())
    }

    fn standalone_items(&mut self, items: &[ItemAssets]) -> io::Result<()> {
        for item in items {
            if let Some((namespace, path)) = item.id.split_once(':') {
                self.item(
                    &format!("assets/{namespace}/items/{path}.json"),
                    &item.model,
                )?;
                self.rig(namespace, path);
            }
        }
        Ok(())
    }

    fn item(&mut self, path: &str, id: &str) -> io::Result<()> {
        if id == "minecraft:item/compass" {
            return self.json(path, &compass_item_definition());
        }
        let model = self.model(id);
        self.json(
            path,
            &json!({"model": {"type": "minecraft:model", "model": model}}),
        )
    }

    fn oriented_stair_item(
        &mut self,
        item_path: &str,
        model_id: &str,
        assets: &StateAssets,
    ) -> io::Result<bool> {
        let source = self.model(&assets.rotating_model);
        let Some((namespace, path)) = source.split_once(':') else {
            return Ok(false);
        };
        let model_path = format!("assets/{namespace}/models/{path}.json");
        let Some(model) = self
            .files
            .get(&model_path)
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        else {
            return Ok(false);
        };
        if model.get("elements").is_some()
            || !matches!(
                model.get("parent").and_then(Value::as_str),
                Some("minecraft:block/stairs" | "block/stairs")
            )
        {
            return Ok(false);
        }
        let Some((namespace, path)) = model_id.split_once(':') else {
            return Ok(false);
        };
        self.json(
            &format!("assets/{namespace}/models/{path}.json"),
            &json!({
                "parent": source,
                "elements": crate::content_stairs::straight_display_elements(assets.x, assets.y),
            }),
        )?;
        self.json(
            item_path,
            &json!({"model": {"type": "minecraft:model", "model": model_id}}),
        )?;
        Ok(true)
    }

    fn rig(&mut self, namespace: &str, item: &str) {
        let descriptor = format!("assets/{namespace}/moddedmc/rigs/{item}.json");
        let Some(path) = shipped_asset(&descriptor) else {
            return;
        };
        let Some(bytes) = read_shipped_asset(&path) else {
            self.missing = true;
            return;
        };
        let Ok(definition) = serde_json::from_slice::<Value>(&bytes) else {
            self.missing = true;
            return;
        };
        let mut assets = Vec::new();
        let mut resources = vec![
            (definition.get("geometry"), "geckolib/models/", ".geo.json"),
            (
                definition.get("animations"),
                "geckolib/animations/",
                ".animation.json",
            ),
            (definition.get("texture"), "", ""),
        ];
        if let Some(textures) = definition.get("textures") {
            let Some(textures) = textures.as_object() else {
                self.missing = true;
                return;
            };
            resources.extend(textures.values().map(|texture| (Some(texture), "", "")));
        }
        for (resource, prefix, suffix) in resources {
            let Some(id) = resource
                .and_then(Value::as_str)
                .filter(|id| valid_model(id))
            else {
                self.missing = true;
                return;
            };
            let Some((namespace, path)) = id.split_once(':') else {
                self.missing = true;
                return;
            };
            if prefix.is_empty() && namespace == "minecraft" {
                continue;
            }
            let key = format!("assets/{namespace}/{prefix}{path}{suffix}");
            if !self.files.contains_key(&key) {
                let Some(data) = self.asset(namespace, &key) else {
                    return;
                };
                assets.push((key, data));
            }
        }
        self.files.extend(assets);
        self.files.insert(descriptor, bytes);
    }

    fn model<'a>(&mut self, id: &'a str) -> &'a str {
        if id.is_empty() || !valid_model(id) {
            return PLACEHOLDER;
        }
        let Some((namespace, path)) = id.split_once(':') else {
            return PLACEHOLDER;
        };
        if namespace == "minecraft" || namespace == "moddedmc" {
            return id;
        }
        let key = format!("assets/{namespace}/models/{path}.json");
        if !self.visited.insert(key.clone()) {
            return if self.files.contains_key(&key) {
                id
            } else {
                PLACEHOLDER
            };
        }
        let Some(bytes) = self.asset(namespace, &key) else {
            return PLACEHOLDER;
        };
        let Ok(mut model) = serde_json::from_slice::<Value>(&bytes) else {
            self.missing = true;
            return PLACEHOLDER;
        };
        if let Some(Value::String(parent)) = model.get_mut("parent")
            && parent.contains(':')
        {
            let replacement = self.model(parent);
            if replacement != parent {
                *parent = replacement.to_owned();
            }
        }
        if let Some(textures) = model.get_mut("textures").and_then(Value::as_object_mut) {
            for value in textures.values_mut() {
                let Some(id) = value.as_str() else {
                    continue;
                };
                let Some((namespace, path)) = id.split_once(':') else {
                    continue;
                };
                if namespace == "minecraft" || id.starts_with('#') || !valid_model(id) {
                    continue;
                }
                let key = format!("assets/{namespace}/textures/{path}.png");
                if !self.files.contains_key(&key) {
                    if let Some(data) = self.asset(namespace, &key) {
                        self.files.insert(key, data);
                    } else {
                        *value = Value::String(PLACEHOLDER.to_owned());
                    }
                }
            }
        }
        if let Ok(bytes) = serde_json::to_vec(&model) {
            self.files.insert(key, bytes);
            id
        } else {
            self.missing = true;
            PLACEHOLDER
        }
    }

    fn scaffolding_catalog(&mut self, blocks: &[BlockAssets]) -> io::Result<()> {
        let mut carriers = BTreeMap::new();
        let mut models = HashMap::new();
        for block in blocks {
            if !block
                .states
                .first()
                .is_some_and(|state| state.scaffolding.is_some())
            {
                continue;
            }
            let Some(registered) = Block::from_name(&block.id) else {
                continue;
            };
            for (state, assets) in registered.states.iter().zip(&block.states) {
                let Some(bottom) = assets.scaffolding else {
                    continue;
                };
                let shifts = models
                    .entry(assets.model.as_str())
                    .or_insert_with(|| self.scaffolding_textures(&assets.model));
                carriers.insert(
                    state.id.vanilla_view().as_u16(),
                    json!([bottom, block.id, shifts]),
                );
            }
        }
        if !carriers.is_empty() {
            self.json(
                "assets/moddedmc/moddedmc/scaffolding.json",
                &serde_json::to_value(carriers)?,
            )?;
        }
        Ok(())
    }

    fn scaffolding_textures(&mut self, model: &str) -> BTreeMap<String, u8> {
        let Some((namespace, path)) = model.split_once(':') else {
            return BTreeMap::new();
        };
        let key = format!("assets/{namespace}/models/{path}.json");
        let Some(model) = self
            .files
            .get(&key)
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        else {
            return BTreeMap::new();
        };
        let mut shifts = BTreeMap::new();
        for (slot, size) in [("side", 2), ("inside", 2), ("casing", 8)] {
            let Some(id) = model["textures"][slot]
                .as_str()
                .filter(|id| valid_model(id))
            else {
                continue;
            };
            let (source, path) = id.split_once(':').unwrap_or((namespace, id));
            let key = format!("assets/{source}/textures/{path}_connected.png");
            if !self.files.contains_key(&key) {
                let Some(texture) = self.read_asset(source, &key) else {
                    continue;
                };
                self.files.insert(key, texture);
            }
            shifts.insert(format!("{source}:{path}"), size);
        }
        shifts
    }

    fn asset(&mut self, namespace: &str, path: &str) -> Option<Vec<u8>> {
        let bytes = self.read_asset(namespace, path);
        self.missing |= bytes.is_none();
        bytes
    }

    fn read_asset(&mut self, namespace: &str, path: &str) -> Option<Vec<u8>> {
        if let Some(file) = shipped_asset(path) {
            return read_shipped_asset(&file);
        }
        let jar = self
            .jars
            .entry(namespace.to_owned())
            .or_insert_with(|| open_jar(namespace));
        jar.as_mut().and_then(|archive| {
            let file = archive.by_name(path).ok()?;
            if file.size() > MAX_ASSET {
                return None;
            }
            let mut bytes = Vec::with_capacity(file.size() as usize);
            file.take(MAX_ASSET + 1).read_to_end(&mut bytes).ok()?;
            (bytes.len() as u64 <= MAX_ASSET).then_some(bytes)
        })
    }

    fn zip(&self) -> io::Result<Vec<u8>> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in &self.files {
            let options = if std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ogg"))
            {
                options.compression_method(zip::CompressionMethod::Stored)
            } else {
                options
            };
            zip.start_file(name, options)?;
            zip.write_all(bytes)?;
        }
        Ok(zip.finish()?.into_inner())
    }

    fn catalog(self, version: &str) -> io::Result<Option<CatalogFiles>> {
        if self.files.len() > 10_000
            || self
                .files
                .values()
                .any(|bytes| bytes.len() as u64 > MAX_ASSET)
            || self.files.values().map(Vec::len).sum::<usize>() > 256 * 1024 * 1024
        {
            tracing::warn!("moddedmc: asset catalog exceeds companion limits");
            return Ok(None);
        }
        let mut assets = Vec::with_capacity(self.files.len());
        let mut blobs = HashMap::new();
        for (path, bytes) in self.files {
            let sha256 = hex(&Sha256::digest(&bytes));
            assets.push(json!({"path": path, "sha256": sha256, "size": bytes.len()}));
            blobs.entry(sha256).or_insert_with(|| Bytes::from(bytes));
        }
        let manifest = serde_json::to_vec(&json!({"pack_version": version, "assets": assets}))?;
        if manifest.len() > 2 * 1024 * 1024 {
            tracing::warn!("moddedmc: asset catalog exceeds companion limits");
            return Ok(None);
        }
        Ok(Some((manifest, blobs)))
    }
}

/// Reference the stock client's compass frames; no vanilla textures are redistributed.
fn compass_item_definition() -> Value {
    let entries: Vec<_> = (0..=32).map(|step| {
        let frame = (step + 16) % 32;
        json!({
            "threshold": if step == 0 {0.0} else {f64::from(step)-0.5},
            "model": {"type":"minecraft:model","model":format!("minecraft:item/compass_{frame:02}")}
        })
    }).collect();
    json!({"model":{"type":"minecraft:range_dispatch","property":"minecraft:compass","target":"lodestone","scale":32.0,"entries":entries}})
}

fn shipped_asset(path: &str) -> Option<PathBuf> {
    ASSET_ROOTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|root| root.join(path))
        .find(|file| file.is_file())
}

fn read_shipped_asset(file: &std::path::Path) -> Option<Vec<u8>> {
    fs::metadata(file)
        .is_ok_and(|meta| meta.len() <= MAX_ASSET)
        .then(|| fs::read(file).ok())
        .flatten()
}

fn open_jar(namespace: &str) -> Option<ZipArchive<File>> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("MODDEDMC_ASSET_JARS")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .collect();
    let directories = std::env::current_dir().map_or_else(
        |_| Vec::new(),
        |dir| {
            dir.ancestors()
                .map(|parent| parent.join("cache"))
                .collect::<Vec<_>>()
        },
    );
    for directory in directories {
        let mut jars: Vec<_> = fs::read_dir(directory)
            .into_iter()
            .flatten()
            .flatten()
            .map(|file| file.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with(namespace)
                            && name.as_bytes().get(namespace.len()) == Some(&b'-')
                            && path
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
                    })
            })
            .collect();
        jars.sort();
        candidates.extend(jars.into_iter().rev());
    }
    let prefix = format!("assets/{namespace}/");
    for path in candidates {
        if let Some(zip) = File::open(&path)
            .ok()
            .and_then(|file| ZipArchive::new(file).ok())
            && zip
                .file_names()
                .flatten()
                .any(|name| name.starts_with(&prefix))
        {
            tracing::info!("moddedmc: using local {} resource assets", path.display());
            return Some(zip);
        }
    }
    None
}

const PLACEHOLDER_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 152, 213, 146, 10, 0, 3,
    63, 1, 132, 169, 23, 106, 243, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
