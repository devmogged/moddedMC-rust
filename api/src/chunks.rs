//! Asynchronous chunk checks and generation on the server's runtime.

use std::ffi::c_void;

use crate::{InterfaceUnavailable, abi, guard, require_interface, s};

#[derive(Clone, Copy)]
pub struct Chunks(&'static abi::ChunksV1);

pub fn require() -> Result<Chunks, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:chunks v1.
    unsafe { require_interface(abi::CHUNKS, 1) }.map(Chunks)
}

impl Chunks {
    /// Returns whether the check was accepted. Completion reports whether the chunk exists.
    pub fn is_generated(
        self,
        dimension: &str,
        x: i32,
        z: i32,
        complete: impl FnOnce(bool) + Send + 'static,
    ) -> bool {
        submit(self.0.is_generated, dimension, x, z, complete)
    }

    /// Returns whether the request was accepted. Completion reports when the chunk is generated.
    pub fn ensure_generated(
        self,
        dimension: &str,
        x: i32,
        z: i32,
        complete: impl FnOnce(bool) + Send + 'static,
    ) -> bool {
        submit(self.0.ensure_generated, dimension, x, z, complete)
    }
}

fn submit<F: FnOnce(bool) + Send + 'static>(
    request: abi::ChunkRequest,
    dimension: &str,
    x: i32,
    z: i32,
    complete: F,
) -> bool {
    unsafe extern "C" fn done<F: FnOnce(bool) + Send + 'static>(ctx: *mut c_void, result: bool) {
        // SAFETY: an accepted request returns its own context exactly once.
        let complete = unsafe { Box::from_raw(ctx.cast::<F>()) };
        guard("chunk completion", (), || complete(result));
    }

    let ctx = Box::into_raw(Box::new(complete));
    // SAFETY: the world name is borrowed, and ctx can be returned once to done.
    let accepted = unsafe { request(s(dimension), x, z, ctx.cast(), done::<F>) };
    if !accepted {
        // SAFETY: a rejected request never calls back, so the SDK still owns ctx.
        drop(unsafe { Box::from_raw(ctx) });
    }
    accepted
}
