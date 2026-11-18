//! Typed commands with literal branches and bounded integer arguments.

use crate::{
    Command, InterfaceUnavailable, RegistrationError, Registry, abi, require_interface, s,
};

#[derive(Clone, Copy)]
pub struct Commands(&'static abi::CommandsV1);

pub fn require() -> Result<Commands, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:commands v1.
    unsafe { require_interface(abi::COMMANDS, 1) }.map(Commands)
}

pub enum Argument {
    Literal,
    Integer { min: i32, max: i32 },
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
            (self.0.add)(
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
        unsafe { (self.0.integer)(command.source, s(name), &raw mut out) }.then_some(out)
    }

    pub fn feedback(self, command: &Command<'_>, message: &str) {
        // SAFETY: live command source and borrowed text.
        unsafe { (self.0.feedback)(command.source, s(message)) }
    }
}
