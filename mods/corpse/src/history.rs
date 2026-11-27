use moddedmc_api::{
    Menu, MenuButton, Player,
    content::World,
    entities::Entity,
    inventory_menu::{Action, ActionSlots, Change, Control},
};

use crate::settings::now_millis;
use crate::{
    RUNTIME, Runtime, archive,
    menus::{
        RESTORE, RESTORE_SLOT, inventory_slots, main_access, matching_targets, restore_button_stack,
    },
    profile_uuid,
    records::Grave,
};

const ACTION_PREFIX: &str = "corpse:history/";
const CREATIVE_MODE: i32 = 1;
const CREATIVE_PREVIEW: u32 = 1 << 31;
const READ_ONLY_PREVIEW: u32 = u32::MAX;

#[derive(Clone, Copy)]
pub struct DeathLink {
    id: i64,
    record: [u8; 16],
}

impl DeathLink {
    fn matches(self, death: &Grave) -> bool {
        self.id == death.id && death.uuid() == Some(self.record)
    }
}

pub fn show(
    runtime: Runtime,
    world: World<'_>,
    player: &Player<'_>,
    uuid: &[u8; 16],
    requested: Option<DeathLink>,
) -> bool {
    let now = now_millis();
    let menu = archive(runtime, world, |current| {
        let deaths: Vec<_> = current
            .deaths
            .iter()
            .rev()
            .filter(|death| death.is_recorded(now) && death.owns(uuid))
            .collect();
        let index = requested.map_or(Some(0), |link| {
            deaths.iter().position(|death| link.matches(death))
        })?;
        let death = deaths.get(index)?;
        let [x, y, z] = death.position;
        let mut buttons = Vec::new();
        if let Some(newer) = index.checked_sub(1).and_then(|index| deaths.get(index)) {
            buttons.extend(button("Newer", "show", newer));
        }
        buttons.extend(button("Show items", "items", death));
        if let Some(older) = deaths.get(index + 1) {
            buttons.extend(button("Older", "show", older));
        }
        Some(Menu {
            title: format!("Death History ({}/{})", index + 1, deaths.len()),
            body: format!(
                "{}\n{}\n{}\n{x:.1}, {y:.1}, {z:.1}",
                death.owner,
                timestamp(death.created_at),
                death.dimension
            ),
            buttons,
        })
    })
    .flatten();
    if let Some(menu) = menu {
        player.show_menu(&menu);
        true
    } else {
        if requested.is_none() {
            player.send_message("You have no recorded deaths.");
        }
        false
    }
}

fn button(label: &str, action: &str, death: &Grave) -> Option<MenuButton> {
    let record = u128::from_be_bytes(death.uuid()?);
    Some(MenuButton {
        label: label.to_owned(),
        tooltip: String::new(),
        action: format!("{ACTION_PREFIX}{action}/{}/{record:032x}", death.id),
    })
}

pub fn click(player: Player<'_>, action: &str) {
    let Some((verb, death)) = action
        .strip_prefix(ACTION_PREFIX)
        .and_then(|action| action.split_once('/'))
    else {
        return;
    };
    let (Some((id, record)), Some(&runtime)) = (death.split_once('/'), RUNTIME.get()) else {
        return;
    };
    let (Ok(id), Ok(record)) = (id.parse::<i64>(), u128::from_str_radix(record, 16)) else {
        return;
    };
    let link = DeathLink {
        id,
        record: record.to_be_bytes(),
    };
    let Some(uuid) = runtime
        .interfaces
        .actors
        .player_profile(&player)
        .and_then(|profile| profile_uuid(&profile))
    else {
        return;
    };
    player.with_entity_context(|world, entity| match verb {
        "show" => show(runtime, world, &player, &uuid, Some(link)),
        "items" => show_items(runtime, world, &player, entity, &uuid, link),
        _ => false,
    });
}

fn show_items(
    runtime: Runtime,
    world: World<'_>,
    player: &Player<'_>,
    entity: Entity<'_>,
    uuid: &[u8; 16],
    link: DeathLink,
) -> bool {
    let Ok(number) = u32::try_from(link.id) else {
        return false;
    };
    let Some((owner, mut items)) = archive(runtime, world, |current| {
        let death = current.get(link.id).filter(|death| {
            death.is_recorded(now_millis()) && death.owns(uuid) && link.matches(death)
        })?;
        Some((death.owner.clone(), inventory_slots(&death.original)))
    })
    .flatten() else {
        return false;
    };
    let mut preview = READ_ONLY_PREVIEW;
    if runtime.interfaces.mobs.game_mode(entity) == Some(CREATIVE_MODE)
        && let Ok(id) = u32::try_from(entity.id())
        && id < READ_ONLY_PREVIEW - CREATIVE_PREVIEW
    {
        preview = CREATIVE_PREVIEW | id;
        items[usize::from(RESTORE_SLOT)] = restore_button_stack();
    }
    let controls = [Control {
        slot: RESTORE_SLOT,
        tag: RESTORE,
    }];
    let creative = preview != READ_ONLY_PREVIEW;
    let access = main_access(creative);
    // Live body pages use the lower half. Creative viewers each get their own copy.
    let inventory = (u64::from(number) << 32) | u64::from(preview);
    runtime.interfaces.menus.open(
        player,
        runtime.menu,
        inventory,
        &format!("Death of {owner}"),
        ActionSlots {
            world,
            access: &access,
            controls: if creative { &controls } else { &[] },
        },
        || Some(items),
    )
}

pub fn creative_player(inventory: u64) -> Option<i32> {
    let preview = u32::try_from(inventory & u64::from(u32::MAX)).ok()?;
    if preview == READ_ONLY_PREVIEW || preview & CREATIVE_PREVIEW == 0 {
        return None;
    }
    i32::try_from(preview & !CREATIVE_PREVIEW).ok()
}

pub fn transfer(action: Action<'_>, player_id: i32) {
    let Some(&runtime) = RUNTIME.get() else {
        return;
    };
    if action.tag != RESTORE
        || action.player.with_entity_context(|_, entity| {
            entity.id() == player_id
                && runtime.interfaces.mobs.game_mode(entity) == Some(CREATIVE_MODE)
        }) != Some(true)
    {
        return;
    }
    if !runtime.interfaces.menus.menus().restore_matching(
        &action.player,
        runtime.menu,
        action.inventory,
        &matching_targets(),
    ) {
        action
            .player
            .send_message("No saved items could be transferred.");
    }
}

pub fn change(mut change: Change<'_>, player_id: i32) {
    let Some(&runtime) = RUNTIME.get() else {
        return;
    };
    if change.overflow().len() != 0 {
        return;
    }
    let permitted = runtime
        .interfaces
        .entities
        .with_entity(change.world, player_id, |entity| {
            runtime.interfaces.mobs.uuid(entity) == Some(change.player_uuid)
                && runtime.interfaces.mobs.game_mode(entity) == Some(CREATIVE_MODE)
        })
        == Some(true);
    let Some(id) = u32::try_from(change.inventory >> 32).ok().map(i64::from) else {
        return;
    };
    let recorded = permitted
        && archive(runtime, change.world, |current| {
            current.get(id).is_some_and(|death| {
                death.owns(&change.player_uuid) && death.is_recorded(now_millis())
            })
        }) == Some(true);
    if recorded {
        change.commit(runtime.preview_key, &[]);
    }
}

fn timestamp(milliseconds: i64) -> String {
    let seconds = milliseconds / 1000;
    if !(0..253_402_300_800).contains(&seconds) {
        return String::new();
    }
    let mut days = seconds / 86_400;
    let mut year = 1970;
    let leap = |year| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    loop {
        let length = 365 + i64::from(leap(year));
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    let lengths = [
        31,
        28 + i64::from(leap(year)),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for length in lengths {
        if days < length {
            break;
        }
        days -= length;
        month += 1;
    }
    let (hour, minute, second) = ((seconds / 3600) % 24, (seconds / 60) % 60, seconds % 60);
    format!(
        "{year:04}/{month:02}/{:02} {hour:02}:{minute:02}:{second:02} UTC",
        days + 1
    )
}
