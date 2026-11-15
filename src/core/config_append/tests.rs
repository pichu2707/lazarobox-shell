use super::*;
use crate::core::ai::{
    mcp_catalog::{
        connect::{Input, SERVICES, plan},
        entry, snippet,
    },
    mcp_secrets::{Client, client_json, read_client},
};

const NAMES: [&str; 2] = ["gsc", "google"];

/// The catalog entries Connect Google appends, as the plan writes them.
fn snippets() -> String {
    format!(
        "{}\n{}",
        snippet(entry("gsc").unwrap()),
        snippet(entry("google").unwrap())
    )
}

const OLD: &str = "# my settings\nmouse = false # keep\n\n[[ai.mcp.servers]]\nname = \"docs\"\ncommand = \"/usr/bin/docs-mcp\"\nenv = [\"TOKEN\"]\n";

fn ok(old: &str) -> String {
    appended(old, &snippets(), &NAMES).unwrap()
}

fn refused(old: &str) -> AppendRefusal {
    appended(old, &snippets(), &NAMES).unwrap_err()
}

fn servers(text: &str) -> Vec<String> {
    let parsed = Config::parse(text.strip_prefix('\u{feff}').unwrap_or(text)).unwrap();
    parsed
        .ai
        .mcp_servers()
        .iter()
        .map(|s| s.name.clone())
        .collect()
}

// S4: the old bytes are a prefix of the new file, comments included, and one
// empty line comes before the block.
#[test]
fn the_old_bytes_stay_a_prefix_and_one_empty_line_comes_before_the_block() {
    let out = ok(OLD);
    assert_eq!(out, format!("{OLD}\n{}", snippets()));
    assert_eq!(servers(&out), ["docs", "gsc", "google"]);
    // A file that already ends in an empty line gets no second one.
    let old = format!("{OLD}\n");
    assert_eq!(ok(&old), format!("{old}{}", snippets()));
}

#[test]
fn a_file_without_a_final_newline_gets_one_then_the_empty_line() {
    let old = "mouse = false";
    assert_eq!(ok(old), format!("{old}\n\n{}", snippets()));
}

#[test]
fn an_empty_or_missing_file_holds_only_the_block() {
    assert_eq!(ok(""), snippets());
    assert_eq!(servers(&ok("")), NAMES);
}

#[test]
fn a_crlf_file_gets_crlf_lines_and_keeps_its_bytes() {
    let old = "# mine\r\nmouse = false\r\n";
    let out = ok(old);
    assert_eq!(
        out,
        format!("{old}\r\n{}", snippets().replace('\n', "\r\n"))
    );
    assert_eq!(servers(&out), NAMES);
}

#[test]
fn a_bom_is_kept_in_front() {
    let old = "\u{feff}mouse = false\n";
    let out = ok(old);
    assert_eq!(out, format!("{old}\n{}", snippets()));
    assert!(out.starts_with('\u{feff}'));
}

// S4: an inline array cannot be extended by a `[[...]]` header.
#[test]
fn an_inline_servers_array_is_refused() {
    let inline = [
        "[ai]\nmcp.servers = [{ name = \"docs\", command = \"d\" }]\n",
        "[ai.mcp]\nservers = [{ name = \"docs\", command = \"d\" }]\n",
        "ai.mcp.servers = []\n",
    ];
    for old in inline {
        assert_eq!(refused(old), AppendRefusal::WouldBreak, "{old:?}");
    }
}

#[test]
fn a_name_already_declared_in_any_form_is_refused() {
    let declared = [
        "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"mine\"\n",
        // Limited to a project.
        "[[ai.mcp.servers]]\nname = \"google\"\ncommand = \"g\"\nproject = \"/home/u/p\"\n",
        // Disabled, or broken so the parser skips it.
        "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"g\"\nenabled = false\n",
        "[[ai.mcp.servers]]\nname = \"google\"\ncommand = { x = 1 }\n",
    ];
    for old in declared {
        assert!(
            matches!(refused(old), AppendRefusal::Declared(_)),
            "{old:?}"
        );
    }
    assert_eq!(
        refused(declared[0]).notice(),
        "config.toml already declares gsc"
    );
}

#[test]
fn an_unparsable_old_file_is_refused_with_its_line() {
    let old = "mouse = false\n[broken\n";
    assert_eq!(refused(old), AppendRefusal::NotToml(2));
}

#[test]
fn entries_that_do_not_add_exactly_the_names_are_refused() {
    // The snippet declares another name than the one announced.
    let out = appended(OLD, &snippet(entry("gsc").unwrap()), &["google"]);
    assert_eq!(out.unwrap_err(), AppendRefusal::ServersDiffer);
    // A snippet the parser skips (no command) adds nothing.
    let out = appended(OLD, "[[ai.mcp.servers]]\nname = \"gsc\"\n", &["gsc"]);
    assert_eq!(out.unwrap_err(), AppendRefusal::ServersDiffer);
}

#[test]
fn a_result_over_the_bound_is_refused() {
    let old = format!("# {}\n", "x".repeat(MAX_BYTES as usize));
    assert_eq!(refused(&old), AppendRefusal::File(Refusal::TooLarge));
}

// Every refusal is one line and never holds a text of the file.
#[test]
fn every_refusal_notice_is_one_line() {
    let path = std::path::PathBuf::from("/home/u/.config/lazarobox/config.toml");
    let all = [
        AppendRefusal::NotToml(3),
        AppendRefusal::WouldBreak,
        AppendRefusal::Declared("gsc".to_owned()),
        AppendRefusal::ServersDiffer,
        AppendRefusal::File(Refusal::TooLarge),
        AppendRefusal::File(Refusal::Changed),
        AppendRefusal::File(Refusal::FileSymlink),
        AppendRefusal::File(Refusal::FileWritable(path.clone())),
        AppendRefusal::File(Refusal::ReadOnly(path)),
    ];
    for refusal in all {
        let notice = refusal.notice();
        assert!(!notice.contains('\n') && !notice.is_empty(), "{notice:?}");
        assert!(notice.contains(FILE), "{notice:?}");
        assert!(!notice.contains("secrets.toml"), "{notice:?}");
    }
}

fn client() -> Client {
    read_client(&client_json("123-abc.apps.googleusercontent.com", "GOCSPX-abc").unwrap()).unwrap()
}

// The effect built from a plan carries its values, its entries and the
// digest of the file the plan was shown with, and never prints a value.
#[test]
fn the_write_of_a_plan_carries_its_values_entries_and_digest() {
    let client = client();
    let has = |_: &str, _: &str| false;
    let plan = plan(&Input {
        servers: &[],
        names: &[],
        has: &has,
        home: Some("/home/ana"),
        client: Some(&client),
        client_file: None,
        route: crate::core::ai::mcp_catalog::connect::ClientRoute::Own,
        email: "ana@example.com",
        chosen: &SERVICES,
    })
    .unwrap();
    let snippets = plan.snippets();
    let write = Connect::new(plan, Some(OLD.as_bytes()));
    assert_eq!(write.names, NAMES);
    assert_eq!(write.snippets, snippets);
    assert_eq!(write.expected, Some(digest(OLD.as_bytes())));
    assert!(write.set.iter().any(|(t, n, v)| {
        (*t, *n, v.expose()) == ("google", "GOOGLE_OAUTH_CLIENT_SECRET", "GOCSPX-abc")
    }));
    assert!(!format!("{write:?}").contains("GOCSPX-abc"));
    let empty = Plan {
        append: Vec::new(),
        set: Vec::new(),
        kept: Vec::new(),
        installs: Vec::new(),
        declared: Vec::new(),
    };
    assert_eq!(Connect::new(empty, None).expected, None);
}
