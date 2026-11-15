//! More of what a server can say with a value of `secrets.toml` in it: a tool
//! result, a value from the host environment, encoded forms, a colored one.

use std::{os::unix::fs::PermissionsExt, time::Duration};

use super::{
    client::Timeouts,
    external::{
        ExternalSet,
        fake::{host_env, lab, timeouts},
    },
    probe::probe,
};
use crate::core::ai::mcp_secrets::Secrets;

const LONG: Duration = Duration::from_secs(10);
const TOKENS: [&str; 2] = ["k9", "p@ss w0rd/x"];

fn toml_of(token: &str) -> String {
    let value = token.replace('\\', "\\\\").replace('"', "\\\"");
    format!("[clarity]\nCLARITY_API_TOKEN = \"{value}\"\n")
}

/// What a call of a server that answers with `token` gives, the server being
/// given `token` by `secrets.toml` or by the host environment.
async fn called(token: &str, from_host: bool) -> String {
    let lab = lab();
    let mut server = lab.server("clarity", "ping", token);
    server.env = vec!["CLARITY_API_TOKEN".to_owned()];
    let secrets = if from_host {
        Secrets::default()
    } else {
        Secrets::parse(toml_of(token).as_bytes()).secrets
    };
    let token = token.to_owned();
    let host = move |name: &str| match name {
        "CLARITY_API_TOKEN" if from_host => Some(token.clone().into()),
        other => host_env(other),
    };
    let set = ExternalSet::start_with(&[&server], &lab.home, host, &secrets, timeouts(LONG)).await;
    let out = set
        .call(
            "clarity__ping",
            serde_json::json!({}),
            std::future::pending::<()>(),
        )
        .await;
    set.shutdown().await;
    out.text
}

#[tokio::test]
async fn a_tool_result_does_not_show_the_token() {
    for token in TOKENS {
        let text = called(token, false).await;
        assert!(!text.contains(token), "{token}: {text}");
        assert!(text.contains("[redacted]"), "{text}");
    }
}

#[tokio::test]
async fn a_token_from_the_host_environment_is_redacted_too() {
    for token in TOKENS {
        let text = called(token, true).await;
        assert!(!text.contains(token), "{token}: {text}");
        let lab = lab();
        let host = move |name: &str| match name {
            "CLARITY_API_TOKEN" => Some(token.into()),
            other => host_env(other),
        };
        let report = probe(
            &lab.noisy("clarity"),
            &lab.home,
            host,
            None,
            Timeouts {
                handshake: LONG,
                call: LONG,
            },
            std::future::pending::<()>(),
        )
        .await;
        let shown = format!("{report:?}");
        assert!(!shown.contains(token), "{token}: {shown}");
        assert!(shown.contains("[redacted]"), "{shown}");
    }
}

#[tokio::test]
async fn encoded_forms_and_a_colored_value_are_redacted() {
    let lab = lab();
    let path = lab.home.join("secrets.toml");
    for (token, said, leaks) in [
        (
            "p@ss w0rd/x",
            "dsn host=h password=p%40ss%20w0rd%2Fx dbname=d",
            vec!["p%40ss"],
        ),
        ("a\"b\\c", r#"{"pw":"a\"b\\c"}"#, vec![r#"a\"b\\c"#, "a\"b"]),
        ("hunter22", "\u{1b}[31mhunter22\u{1b}[0m", vec!["hunter22"]),
    ] {
        std::fs::write(&path, toml_of(token)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let report = probe(
            &lab.says("clarity", said),
            &lab.home,
            host_env,
            Some(path.clone()),
            timeouts(LONG),
            std::future::pending::<()>(),
        )
        .await;
        let shown = format!("{report:?}");
        assert!(shown.contains("[redacted]"), "{said}: {shown}");
        for leak in leaks {
            assert!(!shown.contains(leak), "{said}: {leak}: {shown}");
        }
    }
}
