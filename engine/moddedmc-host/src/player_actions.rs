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

pub struct Registration {
    id: String,
    translation: String,
    key: String,
    phases: u8,
    callback: abi::PlayerActionCallback,
}

static ACTIONS: LazyLock<Mutex<BTreeMap<String, Registration>>> =
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
    if actions.len() + staging.player_actions.len() >= MAX_ACTIONS {
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

pub fn commit(staged: Vec<Registration>) {
    let mut actions = lock(&ACTIONS);
    for action in staged {
        tracing::info!("moddedmc: registered player action {}", action.id);
        actions.insert(action.id.clone(), action);
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
    let mut data = vec![VERSION, 1];
    data.extend_from_slice(&(actions.len() as u16).to_be_bytes());
    for action in actions.values() {
        for field in [&action.id, &action.translation, &action.key] {
            data.push(field.len() as u8);
            data.extend_from_slice(field.as_bytes());
        }
        data.push(action.phases);
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
            let Some((id, phase)) = parse(&event.data) else {
                return;
            };
            if !player.living_entity.is_alive() {
                return;
            }
            let Some((phases, callback)) = lock(&ACTIONS).get(id).map(|a| (a.phases, a.callback))
            else {
                return;
            };
            if phases & phase == 0 {
                return;
            }
            if !lock(&CLIENTS)
                .get_mut(&player_id)
                .is_some_and(|c| c.permit(id, phase, Instant::now()))
            {
                return;
            }
            let world = player.world();
            let entity: &dyn EntityBase = player.as_ref();
            // SAFETY: world, entity and action id are borrowed for this synchronous callback.
            unsafe {
                callback(
                    Arc::as_ptr(&world).cast(),
                    (&raw const entity).cast(),
                    Str::from(id),
                    phase,
                );
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
    fn rejects_malformed_input_and_limits_repeats() {
        assert_eq!(parse(&[1, 2, 1, 3, b'a', b':', b'b']), Some(("a:b", 1)));
        assert!(parse(&[1, 2, 3, 1, b'a']).is_none());
        assert!(parse(&[1, 2, 1, 2, b'a']).is_none());
        let mut client = Client::new();
        let now = Instant::now();
        assert!(client.permit("a:b", 2, now));
        assert!(!client.permit("a:b", 2, now));
        assert!(client.permit("a:b", 4, now));
        assert!(client.permit("a:b", 2, now + Duration::from_millis(200)));
    }
}
