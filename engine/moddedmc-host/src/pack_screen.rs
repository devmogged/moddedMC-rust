//! Standalone pack screens reuse the menu callback ABI without inventory slots.
use crate::{HOST, lock};
use pumpkin_core::{
    entity::{
        EntityBase,
        player::{JavaPlayer, Player},
    },
    plugin::{
        BoxFuture, EventHandler, EventPriority,
        player::{player_custom_payload::PlayerCustomPayloadEvent, player_leave::PlayerLeaveEvent},
    },
    server::Server,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const CHANNEL: &str = "moddedmc:pack_screen";
pub const MARKER: &str = "moddedmc:pack_screen";
const VERSION: u8 = 1;
const MAX_OPEN: usize = 256 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(1);
static CLIENTS: LazyLock<Mutex<HashSet<i32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static OPEN: LazyLock<Mutex<HashMap<i32, Session>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
thread_local! {
    static REPLY_TO: std::cell::RefCell<Option<(i32, String)>> = const { std::cell::RefCell::new(None) };
}

#[derive(Deserialize, Serialize)]
struct Definition {
    id: String,
    state: Value,
    actions: HashMap<String, bool>,
}
struct Session {
    token: String,
    actions: HashMap<String, bool>,
}
#[derive(Deserialize)]
struct Click {
    token: String,
    action: String,
    #[serde(default)]
    text: String,
}
fn valid_id(id: &str) -> bool {
    id.len() <= 160
        && id.split_once(':').is_some_and(|(namespace, path)| {
            !namespace.is_empty()
                && !path.is_empty()
                && namespace
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && path.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part.bytes().all(|b| {
                            b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b)
                        })
                })
        })
}
pub fn invalidate(player: &Player) {
    lock(&OPEN).remove(&player.entity_id());
}
pub fn show(player: &Player, json: &str) -> bool {
    invalidate(player);
    let id = player.entity_id();
    if json.len() > MAX_OPEN || !lock(&CLIENTS).contains(&id) {
        return false;
    }
    let Ok(definition) = serde_json::from_str::<Definition>(json) else {
        return false;
    };
    if !valid_id(&definition.id)
        || !definition.state.is_object()
        || definition.actions.len() > 512
        || definition.actions.keys().any(|action| !valid_id(action))
    {
        return false;
    }
    let world = player.world();
    let Some(server) = world.server.upgrade() else {
        return false;
    };
    let Some(player) = world.get_player_by_id(id) else {
        return false;
    };
    let token = NEXT.fetch_add(1, Ordering::Relaxed).to_string();
    let reply_to = REPLY_TO.with(|reply| {
        reply
            .borrow()
            .as_ref()
            .filter(|(player_id, _)| *player_id == id)
            .map(|(_, token)| token.clone())
    });
    let Ok(body) = serde_json::to_vec(&serde_json::json!({
        "id": definition.id, "state": definition.state, "token": token, "replyTo": reply_to
    })) else {
        return false;
    };
    if body.len() > MAX_OPEN {
        return false;
    }
    lock(&OPEN).insert(
        id,
        Session {
            token,
            actions: definition.actions,
        },
    );
    let mut packet = vec![VERSION, 1];
    packet.extend_from_slice(&body);
    server.spawn_task(async move {
        JavaPlayer(&player)
            .send_custom_payload(CHANNEL, &packet)
            .await;
    });
    true
}

fn resolve(session: &Session, click: &Click) -> Option<String> {
    if click.token != session.token {
        return None;
    }
    let input = *session.actions.get(&click.action)?;
    if input {
        // A bounded, slash-free argument; mod callbacks must still validate its meaning.
        if click.text.is_empty()
            || click.text.len() > 64
            || !click
                .text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.- ".contains(&b))
        {
            return None;
        }
        Some(format!("{}/{}", click.action, click.text))
    } else if click.text.is_empty() {
        Some(click.action.clone())
    } else {
        None
    }
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
        _: &'a Arc<Server>,
        event: &'a mut PlayerCustomPayloadEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.channel != CHANNEL {
                return;
            }
            let id = event.player.entity_id();
            if event.data.as_ref() == [VERSION, 0] {
                lock(&CLIENTS).insert(id);
                return;
            }
            if !event.player.living_entity.is_alive() {
                return;
            }
            let bytes = event.data.as_ref();
            if bytes.len() > 1024 || !bytes.starts_with(&[VERSION, 2]) {
                return;
            }
            let Ok(click) = serde_json::from_slice::<Click>(&bytes[2..]) else {
                return;
            };
            let action = {
                let mut sessions = lock(&OPEN);
                let Some(action) = sessions.get(&id).and_then(|s| resolve(s, &click)) else {
                    return;
                };
                sessions.remove(&id);
                action
            };
            let mods: Vec<_> = lock(&HOST.mods).iter().map(|m| m.module).collect();
            REPLY_TO.with(|reply| {
                let previous = reply.replace(Some((id, click.token)));
                for module in mods {
                    module.menu_click(&event.player, &action);
                }
                reply.replace(previous);
            });
        })
    }
}
struct Leave;
impl EventHandler<PlayerLeaveEvent> for Leave {
    fn handle_blocking<'a>(
        &'a self,
        _: &'a Arc<Server>,
        event: &'a mut PlayerLeaveEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            lock(&CLIENTS).remove(&event.player.entity_id());
            invalidate(&event.player);
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_is_bound_to_current_action_and_token() {
        let session = Session {
            token: "2".into(),
            actions: HashMap::from([
                ("example:add".into(), true),
                ("example:refresh".into(), false),
            ]),
        };
        let mut click = Click {
            token: "2".into(),
            action: "example:add".into(),
            text: "Player_1".into(),
        };
        assert_eq!(
            resolve(&session, &click).as_deref(),
            Some("example:add/Player_1")
        );
        click.text = "evil/path".into();
        assert!(resolve(&session, &click).is_none());
        click.action = "example:refresh".into();
        assert!(resolve(&session, &click).is_none());
        click.text.clear();
        assert_eq!(
            resolve(&session, &click).as_deref(),
            Some("example:refresh")
        );
        click.token = "1".into();
        assert!(resolve(&session, &click).is_none());
    }
}
