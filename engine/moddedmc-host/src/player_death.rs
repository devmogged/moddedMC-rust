use std::{ptr, sync::Mutex};

use moddedmc_api::abi::{self, PlayerRef, Reg, Str, code};
use pumpkin_core::{
    entity::player::{Player, PlayerInventoryDrop, install_player_death_drop_hook},
    world::World,
};
use pumpkin_nbt::Nbt;
use pumpkin_util::math::vector3::Vector3;

use crate::{lock, native, saved_data::Transaction};

static CALLBACKS: Mutex<Vec<(&'static str, abi::PlayerDeathCallback)>> = Mutex::new(Vec::new());

pub static PLAYER_DEATH: abi::PlayerDeathV1 = abi::PlayerDeathV1 {
    size: size_of::<abi::PlayerDeathV1>(),
    on_drops,
};

unsafe extern "C" fn on_drops(reg: Reg, callback: abi::PlayerDeathCallback) -> i32 {
    // SAFETY: the handle belongs to the mod's live registration.
    let stage = unsafe { native::staging(reg) };
    if stage.player_death.is_some() {
        return code::DUPLICATE_ID;
    }
    stage.player_death = Some(callback);
    code::OK
}

pub fn register(owner: &'static str, callback: Option<abi::PlayerDeathCallback>) {
    let Some(callback) = callback else { return };
    let mut callbacks = lock(&CALLBACKS);
    if callbacks.is_empty() && !install_player_death_drop_hook(dispatch) {
        tracing::error!("moddedmc: player death drop hook is already installed");
        return;
    }
    callbacks.push((owner, callback));
}

fn dispatch(
    player: &Player,
    world: &World,
    position: Vector3<f64>,
    drops: &[PlayerInventoryDrop],
) -> bool {
    let callbacks = lock(&CALLBACKS).clone();
    if callbacks.is_empty() {
        return false;
    }

    let Some(saved): Option<Vec<_>> = drops
        .iter()
        .map(|drop| drop.stack.to_nbt().map(|stack| Nbt::from(stack).write()))
        .collect()
    else {
        return false;
    };
    let entries: Vec<_> = drops
        .iter()
        .zip(&saved)
        .map(|(drop, stack)| abi::PlayerDeathStack {
            slot: drop.slot,
            stack: Str::new(stack),
        })
        .collect();
    callbacks.into_iter().any(|(owner, callback)| {
        let raw_world = ptr::from_ref(world).cast();
        let mut transaction = Transaction::new(raw_world, owner);
        let context = abi::PlayerDeathContext {
            world: raw_world,
            player: PlayerRef(ptr::from_ref(player).cast()),
            player_uuid: *player.gameprofile.id.as_bytes(),
            position: [position.x, position.y, position.z],
            drops: entries.as_ptr(),
            count: entries.len(),
            transaction: (&raw mut transaction).cast(),
            claim: Transaction::commit,
        };
        // SAFETY: the world, player, transaction and saved stacks are live for this call.
        unsafe { callback(&raw const context) };
        transaction.committed
    })
}
