//! Source-checked model of `BlockSpoutBlockEntity` (CMR 1.2.1-fix1).
//!
//! This is not wired to the server yet. WS-4-02 needs the Create fluid-handler
//! and recipe registration interfaces before this machine can run in-world.
//! Test data comes from the original `block_spouting/grass_block.json`.

use serde::{Deserialize, Serialize};

pub const CAPACITY_MB: u32 = 1_000;
pub const PROCESSING_TICKS: i32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub input: &'static str,
    pub fluid: &'static str,
    pub fluid_mb: u32,
    pub output: &'static str,
}

pub const GRASS_BLOCK: Recipe = Recipe {
    input: "minecraft:dirt",
    fluid: "minecraft:water",
    fluid_mb: 500,
    output: "minecraft:grass_block",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fluid {
    pub id: &'static str,
    pub amount_mb: u32,
}

impl Fluid {
    pub const fn empty() -> Self {
        Self {
            id: "",
            amount_mb: 0,
        }
    }

    pub fn accepts(&self, requested: u32, id: &str) -> bool {
        self.amount_mb >= requested && self.id == id
    }

    /// A single Create SmartFluidTankBehaviour reservoir holds up to 1,000 mB.
    /// The downward face refuses fluid input in the original addon.
    pub fn fill(&mut self, id: &'static str, amount: u32, from_bottom: bool) -> u32 {
        if from_bottom || (!self.id.is_empty() && self.amount_mb > 0 && self.id != id) {
            return 0;
        }
        let accepted = amount.min(CAPACITY_MB.saturating_sub(self.amount_mb));
        if accepted > 0 {
            self.id = id;
            self.amount_mb += accepted;
        }
        accepted
    }

    /// Called after completion; Create's FluidStack.shrink saturates at zero.
    fn consume(&mut self, amount: u32) {
        self.amount_mb = self.amount_mb.saturating_sub(amount);
        if self.amount_mb == 0 {
            self.id = "";
        }
    }
}

/// Save only the field the original machine writes itself. The recipe
/// pointer is deliberately NOT serialized in the original Java block entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpoutSave {
    #[serde(rename = "ProcessingTicks", default)]
    pub processing_ticks: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockSpout {
    pub processing_ticks: i32,
    pub active_recipe: Option<usize>,
    pub send_splash: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completed {
    /// Set the target block two positions below the spout to this output.
    pub block: &'static str,
    /// Liquid actually consumed. Java can finish even if the player drained
    /// the tank after matching the recipe at the beginning of the cycle.
    pub fluid_consumed_mb: u32,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    pub completed: Option<Completed>,
    pub splash: bool,
    pub started: bool,
}

impl Default for BlockSpout {
    fn default() -> Self {
        Self {
            processing_ticks: -1,
            active_recipe: None,
            send_splash: false,
        }
    }
}

impl BlockSpout {
    pub fn load(saved: SpoutSave) -> Self {
        Self {
            processing_ticks: saved.processing_ticks,
            active_recipe: None,
            send_splash: false,
        }
    }

    pub fn save(self) -> SpoutSave {
        SpoutSave {
            processing_ticks: self.processing_ticks,
        }
    }

    /// Java sends the splash flag only in client update packets and then
    /// immediately clears the flag; disk NBT never contains the flag.
    pub fn take_client_splash(&mut self) -> bool {
        std::mem::take(&mut self.send_splash)
    }

    /// One synchronous server tick. `block_below` is the block TWO positions
    /// below the spout. Successful completion changes that block in this tick.
    ///
    /// Java decrements before the 7-tick completion, then looks for a new
    /// recipe *in the same tick*. Selection order follows recipe iteration.
    /// Neither the current candidate nor the selected recipe is revalidated
    /// mid-cycle. Callers must keep recipe order fixed during the cycle.
    pub fn tick(
        &mut self,
        block_below: &mut &'static str,
        fluid: &mut Fluid,
        recipes: &[Recipe],
    ) -> Tick {
        let mut effect = Tick::default();
        if self.processing_ticks >= 0 {
            self.processing_ticks -= 1;
        }
        if self.processing_ticks > 7 && self.processing_ticks < 93 {
            self.send_splash = true;
            effect.splash = true;
        }
        if self.processing_ticks == 7
            && let Some(index) = self.active_recipe
            && let Some(recipe) = recipes.get(index)
        {
            // The original Java destroys the input block before using
            // transformBlock, so it no longer has the input BlockState for
            // copying properties. This reproduces its item/block selection.
            let drained = fluid.amount_mb.min(recipe.fluid_mb);
            fluid.consume(recipe.fluid_mb);
            *block_below = recipe.output;
            effect.completed = Some(Completed {
                block: recipe.output,
                fluid_consumed_mb: drained,
            });
            self.active_recipe = None;
        }

        if self.active_recipe.is_none()
            && let Some((index, _)) = recipes.iter().enumerate().find(|(_, recipe)| {
                recipe.input == *block_below && fluid.accepts(recipe.fluid_mb, recipe.fluid)
            })
        {
            self.active_recipe = Some(index);
            self.processing_ticks = PROCESSING_TICKS;
            effect.started = true;
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;

    #[test]
    fn grass_recipe_matches_the_published_release_json() {
        // Original MIT recipe extracted verbatim from
        // Create More Recipes 1.2.1-fix1's data/cmr/recipe tree.
        let fixture =
            include_str!("../../../tests/fixtures/ws4-create-more-recipes/grass_block.json");
        let data: serde_json::Value = serde_json::from_str(fixture).expect("valid JSON");
        assert_eq!(data["type"], "cmr:block_spouting");
        assert_eq!(data["ingredients"][0]["item"], GRASS_BLOCK.input);
        assert_eq!(data["ingredients"][1]["type"], "fluid_stack");
        assert_eq!(data["ingredients"][1]["fluid"], GRASS_BLOCK.fluid);
        assert_eq!(
            data["ingredients"][1]["amount"].as_u64(),
            Some(u64::from(GRASS_BLOCK.fluid_mb))
        );
        assert_eq!(data["results"][0]["item"]["id"], GRASS_BLOCK.output);
    }

    #[test]
    fn grass_requires_dirt_and_half_bucket_water() {
        let mut spout = BlockSpout::default();
        let mut block = "minecraft:dirt";
        let mut fluid = Fluid::empty();
        assert_eq!(fluid.fill("minecraft:water", 499, false), 499);
        assert_eq!(
            spout.tick(&mut block, &mut fluid, &[GRASS_BLOCK]),
            Tick::default()
        );
        assert_eq!(spout.processing_ticks, -1);
        assert_eq!(fluid.fill("minecraft:lava", 100, false), 0);
        assert_eq!(fluid.fill("minecraft:water", 1000, true), 0);
        assert_eq!(fluid.fill("minecraft:water", 1000, false), 501);
        assert_eq!(fluid.amount_mb, CAPACITY_MB);
        block = "minecraft:stone";
        assert!(!spout.tick(&mut block, &mut fluid, &[GRASS_BLOCK]).started);
        block = "minecraft:dirt";
        assert!(spout.tick(&mut block, &mut fluid, &[GRASS_BLOCK]).started);
        assert_eq!(spout.processing_ticks, 100);
    }

    #[test]
    fn completes_once_at_original_tick_seven_and_consumes_water() {
        let mut spout = BlockSpout::default();
        let mut block = "minecraft:dirt";
        let mut tank = Fluid {
            id: "minecraft:water",
            amount_mb: 500,
        };
        assert!(spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]).started);
        for remaining in (8..=99).rev() {
            let effect = spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]);
            assert!(effect.completed.is_none());
            assert_eq!(spout.processing_ticks, remaining);
        }
        let effect = spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]);
        assert_eq!(spout.processing_ticks, 7);
        assert_eq!(
            effect.completed,
            Some(Completed {
                block: "minecraft:grass_block",
                fluid_consumed_mb: 500,
            })
        );
        assert_eq!(block, "minecraft:grass_block");
        assert_eq!(tank, Fluid::empty());
        assert_eq!(spout.active_recipe, None);
        assert!(!spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]).started);
    }

    #[test]
    fn splash_only_in_middle_of_cycle_and_only_client_packet_resets() {
        let mut spout = BlockSpout::default();
        let mut block = "minecraft:dirt";
        let mut tank = Fluid {
            id: "minecraft:water",
            amount_mb: 500,
        };
        spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]);
        while spout.processing_ticks > 93 {
            assert!(!spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]).splash);
        }
        assert_eq!(spout.processing_ticks, 93);
        assert!(spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]).splash);
        assert_eq!(spout.processing_ticks, 92);
        assert!(spout.send_splash);
        assert!(spout.take_client_splash());
        assert!(!spout.take_client_splash());
    }

    #[test]
    fn restart_restores_ticks_but_reselects_recipe() {
        let mut spout = BlockSpout::default();
        let mut block = "minecraft:dirt";
        let mut tank = Fluid {
            id: "minecraft:water",
            amount_mb: 500,
        };
        spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]);
        for _ in 0..42 {
            spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]);
        }
        assert_eq!(spout.processing_ticks, 58);
        let bytes = fastnbt::to_bytes(&spout.save()).expect("valid nbt");
        let Value::Compound(compound) = fastnbt::from_bytes::<Value>(&bytes).expect("nbt decode")
        else {
            panic!("compound");
        };
        assert_eq!(compound.get("ProcessingTicks"), Some(&Value::Int(58)));
        assert_eq!(compound.len(), 1);
        let saved: SpoutSave = fastnbt::from_bytes(&bytes).expect("nbt decode");
        let mut restored = BlockSpout::load(saved);
        assert!(restored.active_recipe.is_none());
        // The Java code does NOT restore `recipe`. It discovers the still
        // matching recipe again and resets the countdown to 100.
        assert!(restored.tick(&mut block, &mut tank, &[GRASS_BLOCK]).started);
        assert_eq!(restored.processing_ticks, 100);
    }

    #[test]
    fn matching_recipe_is_not_revalidated_before_completion() {
        let mut spout = BlockSpout::default();
        let mut block = "minecraft:dirt";
        let mut tank = Fluid {
            id: "minecraft:water",
            amount_mb: 500,
        };
        spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]);
        tank.amount_mb = 0;
        tank.id = "";
        for _ in 0..93 {
            let effect = spout.tick(&mut block, &mut tank, &[GRASS_BLOCK]);
            if let Some(done) = effect.completed {
                assert_eq!(done.fluid_consumed_mb, 0);
            }
        }
        assert_eq!(block, "minecraft:grass_block");
        assert_eq!(tank.amount_mb, 0);
    }
}
