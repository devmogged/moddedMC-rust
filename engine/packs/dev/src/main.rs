//! The pack builder will generate one of these per pack. This one is written by hand and
//! links every port in `engine/ports`.

fn main() {
    let launch = pumpkin::Launch::default().on_server_created(moddedmc_host::attach);
    pumpkin::run(launch);
}
