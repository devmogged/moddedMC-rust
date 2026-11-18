//! Links the TeaVM object for one mod. build-mod.ps1 sets the variables; without them
//! (workspace clippy) nothing is linked and the crate only type-checks.

fn main() {
    println!("cargo:rerun-if-env-changed=MODDEDMC_TEAVM_OBJ");
    println!("cargo:rerun-if-env-changed=MODDEDMC_WASI_LIBS");
    println!("cargo:rerun-if-env-changed=MODDEDMC_MOD_ID");
    let id = std::env::var("MODDEDMC_MOD_ID").unwrap_or_else(|_| "fabric-mod".into());
    println!("cargo:rustc-env=MODDEDMC_MOD_ID={id}");
    let Ok(obj) = std::env::var("MODDEDMC_TEAVM_OBJ") else {
        return;
    };
    println!("cargo:rerun-if-changed={obj}");
    println!("cargo:rustc-link-arg={obj}");
    if let Ok(libs) = std::env::var("MODDEDMC_WASI_LIBS") {
        println!("cargo:rustc-link-search=native={libs}");
    }
    // TeaVM exceptions are setjmp/longjmp over Wasm exception handling; the rest is POSIX emulation.
    for lib in ["setjmp", "wasi-emulated-mman", "wasi-emulated-signal", "wasi-emulated-process-clocks", "wasi-emulated-getpid"] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
}
