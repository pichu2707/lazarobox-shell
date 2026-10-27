//! The context window an engine reports, read from its answers. Pure: the
//! runtime does the asking.

/// The most a window may claim, in tokens: past it the number is garbage.
const MAX_WINDOW: u64 = 100_000_000;

/// The per-slot context size of a llama-server `GET /props` body
/// (`default_generation_settings.n_ctx`); `None` when it is absent, not a
/// positive number, or absurd.
pub fn llamacpp_n_ctx(body: &[u8]) -> Option<usize> {
    let props: serde_json::Value = serde_json::from_slice(body).ok()?;
    let n_ctx = props
        .get("default_generation_settings")?
        .get("n_ctx")?
        .as_u64()?;
    (1..=MAX_WINDOW)
        .contains(&n_ctx)
        .then(|| usize::try_from(n_ctx).ok())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::http::parse_response;

    #[test]
    fn the_recorded_props_reply_gives_its_n_ctx() {
        let raw = include_bytes!("../../../tests/fixtures/llamacpp/props.http");
        let (status, body) = parse_response(raw).expect("a whole reply");
        assert_eq!(status, 200);
        assert_eq!(llamacpp_n_ctx(&body), Some(8_192));
    }

    #[test]
    fn a_reply_without_the_field_or_with_a_bad_one_gives_none() {
        for body in [
            &b""[..],
            b"not json",
            b"{}",
            br#"{"default_generation_settings":{}}"#,
            br#"{"default_generation_settings":{"n_ctx":0}}"#,
            br#"{"default_generation_settings":{"n_ctx":-4}}"#,
            br#"{"default_generation_settings":{"n_ctx":"4096"}}"#,
            br#"{"default_generation_settings":{"n_ctx":99999999999999}}"#,
        ] {
            assert_eq!(
                llamacpp_n_ctx(body),
                None,
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }
}
