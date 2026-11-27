//! Companion play-channel input; only registered semantic actions reach mods.
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use moddedmc_api::abi::{self, Reg, Str, code};
use pumpkin_core::{
    entity::{EntityBase, player::JavaPlayer},
    plugin::{
        BoxFuture, EventHandler, EventPriority,
        player::{player_custom_payload::PlayerCustomPayloadEvent, player_leave::PlayerLeaveEvent},
    },
    server::Server,
};

use crate::{lock, native};

const CHANNEL: &str = "moddedmc:player_actions";
const VERSION: u8 = 1;
const MAX_ACTIONS: usize = 128;
const PHASES: u8 = abi::ACTION_PRESSED | abi::ACTION_HELD | abi::ACTION_RELEASED;
const BUILTIN_KEY: &str = "moddedmc:mounted";
const QUERY_RATE_KEY: &str = "\0mount";

fn builtin_phases(id: &str) -> Option<u8> {
    use moddedmc_api::player_actions::{
        DOUBLE_TAP_BACK, DOUBLE_TAP_FORWARD, DOUBLE_TAP_LEFT, DOUBLE_TAP_RIGHT, MOUNTED_ATTACK,
        MOUNTED_USE,
    };
    match id {
        MOUNTED_ATTACK | MOUNTED_USE => Some(PHASES),
        DOUBLE_TAP_FORWARD | DOUBLE_TAP_BACK | DOUBLE_TAP_LEFT | DOUBLE_TAP_RIGHT => {
            Some(abi::ACTION_PRESSED)
        }
        _ => None,
    }
}

pub struct Registration {
    id: String,
    translation: String,
    key: String,
    phases: u8,
    callback: abi::PlayerActionCallback,
}

static ACTIONS: LazyLock<Mutex<BTreeMap<String, Vec<Registration>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static CLIENTS: LazyLock<Mutex<HashMap<i32, Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Client {
    window: Instant,
    count: u32,
    last: HashMap<String, [Option<Instant>; 3]>,
}

impl Client {
    fn new() -> Self {
        Self {
            window: Instant::now(),
            count: 0,
            last: HashMap::new(),
        }
    }

    fn permit(&mut self, id: &str, phase: u8, now: Instant) -> bool {
        if now.duration_since(self.window) >= Duration::from_secs(1) {
            self.window = now;
            self.count = 0;
        }
        if self.count >= 128 {
            return false;
        }
        self.count += 1;
        let index = phase.trailing_zeros() as usize;
        let delay = if phase == abi::ACTION_HELD { 150 } else { 40 };
        let times = if let Some(times) = self.last.get_mut(id) {
            times
        } else {
            self.last.entry(id.to_owned()).or_default()
        };
        let last = &mut times[index];
        if last.is_some_and(|last| now.duration_since(last) < Duration::from_millis(delay)) {
            return false;
        }
        *last = Some(now);
        true
    }
}

pub static PLAYER_ACTIONS: abi::PlayerActionsV1 = abi::PlayerActionsV1 {
    size: size_of::<abi::PlayerActionsV1>(),
    add,
    subscribe,
};

unsafe extern "C" fn add(
    reg: Reg,
    id: Str,
    translation: Str,
    key: Str,
    phases: u8,
    callback: abi::PlayerActionCallback,
) -> i32 {
    // SAFETY: registration and all strings are borrowed for this call.
    let (staging, id, translation, key) =
        unsafe { (native::staging(reg), id.str(), translation.str(), key.str()) };
    if !crate::valid_id(id)
        || builtin_phases(id).is_some()
        || key == BUILTIN_KEY
        || id.len() > 128
        || translation.is_empty()
        || translation.len() > 128
        || key.is_empty()
        || key.len() > 64
        || phases == 0
        || phases & !PHASES != 0
    {
        return code::INVALID_DEFINITION;
    }
    let actions = lock(&ACTIONS);
    if actions.contains_key(id) || staging.player_actions.iter().any(|a| a.id == id) {
        return code::DUPLICATE_ID;
    }
    let new_ids = staging
        .player_actions
        .iter()
        .filter(|a| !actions.contains_key(&a.id))
        .count();
    if actions.len() + new_ids >= MAX_ACTIONS {
        return code::INVALID_DEFINITION;
    }
    staging.player_actions.push(Registration {
        id: id.to_owned(),
        translation: translation.to_owned(),
        key: key.to_owned(),
        phases,
        callback,
    });
    code::OK
}

unsafe extern "C" fn subscribe(
    reg: Reg,
    id: Str,
    phases: u8,
    callback: abi::PlayerActionCallback,
) -> i32 {
    // SAFETY: registration and id are borrowed for this call.
    let (staging, id) = unsafe { (native::staging(reg), id.str()) };
    if !builtin_phases(id).is_some_and(|allowed| phases != 0 && phases & !allowed == 0) {
        return code::INVALID_DEFINITION;
    }
    if staging.player_actions.iter().any(|a| a.id == id) {
        return code::DUPLICATE_ID;
    }
    let actions = lock(&ACTIONS);
    let new_ids = staging
        .player_actions
        .iter()
        .filter(|a| !actions.contains_key(&a.id))
        .count();
    if !actions.contains_key(id) && actions.len() + new_ids >= MAX_ACTIONS {
        return code::INVALID_DEFINITION;
    }
    staging.player_actions.push(Registration {
        id: id.to_owned(),
        translation: id.to_owned(),
        key: BUILTIN_KEY.to_owned(),
        phases,
        callback,
    });
    code::OK
}

pub fn commit(staged: Vec<Registration>) {
    let mut actions = lock(&ACTIONS);
    for action in staged {
        tracing::info!("moddedmc: registered player action {}", action.id);
        actions.entry(action.id.clone()).or_default().push(action);
    }
}

pub fn namespaces() -> std::collections::BTreeSet<String> {
    lock(&ACTIONS)
        .keys()
        .filter_map(|id| {
            id.split_once(':')
                .map(|(namespace, _)| namespace.to_owned())
        })
        .collect()
}

fn list() -> Vec<u8> {
    let actions = lock(&ACTIONS);
    encode_catalog(&actions)
}

fn encode_catalog(actions: &BTreeMap<String, Vec<Registration>>) -> Vec<u8> {
    let mut data = vec![VERSION, 1];
    data.extend_from_slice(&(actions.len() as u16).to_be_bytes());
    for entries in actions.values() {
        let action = &entries[0];
        for field in [&action.id, &action.translation, &action.key] {
            data.push(field.len() as u8);
            data.extend_from_slice(field.as_bytes());
        }
        data.push(entries.iter().fold(0, |mask, entry| mask | entry.phases));
    }
    data
}

fn parse(data: &[u8]) -> Option<(&str, u8)> {
    let [VERSION, 2, phase, len, id @ ..] = data else {
        return None;
    };
    if usize::from(*len) != id.len() || !matches!(*phase, 1 | 2 | 4) {
        return None;
    }
    Some((std::str::from_utf8(id).ok()?, *phase))
}

fn mounted_mod_id(player: &dyn EntityBase) -> Option<i32> {
    let vehicle = player.get_entity().get_vehicle()?;
    crate::mobs::registered_kind(vehicle.as_ref())?;
    Some(vehicle.get_entity().entity_id)
}

fn mount_query(data: &[u8]) -> Option<i32> {
    let [VERSION, 3, a, b, c, d] = data else {
        return None;
    };
    Some(i32::from_be_bytes([*a, *b, *c, *d]))
}

pub fn attach(server: &Server) {
    server
        .plugin_manager
        .register::<PlayerCustomPayloadEvent, _>(Arc::new(Payload), EventPriority::Normal, true);
    server.plugin_manager.register::<PlayerLeaveEvent, _>(
        Arc::new(Leave),
        EventPriority::Normal,
        true,
    );
}

struct Payload;
impl EventHandler<PlayerCustomPayloadEvent> for Payload {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerCustomPayloadEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.channel != CHANNEL {
                return;
            }
            let player = &event.player;
            let player_id = player.entity_id();
            if event.data.as_ref() == [VERSION, 0] {
                let fresh = {
                    let mut clients = lock(&CLIENTS);
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        clients.entry(player_id)
                    {
                        entry.insert(Client::new());
                        true
                    } else {
                        false
                    }
                };
                if fresh {
                    JavaPlayer(player)
                        .send_custom_payload(CHANNEL, &list())
                        .await;
                }
                return;
            }
            if !player.living_entity.is_alive() {
                return;
            }
            let entity: &dyn EntityBase = player.as_ref();
            if let Some(requested) = mount_query(&event.data) {
                if !lock(&CLIENTS)
                    .get_mut(&player_id)
                    .is_some_and(|c| c.permit(QUERY_RATE_KEY, abi::ACTION_PRESSED, Instant::now()))
                {
                    return;
                }
                let mut reply = vec![VERSION, 4];
                reply.extend_from_slice(&requested.to_be_bytes());
                reply.push(u8::from(mounted_mod_id(entity) == Some(requested)));
                JavaPlayer(player)
                    .send_custom_payload(CHANNEL, &reply)
                    .await;
                return;
            }
            let Some((id, phase)) = parse(&event.data) else {
                return;
            };
            if builtin_phases(id).is_some() && mounted_mod_id(entity).is_none() {
                return;
            }
            // Copy callbacks and drop the registry lock before invoking mod code.
            let callbacks: Vec<_> = {
                let actions = lock(&ACTIONS);
                let Some(entries) = actions.get(id) else {
                    return;
                };
                entries
                    .iter()
                    .filter(|a| a.phases & phase != 0)
                    .map(|a| a.callback)
                    .collect()
            };
            if callbacks.is_empty() {
                return;
            }
            if !lock(&CLIENTS)
                .get_mut(&player_id)
                .is_some_and(|c| c.permit(id, phase, Instant::now()))
            {
                return;
            }
            let world = player.world();
            for callback in callbacks {
                // SAFETY: world, entity and action id are borrowed for this synchronous callback.
                unsafe {
                    callback(
                        Arc::as_ptr(&world).cast(),
                        (&raw const entity).cast(),
                        Str::from(id),
                        phase,
                    );
                }
            }
        })
    }
}

struct Leave;
impl EventHandler<PlayerLeaveEvent> for Leave {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerLeaveEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            lock(&CLIENTS).remove(&event.player.entity_id());
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_combines_multiple_subscribers_without_duplicate_bindings() {
        unsafe extern "C" fn callback(_: abi::WorldRef, _: abi::EntityRef, _: Str, _: u8) {}
        let id = "moddedmc:mounted_attack";
        let entry = |phases| Registration {
            id: id.to_owned(),
            translation: id.to_owned(),
            key: BUILTIN_KEY.to_owned(),
            phases,
            callback,
        };
        let actions = BTreeMap::from([(
            id.to_owned(),
            vec![entry(abi::ACTION_PRESSED), entry(abi::ACTION_RELEASED)],
        )]);
        let data = encode_catalog(&actions);
        let mut expected = vec![VERSION, 1, 0, 1];
        for field in [id, id, BUILTIN_KEY] {
            expected.push(field.len() as u8);
            expected.extend_from_slice(field.as_bytes());
        }
        expected.push(abi::ACTION_PRESSED | abi::ACTION_RELEASED);
        assert_eq!(data, expected);
    }

    #[test]
    fn rejects_malformed_input_and_limits_repeats() {
        assert_eq!(parse(&[1, 2, 1, 3, b'a', b':', b'b']), Some(("a:b", 1)));
        assert!(parse(&[1, 2, 3, 1, b'a']).is_none());
        assert!(parse(&[1, 2, 1, 2, b'a']).is_none());
        assert_eq!(mount_query(&[1, 3, 0, 0, 0, 42]), Some(42));
        assert!(mount_query(&[1, 3, 0, 0, 0]).is_none());
        assert_eq!(
            builtin_phases("moddedmc:double_tap/left"),
            Some(abi::ACTION_PRESSED)
        );
        assert_eq!(builtin_phases("moddedmc:mounted_attack"), Some(PHASES));
        assert_eq!(builtin_phases("moddedmc:double_tap/up"), None);
        let mut client = Client::new();
        let now = Instant::now();
        assert!(client.permit("a:b", 2, now));
        assert!(!client.permit("a:b", 2, now));
        assert!(client.permit("a:b", 4, now));
        assert!(client.permit("a:b", 2, now + Duration::from_millis(200)));
    }
}
