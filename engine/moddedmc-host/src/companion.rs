use pumpkin_core::{net::java::pending::JavaConfigurationExtension, server::Server};

const CHANNEL: &str = "moddedmc:negotiate";
const VERSION: u16 = 1;

pub fn attach(server: &Server) {
    let _ = server
        .java_configuration_extension
        .set(JavaConfigurationExtension {
            channel: CHANNEL,
            answer,
        });
}

fn answer(hello: &[u8]) -> Option<Vec<u8>> {
    let [_, _, _, _, _, _] = hello else {
        return None;
    };
    let mut reply = VERSION.to_be_bytes().to_vec();
    reply.extend_from_slice(&0u32.to_be_bytes());
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::answer;

    #[test]
    fn negotiates_a_baseline_and_ignores_bad_lengths() {
        let baseline = Some(vec![0, 1, 0, 0, 0, 0]);
        assert_eq!(answer(&[0, 1, 0, 0, 0, 0]), baseline);
        assert_eq!(answer(&[0, 2, 255, 255, 255, 255]), baseline);
        assert_eq!(answer(&[0, 1]), None);
    }
}
