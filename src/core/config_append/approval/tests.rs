use super::*;
use crate::core::ai::config::McpApproval::{All, Ask, Reads};

const TWO: &str = "# my settings\nmouse = false\n\n# search\n[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"uvx\"\nargs = [\"x\"]\n\n[[ai.mcp.servers]]\nname = \"ga4\"\ncommand = \"g\"\n";

fn level_of(text: &str, name: &str) -> McpApproval {
    Config::parse(text)
        .unwrap()
        .ai
        .mcp_servers()
        .iter()
        .find(|s| s.name == name)
        .unwrap()
        .approval
}

// mcp-connect: Confirmed change adds the key
#[test]
fn a_missing_key_is_added_after_the_entrys_last_key() {
    let out = edit_approval(TWO, "gsc", Reads).unwrap();
    assert_eq!(
        out,
        TWO.replace("args = [\"x\"]\n", "args = [\"x\"]\napproval = \"reads\"\n")
    );
    assert_eq!(level_of(&out, "gsc"), Reads);
    assert_eq!(level_of(&out, "ga4"), Ask);
    // The last entry of a file without a final newline.
    let bare = TWO.trim_end();
    let out = edit_approval(bare, "ga4", All).unwrap();
    assert_eq!(out, format!("{bare}\napproval = \"all\"\n"));
}

// mcp-connect: Confirmed change replaces the value only
#[test]
fn an_existing_key_changes_only_its_value() {
    let old = TWO.replace(
        "args = [\"x\"]\n",
        "args = [\"x\"]\n# why\napproval = \"reads\"  # only reads\n",
    );
    let out = edit_approval(&old, "gsc", All).unwrap();
    assert_eq!(out, old.replace("\"reads\"", "\"all\""));
    // An invalid value is replaced too.
    let bad = TWO.replace("args = [\"x\"]\n", "args = [\"x\"]\napproval = 7\n");
    let out = edit_approval(&bad, "gsc", Reads).unwrap();
    assert_eq!(level_of(&out, "gsc"), Reads);
    assert_eq!(out, bad.replace("= 7", "= \"reads\""));
}

// mcp-connect: Lowering to ask removes the key
#[test]
fn ask_removes_the_line_and_keeps_the_comment_above() {
    let old = TWO.replace(
        "args = [\"x\"]\n",
        "args = [\"x\"]\n# why\napproval = \"all\"  # note\n",
    );
    let out = edit_approval(&old, "gsc", Ask).unwrap();
    assert_eq!(
        out,
        TWO.replace("args = [\"x\"]\n", "args = [\"x\"]\n# why\n")
    );
    assert_eq!(level_of(&out, "gsc"), Ask);
}

#[test]
fn removing_an_absent_key_or_setting_the_same_text_writes_nothing() {
    assert_eq!(
        edit_approval(TWO, "gsc", Ask),
        Err(AppendRefusal::Unchanged)
    );
    let old = TWO.replace("args = [\"x\"]\n", "args = [\"x\"]\napproval = \"all\"\n");
    assert_eq!(
        edit_approval(&old, "gsc", All),
        Err(AppendRefusal::Unchanged)
    );
}

#[test]
fn crlf_and_a_bom_keep_their_bytes() {
    let crlf = TWO.replace('\n', "\r\n");
    let out = edit_approval(&crlf, "gsc", Reads).unwrap();
    assert_eq!(
        out,
        crlf.replace(
            "args = [\"x\"]\r\n",
            "args = [\"x\"]\r\napproval = \"reads\"\r\n"
        )
    );
    let bom = format!("\u{feff}{TWO}");
    let out = edit_approval(&bom, "gsc", Reads).unwrap();
    assert!(out.starts_with("\u{feff}# my settings"));
    assert_eq!(level_of(&out, "gsc"), Reads);
    // Removal from a CRLF file takes the whole line, break included.
    let on = crlf.replace(
        "args = [\"x\"]\r\n",
        "args = [\"x\"]\r\napproval = \"all\"\r\n",
    );
    assert_eq!(edit_approval(&on, "gsc", Ask).unwrap(), crlf);
}

#[test]
fn a_key_inside_a_multi_line_string_or_a_comment_is_not_the_key() {
    let old = "[[ai.mcp.servers]]\nname = \"gsc\"\n# approval = \"all\"\ncommand = \"\"\"\napproval = 1\n\"\"\"\n";
    let out = edit_approval(old, "gsc", Reads).unwrap();
    assert_eq!(out, format!("{old}approval = \"reads\"\n"));
}

// mcp-connect: An entry declared twice is not edited; The change is refused
#[test]
fn refusals_say_why_and_write_nothing() {
    let project = "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"g\"\nproject = \"/p\"\n";
    let cases: [(String, AppendRefusal); 5] = [
        (TWO.to_owned(), AppendRefusal::NotDeclared("zzz".into())),
        (
            format!(
                "{TWO}\n{project}\n[[ai.mcp.servers]]\nname = \"zzz\"\ncommand = \"z\"\nproject = \"/p\"\n"
            ),
            AppendRefusal::Limited("zzz".into()),
        ),
        (
            format!(
                "{TWO}\n[[ai.mcp.servers]]\nname = \"zzz\"\ncommand = \"z\"\n\n[[ai.mcp.servers]]\nname = \"zzz\"\ncommand = \"z\"\nproject = \"/p\"\n"
            ),
            AppendRefusal::Twice,
        ),
        (
            "[ai.mcp]\nservers = [{ name = \"zzz\", command = \"z\" }]\n".into(),
            AppendRefusal::Inline,
        ),
        ("[broken\n".into(), AppendRefusal::NotToml(1)),
    ];
    for (old, refusal) in cases {
        assert_eq!(edit_approval(&old, "zzz", Reads), Err(refusal), "{old:?}");
    }
    assert_eq!(
        AppendRefusal::Twice.notice(),
        "declared more than once; set approval by hand"
    );
    assert_eq!(
        AppendRefusal::Limited("gsc".into()).notice(),
        "gsc is limited to a project; set approval by hand"
    );
    // No servers at all.
    assert_eq!(
        edit_approval("mouse = false\n", "gsc", Reads),
        Err(AppendRefusal::NotDeclared("gsc".into()))
    );
}

// An entry with a sub-table keeps the key before it, and the result must
// parse to the same servers.
#[test]
fn the_key_goes_before_a_sub_table_of_the_entry() {
    let old = "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"g\"\n\n[ai.other]\nx = 1\n";
    let out = edit_approval(old, "gsc", All).unwrap();
    assert_eq!(
        out,
        "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"g\"\napproval = \"all\"\n\n[ai.other]\nx = 1\n"
    );
}

// An entry the parser skips (no command) cannot take a level: the result
// would not read as the named server at that level.
#[test]
fn a_result_that_does_not_read_back_is_refused() {
    let old = "[[ai.mcp.servers]]\nname = \"gsc\"\n";
    assert_eq!(
        edit_approval(old, "gsc", All),
        Err(AppendRefusal::ServersDiffer)
    );
}
