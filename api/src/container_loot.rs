//! Non-destructive generation of loot inventory slots.
use crate::{InterfaceUnavailable, Player, Pos, abi, content::World, require_interface};
use std::ffi::c_void;

#[derive(Clone, Copy)]
pub struct ContainerLoot(&'static abi::ContainerLootV1);

pub fn require() -> Result<ContainerLoot, InterfaceUnavailable> {
    // SAFETY: this is the published table for container_loot v1.
    unsafe { require_interface(abi::CONTAINER_LOOT, 1) }.map(ContainerLoot)
}

struct Output {
    slots: Vec<Option<Vec<u8>>>,
    valid: bool,
}

unsafe extern "C" fn collect(ctx: *mut c_void, index: u64, bytes: abi::Str) {
    // SAFETY: the synchronous call borrows this output.
    let out = unsafe { &mut *ctx.cast::<Output>() };
    let Some(slot) = usize::try_from(index)
        .ok()
        .and_then(|i| out.slots.get_mut(i))
    else {
        out.valid = false;
        return;
    };
    if slot.is_some() || bytes.len == 0 || bytes.len > abi::MAX_STACK_BYTES {
        out.valid = false;
        return;
    }
    // SAFETY: host borrows the serialized stack for this callback.
    *slot = Some(unsafe { bytes.bytes() }.to_vec());
}

impl ContainerLoot {
    pub fn generate(
        self,
        world: World<'_>,
        player: &Player<'_>,
        table: &str,
        seed: i64,
        pos: Pos,
        size: usize,
    ) -> Option<Vec<Option<Vec<u8>>>> {
        if !(1..=54).contains(&size) || table.is_empty() || table.len() > 256 {
            return None;
        }
        let mut out = Output {
            slots: vec![None; size],
            valid: true,
        };
        let sink = abi::Sink {
            ctx: std::ptr::from_mut(&mut out).cast(),
            put: collect,
        };
        // SAFETY: all handles and output are live for this synchronous call.
        let ok = unsafe {
            (self.0.generate)(
                world.raw(),
                player.raw,
                abi::Str::from(table),
                seed,
                pos,
                size,
                sink,
            )
        };
        (ok && out.valid).then_some(out.slots)
    }
}
