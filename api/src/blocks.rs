//! `moddedmc:blocks/1`: real modded blocks (see [`crate::abi::BlocksV1`]).
//!
//! Define blocks in [`crate::Mod::define_blocks`], place and read them with [`crate::world`]
//! (`state_id("demo:shaft[axis=x]")`), hand them to players with `Player::give_block`, and
//! react to players in [`crate::Mod::block_event`].

use std::marker::PhantomData;

use crate::{
    Player, Pos, RegistrationError,
    abi::{self, BlocksV1, Defs, Property, Str},
    interface, s,
};

/// Where a mod defines its blocks; valid only during [`crate::Mod::define_blocks`].
pub struct Definitions<'a> {
    pub(crate) defs: Defs,
    pub(crate) table: Option<&'static BlocksV1>,
    pub(crate) _call: PhantomData<(&'a (), *const ())>,
}

impl Definitions<'_> {
    /// Defines `name` (`namespace:path`). Its states are every combination of `properties`,
    /// the last property changing fastest, and the first is the default. `appearances` names
    /// the vanilla state a stock client sees for each state, in the same order.
    ///
    /// Fails with `InvalidDefinition` if the server has no `blocks/1`.
    pub fn block(
        &mut self,
        name: &str,
        properties: &[(&str, &[&str])],
        appearances: &[&str],
    ) -> Result<(), RegistrationError> {
        let Some(table) = self.table else {
            return Err(RegistrationError::InvalidDefinition);
        };
        let values: Vec<Vec<Str>> = properties
            .iter()
            .map(|(_, v)| v.iter().map(|v| s(v)).collect())
            .collect();
        let props: Vec<Property> = properties
            .iter()
            .zip(&values)
            .map(|((name, _), values)| Property {
                name: s(name),
                values: values.as_ptr(),
                count: values.len(),
            })
            .collect();
        let looks: Vec<Str> = appearances.iter().map(|a| s(a)).collect();
        // SAFETY: a live definition handle; everything is borrowed for the call.
        let code = unsafe {
            (table.define_block)(
                self.defs,
                s(name),
                props.as_ptr(),
                props.len(),
                looks.as_ptr(),
                looks.len(),
            )
        };
        RegistrationError::from_code(code)
    }
}

pub(crate) fn table() -> Option<&'static BlocksV1> {
    interface::<BlocksV1>(abi::BLOCKS_V1)
}

/// The blocks interface, if the server offers it.
#[must_use]
pub fn get() -> Option<Blocks> {
    table().map(Blocks)
}

/// Runtime services for `blocks/1` blocks.
#[derive(Clone, Copy)]
pub struct Blocks(&'static BlocksV1);

impl Blocks {
    /// Sends the block at `pos` a `Tick` event `delay` ticks from now (at least 1), if it is still
    /// one of this mod's blocks. Not saved across restarts.
    pub fn schedule_tick(&self, dimension: &str, pos: Pos, delay: u32) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (self.0.schedule_tick)(s(dimension), pos, delay) }
    }

    /// Whether the block at `pos` receives redstone power; `None` if it is not loaded.
    #[must_use]
    pub fn is_powered(&self, dimension: &str, pos: Pos) -> Option<bool> {
        // SAFETY: borrowed for the call.
        match unsafe { (self.0.is_powered)(s(dimension), pos) } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// A player right-clicked the block.
    Use,
    /// A player placed it from its item.
    Place,
    /// It is being broken; `state` is its last state.
    Break,
    /// A tick it asked for with [`Blocks::schedule_tick`].
    Tick,
    /// A neighboring block changed (also redstone power).
    Neighbor,
}

/// Something happened to one of this mod's blocks.
pub struct Event<'a> {
    pub kind: EventKind,
    pub dimension: &'a str,
    pub pos: Pos,
    /// Use `world::get()?.state_name(state)` for its name and properties.
    pub state: u32,
    pub player: Option<Player<'a>>,
}
