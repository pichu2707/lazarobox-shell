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

/// The model's own (maximum) context window from a `POST /api/show` reply:
/// `model_info["<general.architecture>.context_length"]`. `None` when the
/// architecture or the key is missing, or the number is not positive or is
/// absurd.
pub fn ollama_context_length(body: &[u8]) -> Option<usize> {
    let reply: serde_json::Value = serde_json::from_slice(body).ok()?;
    let info = reply.get("model_info")?;
    let arch = info.get("general.architecture")?.as_str()?;
    let length = info.get(format!("{arch}.context_length"))?.as_u64()?;
    (1..=MAX_WINDOW)
        .contains(&length)
        .then(|| usize::try_from(length).ok())?
}

/// The `num_ctx` to ask Ollama for: the model's `max` window held to the
/// user's `cap`. `None` when the window is unknown: asking for the cap could
/// pass what the model was trained for. The budget and the request body both
/// use this, so they always agree.
pub fn ollama_num_ctx(max: Option<usize>, cap: usize) -> Option<usize> {
    max.map(|max| max.min(cap))
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
        assert_eq!(llamacpp_n_ctx(&body), Some(16_384));
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

    #[test]
    fn the_recorded_show_reply_gives_its_context_length() {
        let body =
            include_bytes!("../../../tests/fixtures/ollama/api-show-llama3.2-3b-ctx-full.json");
        assert_eq!(ollama_context_length(body), Some(131_072));
    }

    #[test]
    fn an_architecture_key_picks_the_matching_context_length() {
        let body = br#"{"model_info":{"general.architecture":"qwen3","llama.context_length":1,"qwen3.context_length":40960}}"#;
        assert_eq!(ollama_context_length(body), Some(40_960));
    }

    #[test]
    fn a_show_reply_without_a_usable_window_gives_none() {
        for body in [
            &b""[..],
            b"not json",
            b"{}",
            br#"{"model_info":{}}"#,
            br#"{"model_info":{"llama.context_length":4096}}"#,
            br#"{"model_info":{"general.architecture":"llama"}}"#,
            br#"{"model_info":{"general.architecture":"llama","llama.context_length":0}}"#,
            br#"{"model_info":{"general.architecture":"llama","llama.context_length":"4096"}}"#,
            br#"{"model_info":{"general.architecture":"llama","llama.context_length":99999999999999}}"#,
            br#"{"model_info":{"general.architecture":7,"llama.context_length":4096}}"#,
        ] {
            assert_eq!(
                ollama_context_length(body),
                None,
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn num_ctx_is_the_smaller_of_the_window_and_the_cap() {
        assert_eq!(ollama_num_ctx(Some(131_072), 16_384), Some(16_384));
        assert_eq!(ollama_num_ctx(Some(8_192), 16_384), Some(8_192));
        assert_eq!(ollama_num_ctx(Some(4_096), 4_096), Some(4_096));
        assert_eq!(ollama_num_ctx(None, 16_384), None);
    }
}
