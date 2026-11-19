use pumpkin_core::{net::java::pending::JavaConfigurationExtension, server::Server};
use serde_json::json;

use crate::content_pack::{CATALOG, Catalog, hex};

const CHANNEL: &str = "moddedmc:negotiate";
const VERSION: u16 = 1;
const ASSETS: u32 = 1;

pub fn attach(server: &Server) {
    let _ = server
        .java_configuration_extension
        .set(JavaConfigurationExtension {
            channel: CHANNEL,
            answer: |hello| answer(hello, CATALOG.get()),
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
    let mut reply = VERSION.to_be_bytes().to_vec();
    if u16::from_be_bytes([v1, v2]) == VERSION
        && u32::from_be_bytes([a, b, c, d]) & ASSETS != 0
        && let Some(catalog) = catalog
    {
        reply.extend_from_slice(&ASSETS.to_be_bytes());
        reply.extend_from_slice(&catalog.manifest);
        reply.extend_from_slice(&catalog.zip_sha1);
    } else {
        reply.extend_from_slice(&0u32.to_be_bytes());
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
        assert_eq!(answer(&[0, 1, 0, 0, 0, 0], Some(&catalog)), baseline);
        assert_eq!(
            answer(&[0, 2, 255, 255, 255, 255], Some(&catalog)),
            baseline
        );
        assert_eq!(answer(&[0, 1], Some(&catalog)), None);
        let mut expected = vec![0, 1, 0, 0, 0, 1];
        expected.extend_from_slice(&[11; 32]);
        expected.extend_from_slice(&[22; 20]);
        assert_eq!(
            answer(&[0, 1, 255, 255, 255, 255], Some(&catalog)),
            Some(expected)
        );
    }
}
