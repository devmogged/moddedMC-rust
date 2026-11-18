//! Links the `TeaVM` object for one mod. `build-mod.ps1` sets the variables. Without them (a plain
//! workspace build or clippy) the Java entry points are stubbed and the plugin refuses to load.

fn main() {
    println!("cargo:rerun-if-env-changed=MODDEDMC_TEAVM_OBJ");
    println!("cargo:rerun-if-env-changed=MODDEDMC_MOD_ID");
    println!("cargo::rustc-check-cfg=cfg(teavm)");
    let id = std::env::var("MODDEDMC_MOD_ID").unwrap_or_else(|_| "fabric-mod".into());
    println!("cargo:rustc-env=MODDEDMC_MOD_ID={id}");
    let Ok(obj) = std::env::var("MODDEDMC_TEAVM_OBJ") else {
        return;
    };
    println!("cargo:rerun-if-changed={obj}");
    println!("cargo:rustc-link-arg={obj}");
    println!("cargo:rustc-cfg=teavm");
}
