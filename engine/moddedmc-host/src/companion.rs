use pumpkin_core::{net::java::pending::JavaConfigurationExtension, server::Server};
use serde_json::json;

use crate::content_pack::{CATALOG, Catalog, hex};

const CHANNEL: &str = "moddedmc:negotiate";
const VERSION: u16 = 1;
const ASSETS: u32 = 1;
const PLAYER_ACTIONS: u32 = 2;
const BLOCK_DISPLAY_USE: u32 = 4;

const REQUIRED: &str = "This server requires the moddedMC companion mod for Fabric 26.3. Get it from https://github.com/devmogged/moddedMC-rust (companion/), or ask the server owner for the JAR.";

pub fn attach(server: &Server, has_mods: bool) {
    let _ = server
        .java_configuration_extension
        .set(JavaConfigurationExtension {
            channel: CHANNEL,
            answer: |hello| answer(hello, CATALOG.get()),
            required: has_mods.then_some(REQUIRED),
            status: CATALOG.get().map(|catalog| {
                (
                    "moddedmc",
                    json!({
                        "protocol": VERSION,
                        "pack_url": catalog.zip_url,
                        "manifest_sha256": hex(&catalog.manifest),
                    }),
                )
            }),
        });
}

fn answer(hello: &[u8], catalog: Option<&Catalog>) -> Option<Vec<u8>> {
    let [v1, v2, a, b, c, d] = *hello else {
        return None;
    };
    if u16::from_be_bytes([v1, v2]) != VERSION {
        return None;
    }
    let mut reply = VERSION.to_be_bytes().to_vec();
    let supported = u32::from_be_bytes([a, b, c, d])
        & (PLAYER_ACTIONS | if catalog.is_some() { ASSETS | BLOCK_DISPLAY_USE } else { 0 });
    reply.extend_from_slice(&supported.to_be_bytes());
    if supported & ASSETS != 0
        && let Some(catalog) = catalog
    {
        reply.extend_from_slice(&catalog.manifest);
        reply.extend_from_slice(&catalog.zip_sha1);
    }
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::{Catalog, answer};

    #[test]
    fn negotiates_a_baseline_and_ignores_bad_lengths() {
        let baseline = Some(vec![0, 1, 0, 0, 0, 0]);
        let catalog = Catalog {
            manifest: [11; 32],
            zip_sha1: [22; 20],
            zip_url: String::new(),
        };
        assert_eq!(answer(&[0, 1, 0, 0, 0, 1], None), baseline);
        assert_eq!(
            answer(&[0, 1, 0, 0, 0, 3], None),
            Some(vec![0, 1, 0, 0, 0, 2])
        );
        assert_eq!(answer(&[0, 1, 0, 0, 0, 0], Some(&catalog)), baseline);
        assert_eq!(answer(&[0, 2, 255, 255, 255, 255], Some(&catalog)), None);
        assert_eq!(answer(&[0, 1], Some(&catalog)), None);
        let mut expected = vec![0, 1, 0, 0, 0, 7];
        expected.extend_from_slice(&[11; 32]);
        expected.extend_from_slice(&[22; 20]);
        assert_eq!(
            answer(&[0, 1, 255, 255, 255, 255], Some(&catalog)),
            Some(expected)
        );
    }
}
