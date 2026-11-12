use super::*;

const ID: &str = "123-abc.apps.googleusercontent.com";

fn url(redirect: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/auth?response_type=code&client_id={ID}&redirect_uri={redirect}&scope=a%20b&state=s"
    )
}

fn ok(text: &str) -> bool {
    sign_in_url(text, ID).is_ok()
}

fn refused(text: &str) -> Refused {
    sign_in_url(text, ID).unwrap_err()
}

// Spec mcp-connect, google link succeeds: a bare link line.
#[test]
fn a_bare_link_on_a_local_port_is_accepted() {
    let link = url("http://localhost:8000/oauth2callback");
    assert_eq!(sign_in_url(&format!("{link}\n"), ID), Ok(link.as_str()));
    let loopback = url("http://127.0.0.1:8004");
    assert_eq!(sign_in_url(&loopback, ID), Ok(loopback.as_str()));
}

// 6b2.4: workspace-mcp 2.4.1 prints `   Authorization URL: <link>` with the
// redirect percent-encoded.
#[test]
fn the_real_answer_of_workspace_mcp_is_accepted() {
    let link = url("http%3A%2F%2Flocalhost%3A8000%2Foauth2callback");
    let text = format!(
        "**ACTION REQUIRED: Google Authentication Needed for Google Workspace for 'me@x.org'**\n\n1. Open this URL in your browser:\n   Authorization URL: {link}\n2. After successful authorization, **retry their original command**.\n"
    );
    assert_eq!(sign_in_url(&text, ID), Ok(link.as_str()));
}

// Spec mcp-connect, Links that are refused.
#[test]
fn another_host_path_or_scheme_is_refused() {
    let bad = |l: &str| l.replace("accounts.google.com", "evil.example");
    let l = url("http://localhost:8000/");
    assert!(!ok(&bad(&l)));
    assert!(!ok(&l.replace("/o/oauth2/auth", "/signin")));
    assert!(!ok(&l.replace("https://", "http://")));
    assert!(!ok(&l.replace(
        "accounts.google.com/",
        "accounts.google.com.evil.example/"
    )));
    assert!(!ok(&l.replace(
        "accounts.google.com/",
        "accounts.google.com@evil.example/"
    )));
}

#[test]
fn another_client_id_or_a_repeated_one_is_refused() {
    let l = url("http://localhost:8000/");
    assert!(!ok(&l.replace(ID, "other")));
    assert!(!ok(&format!("{l}&client_id=other")));
    assert!(!ok(&format!("{l}&redirect_uri=http://localhost:8001")));
    assert!(!ok(&l.replace("client_id=", "client_ids=")));
}

#[test]
fn a_redirect_off_the_local_ports_is_refused() {
    for r in [
        "http://localhost:9000/",
        "http://localhost:7999",
        "http://localhost:8005",
        "http://localhost/",
        "http://localhost:80a0/",
        "https://localhost:8000/",
        "http://localhost.evil.example:8000/",
        "http://localhost:8000@evil.example/",
        "http://evil.example:8000/",
        "http://localhost:08000/",
    ] {
        assert!(!ok(&url(r)), "{r}");
    }
    assert!(!ok(&url("http%3A%2F%2Flocalhost%3A9000%2F")));
    assert!(!ok(&url("http%3A%2F%2Flocalhost%3A80%ZZ")));
}

#[test]
fn non_ascii_long_or_spaced_links_are_refused() {
    let l = url("http://localhost:8000/");
    assert!(!ok(&format!("{l}\u{e9}")));
    assert!(!ok(&format!("{l}\u{202e}")));
    assert!(!ok(&format!("{l}&x={}", "a".repeat(2048))));
    assert!(!ok("no link here"));
    assert!(!ok(""));
    // A link with text after it is not a single link.
    assert!(!ok(&format!("{l} and more")));
}

#[test]
fn only_the_first_link_line_counts() {
    let good = url("http://localhost:8000/");
    let evil = "https://evil.example/o/oauth2/auth";
    assert!(!ok(&format!("{evil}\n{good}")));
    assert!(ok(&format!("intro\n\n  {good}  \n{evil}")));
}

// 6b2.4: the file name of an account, as the server builds it.
#[test]
fn the_credentials_names_follow_the_servers_two_schemes() {
    assert_eq!(credentials_names("me@x.org"), ["me@x.org.json"]);
    assert_eq!(
        credentials_names("me+a b@x.org"),
        ["me%2Ba%20b@x.org.json", "me_a_b@x.org.json"]
    );
    // `~` stays in the encoded name; the legacy scheme turns it into `_`.
    assert_eq!(
        credentials_names("a~b@x.org"),
        ["a~b@x.org.json", "a_b@x.org.json"]
    );
}

fn server(env: &[&str]) -> McpServer {
    McpServer {
        name: "google".to_owned(),
        command: "workspace-mcp".to_owned(),
        args: Vec::new(),
        env: env.iter().map(|n| (*n).to_owned()).collect(),
        project: None,
        enabled: true,
    }
}

const NAMES: [&str; 3] = [
    "GOOGLE_OAUTH_CLIENT_ID",
    "GOOGLE_OAUTH_CLIENT_SECRET",
    "USER_GOOGLE_EMAIL",
];

fn planned(env: &[&str], set: &[&str], email: &str) -> Result<Ask, String> {
    let has = |n: &str| set.contains(&n);
    let shown = |n: &str| match n {
        "GOOGLE_OAUTH_CLIENT_ID" if set.contains(&n) => Some(ID.to_owned()),
        "USER_GOOGLE_EMAIL" if set.contains(&n) => Some(email.to_owned()),
        _ => None,
    };
    plan(&server(env), &has, &shown)
}

// Spec mcp-connect, No client id yet.
#[test]
fn missing_values_are_named_with_the_fill_key() {
    assert_eq!(
        planned(&NAMES, &["USER_GOOGLE_EMAIL"], "me@x.org"),
        Err("fill GOOGLE_OAUTH_CLIENT_ID, GOOGLE_OAUTH_CLIENT_SECRET first (f)".to_owned())
    );
    assert_eq!(
        planned(&NAMES, &[], ""),
        Err(
            "fill GOOGLE_OAUTH_CLIENT_ID, GOOGLE_OAUTH_CLIENT_SECRET, USER_GOOGLE_EMAIL first (f)"
                .to_owned()
        )
    );
}

#[test]
fn an_entry_that_does_not_pass_a_value_asks_to_paste_it_again() {
    assert_eq!(
        planned(&NAMES[1..], &NAMES, "me@x.org"),
        Err("the declared google entry does not pass GOOGLE_OAUTH_CLIENT_ID; paste the current entry".to_owned())
    );
}

#[test]
fn all_values_give_the_ask() {
    assert_eq!(
        planned(&NAMES, &NAMES, "me@x.org"),
        Ok(Ask {
            client_id: ID.to_owned(),
            email: "me@x.org".to_owned()
        })
    );
}

#[test]
fn an_e_mail_that_is_not_plain_is_refused() {
    for email in ["me x@y.org", "me\u{e9}@y.org", "no-at", "m\u{202e}@y.org"] {
        let r = planned(&NAMES, &NAMES, email);
        assert_eq!(
            r,
            Err(
                "USER_GOOGLE_EMAIL is not a plain e-mail address; fix it in Connectors (f)"
                    .to_owned()
            ),
            "{email}"
        );
    }
}

// S6b2 review: each refusal has its own notice and none echoes the link.
#[test]
fn each_refusal_is_told_apart() {
    let l = url("http://localhost:8000/");
    assert_eq!(refused("no link here"), Refused::NoLink);
    assert_eq!(
        refused(&l.replace("accounts.google.com", "evil.example")),
        Refused::Shape
    );
    assert_eq!(
        refused(&l.replace("/o/oauth2/auth", "/signin")),
        Refused::Shape
    );
    assert_eq!(refused(&format!("{l}\u{e9}")), Refused::Shape);
    assert_eq!(refused(&l.replace(ID, "other")), Refused::ClientId);
    assert_eq!(refused(&url("http://localhost:9000/")), Refused::Redirect);
    assert_eq!(
        refused(&url("http%3A%2F%2Flocalhost%3A80%ZZ")),
        Refused::Redirect
    );
    assert_eq!(refused(&format!("{l}&client_id=other")), Refused::Duplicate);
    assert_eq!(
        refused(&format!("{l}&redirect_uri=http://localhost:8001")),
        Refused::Duplicate
    );
    let notices: Vec<&str> = [
        Refused::NoLink,
        Refused::Shape,
        Refused::ClientId,
        Refused::Redirect,
        Refused::Duplicate,
    ]
    .iter()
    .map(|r| r.notice())
    .collect();
    for (i, a) in notices.iter().enumerate() {
        assert!(a.starts_with("link failed: "), "{a}");
        assert!(!a.contains("http"), "{a}");
        assert!(notices[i + 1..].iter().all(|b| a != b), "{a}");
    }
    assert_eq!(Refused::NoLink.notice(), NO_LINK);
}

// A key written with an escape is the same key.
#[test]
fn an_escaped_key_name_counts_as_a_repeat() {
    let l = url("http://localhost:8000/");
    assert_eq!(
        refused(&format!("{l}&client%5Fid=other")),
        Refused::Duplicate
    );
    assert_eq!(
        refused(&format!("{l}&redirect%5Furi=http://localhost:8001")),
        Refused::Duplicate
    );
}

// S6b2 review: the credentials folder is not configurable.
const NOT_SUPPORTED: &str = "WORKSPACE_MCP_CREDENTIALS_DIR / GOOGLE_MCP_CREDENTIALS_DIR are not supported; remove them from the entry";

#[test]
fn an_entry_that_names_a_credentials_folder_is_refused() {
    assert_eq!(CREDENTIALS_DIR, NOT_SUPPORTED);
    for name in [
        "WORKSPACE_MCP_CREDENTIALS_DIR",
        "GOOGLE_MCP_CREDENTIALS_DIR",
    ] {
        let mut env = NAMES.to_vec();
        env.push(name);
        let s = server(&env);
        assert_eq!(start_refusal(&s), Some(NOT_SUPPORTED), "{name}");
        // The link refuses too, before any prompt.
        assert_eq!(
            planned(&env, &NAMES, "me@x.org"),
            Err(NOT_SUPPORTED.to_owned()),
            "{name}"
        );
    }
    assert_eq!(start_refusal(&server(&NAMES)), None);
}

#[test]
fn only_google_is_checked() {
    let mut s = server(&["WORKSPACE_MCP_CREDENTIALS_DIR"]);
    s.name = "other".to_owned();
    assert_eq!(start_refusal(&s), None);
}
