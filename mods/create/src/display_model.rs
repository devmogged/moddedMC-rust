//! A transient display of an item model from the server resource pack.
use moddedmc_api::{content::World, stacks::ItemDisplay};

use crate::DISPLAYS;

#[derive(Clone, Debug, Default)]
pub(crate) struct ModelDisplay {
    entity: Option<i32>,
    model: String,
}

impl ModelDisplay {
    pub(crate) const fn entity_id(&self) -> Option<i32> {
        self.entity
    }

    pub(crate) fn show(&mut self, world: World<'_>, model: &str, transform: &ItemDisplay) {
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
                model.clone_into(&mut self.model);
            }
        } else {
            self.entity = displays.spawn_item(world, &stack, transform);
            model.clone_into(&mut self.model);
        }
    }

    pub(crate) fn clear(&mut self, world: World<'_>) {
        if let (Some(displays), Some(entity)) = (DISPLAYS.get(), self.entity.take()) {
            displays.remove(world, entity);
        }
    }
}
