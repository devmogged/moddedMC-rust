# Create ABI requests

## Comparator output (deferred)

Create's Depot supplies an analog signal derived from its held stack and eight output slots. The content registration also needs a per-block analog-output flag. Pumpkin reads the static `HAS_ANALOG_OUTPUT_SIGNAL` state flag before calling the block, and content currently inherits that flag from its vanilla physical template. A generic `moddedmc:block_world` extension can expose `set_analog_output(world: WorldRef, position: Pos, strength: u8) -> bool`. A successful call marks the loaded content block as an analog source and records a value in the range 0 through 15. The host notifies adjacent comparators when the value changes and invalidates the value on removal or unload. The mod recomputes it from its persisted inventory after loading the block.
