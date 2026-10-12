//! Classification of an engram `/health` reply.

use crate::core::ai::sanitize::clean;

/// What answers on the memory port.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Health {
    /// engram 3.x: has a well-formed `instance_id` (`[0-9a-f-]{1,64}`) and a
    /// `capabilities` object.
    Ready {
        instance_id: String,
        version: String,
    },
    /// An older engram (1.x): it answers as engram but without them.
    Old { version: String },
    /// Something else is on the port (HTML, another service, an error).
    Foreign,
    /// Nothing answers. Set by the worker on a transport failure; never
    /// produced by `classify_health`.
    Down,
}

/// The longest version text kept, in characters.
pub const VERSION_CAP: usize = 32;
/// The longest instance id accepted, in characters.
pub const INSTANCE_CAP: usize = 64;

/// Whether `id` is 1 to `INSTANCE_CAP` lowercase hex digits or dashes.
fn valid_instance_id(id: &str) -> bool {
    (1..=INSTANCE_CAP).contains(&id.len())
        && id
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f' | b'-'))
}

/// Classifies a `/health` answer from its status and body.
pub fn classify_health(status: u16, body: &[u8]) -> Health {
    if status != 200 {
        return Health::Foreign;
    }
    let Ok(serde_json::Value::Object(reply)) = serde_json::from_slice(body) else {
        return Health::Foreign;
    };
    if reply.get("service").and_then(|v| v.as_str()) != Some("engram") {
        return Health::Foreign;
    }
    let text = |key: &str, cap: usize| -> String {
        let raw = reply.get(key).and_then(|v| v.as_str()).unwrap_or_default();
        clean(raw).trim().chars().take(cap).collect()
    };
    let version = text("version", VERSION_CAP);
    // Taken as is: only `[0-9a-f-]{1,64}` is an id; anything else is not.
    let instance_id = reply
        .get("instance_id")
        .and_then(|v| v.as_str())
        .filter(|id| valid_instance_id(id))
        .unwrap_or_default()
        .to_string();
    let has_capabilities = reply.get("capabilities").is_some_and(|v| v.is_object());
    if instance_id.is_empty() || !has_capabilities {
        Health::Old { version }
    } else {
        Health::Ready {
            instance_id,
            version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::memory::testing::split;

    fn fixture(raw: &[u8]) -> Health {
        let (status, body) = split(raw);
        classify_health(status, body)
    }

    fn ready_body() -> serde_json::Value {
        serde_json::from_slice(split(include_bytes!("fixtures/3.4.0/health_3_4_0.http")).1)
            .expect("recorded body")
    }

    fn classify_json(value: &serde_json::Value) -> Health {
        classify_health(200, value.to_string().as_bytes())
    }

    #[test]
    fn engram_3_4_0_is_ready() {
        assert_eq!(
            fixture(include_bytes!("fixtures/3.4.0/health_3_4_0.http")),
            Health::Ready {
                instance_id: "999100b4a245378075d419132ab1c06e".into(),
                version: "3.4.0".into()
            }
        );
    }

    #[test]
    fn engram_1_x_is_old() {
        assert_eq!(
            fixture(include_bytes!("fixtures/3.4.0/health_1_20.http")),
            Health::Old {
                version: "0.1.0".into()
            }
        );
    }

    #[test]
    fn html_and_another_service_are_foreign() {
        assert_eq!(
            fixture(include_bytes!("fixtures/3.4.0/health_html.http")),
            Health::Foreign
        );
        assert_eq!(
            fixture(include_bytes!("fixtures/3.4.0/health_other_service.http")),
            Health::Foreign
        );
    }

    #[test]
    fn a_non_200_status_is_foreign_whatever_the_body() {
        let (_, body) = split(include_bytes!("fixtures/3.4.0/health_3_4_0.http"));
        for status in [201, 204, 301, 401, 404, 500, 503] {
            assert_eq!(classify_health(status, body), Health::Foreign, "{status}");
        }
    }

    #[test]
    fn a_body_that_is_not_a_json_object_is_foreign() {
        for body in [&b""[..], b"{", b"[]", b"\"engram\"", b"null", b"\xff\xfe"] {
            assert_eq!(classify_health(200, body), Health::Foreign, "{body:?}");
        }
    }

    #[test]
    fn missing_capabilities_or_instance_id_is_old() {
        let mut no_caps = ready_body();
        no_caps.as_object_mut().unwrap().remove("capabilities");
        assert_eq!(
            classify_json(&no_caps),
            Health::Old {
                version: "3.4.0".into()
            }
        );

        let mut no_id = ready_body();
        no_id.as_object_mut().unwrap().remove("instance_id");
        assert!(matches!(classify_json(&no_id), Health::Old { .. }));
    }

    #[test]
    fn capabilities_that_is_not_an_object_or_an_empty_id_is_old() {
        for (key, value) in [
            ("capabilities", serde_json::json!("yes")),
            ("capabilities", serde_json::json!(null)),
            ("capabilities", serde_json::json!([1])),
            ("instance_id", serde_json::json!("")),
            ("instance_id", serde_json::json!(7)),
            ("instance_id", serde_json::json!("\u{1b}[0m")),
        ] {
            let mut body = ready_body();
            body[key] = value.clone();
            assert!(
                matches!(classify_json(&body), Health::Old { .. }),
                "{key}={value}"
            );
        }
    }

    #[test]
    fn an_empty_capabilities_object_is_still_ready() {
        let mut body = ready_body();
        body["capabilities"] = serde_json::json!({});
        assert!(matches!(classify_json(&body), Health::Ready { .. }));
    }

    #[test]
    fn the_service_must_be_engram() {
        let mut body = ready_body();
        body["service"] = serde_json::json!("engram2");
        assert_eq!(classify_json(&body), Health::Foreign);
        body.as_object_mut().unwrap().remove("service");
        assert_eq!(classify_json(&body), Health::Foreign);
    }

    #[test]
    fn the_version_is_sanitized_and_capped() {
        let mut body = ready_body();
        body["version"] = serde_json::json!(format!("\u{1b}[31m{}", "9".repeat(100)));
        let Health::Ready { version, .. } = classify_json(&body) else {
            panic!("ready");
        };
        assert_eq!(version, "9".repeat(VERSION_CAP));
    }

    #[test]
    fn an_instance_id_must_be_lowercase_hex_and_dashes_up_to_64() {
        for good in [
            "a",
            "0f",
            "999100b4a245378075d419132ab1c06e",
            &"a-0".repeat(21),
        ] {
            let mut body = ready_body();
            body["instance_id"] = serde_json::json!(good);
            assert!(
                matches!(classify_json(&body), Health::Ready { instance_id, .. } if instance_id == good),
                "{good:?}"
            );
        }
        for bad in [
            "a".repeat(65),
            "A1".to_string(),
            "abc def".to_string(),
            " abc".to_string(),
            "abc\n".to_string(),
            "g1".to_string(),
            "abc/../x".to_string(),
            "\u{ff10}\u{ff11}".to_string(),
            "abc\u{1b}[0m".to_string(),
            "<b>".to_string(),
        ] {
            let mut body = ready_body();
            body["instance_id"] = serde_json::json!(bad);
            assert!(
                matches!(classify_json(&body), Health::Old { .. }),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_missing_version_is_empty() {
        let mut body = ready_body();
        body.as_object_mut().unwrap().remove("version");
        assert!(matches!(
            classify_json(&body),
            Health::Ready { version, .. } if version.is_empty()
        ));
    }
}
