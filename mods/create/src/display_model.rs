//! A transient display of an item model from the server resource pack.
use moddedmc_api::{content::World, stacks::ItemDisplay};

use crate::DISPLAYS;

#[derive(Default)]
pub(crate) struct ModelDisplay {
    entity: Option<i32>,
    model: &'static str,
}

impl ModelDisplay {
    pub(crate) fn show(&mut self, world: World<'_>, model: &'static str, transform: &ItemDisplay) {
        let Some(displays) = DISPLAYS.get() else {
            return;
        };
        let live = self
            .entity
            .filter(|&entity| displays.update(world, entity, transform));
        if live.is_some() && model == self.model {
            return;
        }
        let stack = fastnbt::nbt!({
            "id": "minecraft:paper",
            "count": 1i32,
            "components": {"minecraft:item_model": model}
        });
        let Ok(stack) = fastnbt::to_bytes(&stack) else {
            return;
        };
        if let Some(entity) = live {
            if displays.set_item(world, entity, &stack) {
                self.model = model;
            }
        } else {
            self.entity = displays.spawn_item(world, &stack, transform);
            self.model = model;
        }
    }

    pub(crate) fn clear(&mut self, world: World<'_>) {
        if let (Some(displays), Some(entity)) = (DISPLAYS.get(), self.entity.take()) {
            displays.remove(world, entity);
        }
    }
}
