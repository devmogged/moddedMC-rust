//! Stock pack server, shared by every pack.

fn main() {
    let launch = pumpkin::Launch::default()
        .before_worlds_loaded(moddedmc_host::load_mods)
        .on_server_created(moddedmc_host::attach);
    pumpkin::run(launch);
}
