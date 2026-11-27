//! Typed commands with literal branches and bounded integer arguments.

use crate::{
    Command, InterfacePrefix, InterfaceUnavailable, RegistrationError, Registry, abi,
    content::World, entities::Entity, guard, s,
};
use std::{ffi::c_void, mem::offset_of};

#[derive(Clone, Copy)]
pub struct Commands(InterfacePrefix<abi::CommandsV1>);

pub fn require() -> Result<Commands, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:commands v1.
    unsafe { InterfacePrefix::require(abi::COMMANDS, 1, offset_of!(abi::CommandsV1, string)) }
        .map(Commands)
}

pub enum Argument {
    Literal,
    Integer { min: i32, max: i32 },
    String,
    Entity,
    Entities,
    Player,
    Players,
}

pub struct Node<'a> {
    /// Omit for a child of the root. Otherwise index an earlier node.
    pub parent: Option<u32>,
    pub name: &'a str,
    pub argument: Argument,
    /// Set on nodes that call Mod::command.
    pub tag: Option<u64>,
}

impl Commands {
    pub fn require_selectors(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(offset_of!(abi::CommandsV1, failure))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: abi::COMMANDS,
                version: 1,
            })
    }

    pub fn add(
        self,
        registry: &mut Registry<'_>,
        name: &str,
        description: &str,
        op_level: u8,
        nodes: &[Node<'_>],
    ) -> Result<(), RegistrationError> {
        let nodes: Vec<_> = nodes
            .iter()
            .map(|node| {
                let (kind, min, max) = match node.argument {
                    Argument::Literal => (abi::COMMAND_LITERAL, 0, 0),
                    Argument::Integer { min, max } => (abi::COMMAND_INTEGER, min, max),
                    Argument::String => (abi::COMMAND_STRING, 0, 0),
                    Argument::Entity => (abi::COMMAND_ENTITY, 0, 0),
                    Argument::Entities => (abi::COMMAND_ENTITIES, 0, 0),
                    Argument::Player => (abi::COMMAND_PLAYER, 0, 0),
                    Argument::Players => (abi::COMMAND_PLAYERS, 0, 0),
                };
                abi::CommandNode {
                    name: s(node.name),
                    parent: node.parent.unwrap_or(abi::COMMAND_ROOT),
                    kind,
                    min,
                    max,
                    executable: node.tag.is_some(),
                    tag: node.tag.unwrap_or_default(),
                }
            })
            .collect();
        // SAFETY: registration and all node data are valid for this call.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).add).read())(
                registry.reg,
                s(name),
                s(description),
                op_level,
                nodes.as_ptr(),
                nodes.len(),
            )
        })
    }

    pub fn integer(self, command: &Command<'_>, name: &str) -> Option<i32> {
        let mut out = 0;
        // SAFETY: live command source, borrowed name and space for one integer.
        unsafe {
            ((&raw const (*self.0.ptr).integer).read())(command.source, s(name), &raw mut out)
        }
        .then_some(out)
    }

    pub fn feedback(self, command: &Command<'_>, message: &str) {
        // SAFETY: live command source and borrowed text.
        unsafe { ((&raw const (*self.0.ptr).feedback).read())(command.source, s(message)) }
    }

    pub fn string(self, command: &Command<'_>, name: &str) -> Option<String> {
        let api = self.0.upto(offset_of!(abi::CommandsV1, string))?;
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: checked field, live source and sink.
        unsafe { ((&raw const (*api).string).read())(command.source, s(name), sink) }
            .then(|| String::from_utf8(data).ok())
            .flatten()
    }

    /// Visit the resolved selector in its own world; handles and name are callback-scoped.
    pub fn select<F: FnMut(World<'_>, Entity<'_>, &str)>(
        self,
        command: &Command<'_>,
        name: &str,
        argument: Argument,
        mut f: F,
    ) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::CommandsV1, select)) else {
            return false;
        };
        let kind = match argument {
            Argument::Entity => abi::COMMAND_ENTITY,
            Argument::Entities => abi::COMMAND_ENTITIES,
            Argument::Player => abi::COMMAND_PLAYER,
            Argument::Players => abi::COMMAND_PLAYERS,
            _ => return false,
        };
        // SAFETY: checked field; the callback and context are synchronous.
        unsafe {
            ((&raw const (*api).select).read())(
                command.source,
                s(name),
                kind,
                (&raw mut f).cast(),
                visit::<F>,
            )
        }
    }

    pub fn online_player<F: FnMut(World<'_>, Entity<'_>, &str)>(
        self,
        command: &Command<'_>,
        name: &str,
        mut f: F,
    ) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::CommandsV1, online_player)) else {
            return false;
        };
        // SAFETY: checked field; the callback and context are synchronous.
        unsafe {
            ((&raw const (*api).online_player).read())(
                command.source,
                s(name),
                (&raw mut f).cast(),
                visit::<F>,
            )
        }
    }

    pub fn failure(self, command: &Command<'_>, message: &str) {
        if let Some(api) = self.0.upto(offset_of!(abi::CommandsV1, failure)) {
            // SAFETY: checked field and live source.
            unsafe { ((&raw const (*api).failure).read())(command.source, s(message)) }
        }
    }
}

unsafe extern "C" fn visit<F: FnMut(World<'_>, Entity<'_>, &str)>(
    ctx: *mut c_void,
    world: abi::WorldRef,
    entity: abi::EntityRef,
    name: abi::Str,
) {
    guard("command entity visitor", (), || {
        if let (Some(world), Some(entity)) = (World::borrowed(world), Entity::borrowed(entity)) {
            // SAFETY: context and name are live during the synchronous call.
            unsafe { (&mut *ctx.cast::<F>())(world, entity, name.str()) };
        }
    });
}
