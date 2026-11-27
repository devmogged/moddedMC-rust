//! Register content blocks with vanilla copper weathering.
use crate::{InterfaceUnavailable, RegistrationError, Registry, abi, require_interface};

#[derive(Clone, Copy)]
pub struct WeatheringCopper(&'static abi::WeatheringCopperV1);

pub fn require() -> Result<WeatheringCopper, InterfaceUnavailable> {
    // SAFETY: WeatheringCopperV1 is the full table for this name and version.
    unsafe { require_interface(abi::WEATHERING_COPPER, 1) }.map(WeatheringCopper)
}

impl WeatheringCopper {
    /// Register four staged unwaxed blocks and their four waxed equivalents, in oxidation order.
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        ids: [&str; 8],
    ) -> Result<(), RegistrationError> {
        let ids = ids.map(abi::Str::from);
        // SAFETY: the staging handle and all eight IDs are valid for this call.
        RegistrationError::from_code(unsafe { (self.0.register)(registry.reg, &raw const ids) })
    }
}
