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
        self.show_item(world, model, transform, || {
            let stack = fastnbt::nbt!({
                "id": "minecraft:paper",
                "count": 1i32,
                "components": {"minecraft:item_model": model}
            });
            fastnbt::to_bytes(&stack).ok()
        });
    }

    pub(crate) fn show_stack(
        &mut self,
        world: World<'_>,
        model: &str,
        stack: &[u8],
        transform: &ItemDisplay,
    ) {
        self.show_item(world, model, transform, || Some(stack));
    }

    fn show_item<B: AsRef<[u8]>>(
        &mut self,
        world: World<'_>,
        model: &str,
        transform: &ItemDisplay,
        stack: impl FnOnce() -> Option<B>,
    ) {
        let Some(displays) = DISPLAYS.get() else {
            return;
        };
        let live = self
            .entity
            .filter(|&entity| displays.update(world, entity, transform));
        if live.is_some() && model == self.model {
            return;
        }
        let Some(stack) = stack() else {
            return;
        };
        if let Some(entity) = live {
            if displays.set_item(world, entity, stack.as_ref()) {
                model.clone_into(&mut self.model);
            }
        } else {
            self.entity = displays.spawn_item(world, stack.as_ref(), transform);
            model.clone_into(&mut self.model);
        }
    }

    pub(crate) fn clear(&mut self, world: World<'_>) {
        if let (Some(displays), Some(entity)) = (DISPLAYS.get(), self.entity.take()) {
            displays.remove(world, entity);
        }
    }
}
