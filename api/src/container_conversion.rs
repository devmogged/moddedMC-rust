//! Propose conversion of an untouched deferred-loot container before it opens.
use std::{panic::catch_unwind, sync::OnceLock};

use crate::{
    InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World, require_interface,
};

#[derive(Clone, Copy)]
pub struct Conversion(&'static abi::ContainerConversionV1);

pub struct Candidate<'a> {
    pub world: World<'a>,
    pub position: Pos,
    pub state: u32,
    /// Complete named-root Java NBT of the source.
    pub nbt: &'a [u8],
}

pub enum Decision {
    Pass,
    Defer,
    Convert { state: u32, nbt: Vec<u8> },
}

type Propose = fn(Candidate<'_>) -> Decision;
static PROPOSE: OnceLock<Propose> = OnceLock::new();

pub fn require() -> Result<Conversion, InterfaceUnavailable> {
    // SAFETY: this is the published table for container_conversion v1.
    unsafe { require_interface(abi::CONTAINER_CONVERSION, 1) }.map(Conversion)
}

impl Conversion {
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        propose: Propose,
    ) -> Result<(), RegistrationError> {
        PROPOSE
            .set(propose)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the registration handle is borrowed for this call.
        RegistrationError::from_code(unsafe { (self.0.register)(registry.reg, proposal) })
    }
}

unsafe extern "C" fn proposal(candidate: *const abi::ConversionCandidate, out: abi::Sink) -> i32 {
    // SAFETY: the host borrows the candidate, its handles and its NBT for this call.
    let Some(candidate) = (unsafe { candidate.as_ref() }) else {
        return abi::CONVERSION_PASS;
    };
    let (Some(world), Some(propose)) = (World::borrowed(candidate.world), PROPOSE.get()) else {
        return abi::CONVERSION_PASS;
    };
    if !(1..=abi::MAX_BLOCK_ENTITY_BYTES).contains(&candidate.nbt.len) {
        return abi::CONVERSION_PASS;
    }
    // SAFETY: the length is bounded and the bytes live until the callback returns.
    let nbt = unsafe { candidate.nbt.bytes() };
    match catch_unwind(|| {
        propose(Candidate {
            world,
            position: candidate.position,
            state: candidate.state,
            nbt,
        })
    }) {
        Ok(Decision::Pass) => abi::CONVERSION_PASS,
        Ok(Decision::Convert { state, nbt })
            if (1..=abi::MAX_BLOCK_ENTITY_BYTES).contains(&nbt.len()) =>
        {
            // SAFETY: the sink copies the bytes synchronously before they drop.
            unsafe { (out.put)(out.ctx, u64::from(state), abi::Str::new(&nbt)) };
            abi::CONVERSION_REPLACE
        }
        _ => abi::CONVERSION_DEFER,
    }
}
