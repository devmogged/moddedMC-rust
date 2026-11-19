//! Stock pack server, shared by every pack.

fn main() {
    let launch = pumpkin::Launch::default()
        .before_world_open(moddedmc_host::open_world)
        .on_server_created(moddedmc_host::attach);
    pumpkin::run(launch);
}
