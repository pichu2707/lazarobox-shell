//! Pure edit of `config.toml` text: changes only the menu-owned keys and
//! keeps everything else byte-for-byte (comments, order, whitespace).

use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value};

use super::config::ConfigEdit;

/// Why an edit was refused; nothing is written in either case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditError {
    /// The existing text is not valid TOML.
    Unparseable,
    /// The named entry exists but is not a table.
    NotATable(&'static str),
    /// The owned key exists but holds a table (`[table.key]` or an inline
    /// table), so writing a plain value would destroy its content.
    KeyIsTable {
        table: &'static str,
        key: &'static str,
    },
}

/// Applies `edits` to the existing file text (`None` means no file yet) and
/// returns the new text.
///
/// Line endings: `toml_edit` keeps a `\r\n` inside a multi-line string but
/// turns every other one into `\n`. So the file's style is the ending of its
/// first line break outside string content (`\n` when there is none), and
/// that style is restored on every line break outside strings. Line breaks
/// inside multi-line strings are never touched. A file that mixes endings
/// outside strings therefore ends up uniform, in the style of its first one.
///
/// Known limits, not fixed on purpose: a missing trailing newline gets
/// added, and a leading UTF-8 BOM is dropped.
pub fn apply_edit(existing: Option<&str>, edits: &[ConfigEdit]) -> Result<String, EditError> {
    let crlf = existing.is_some_and(|text| {
        breaks_outside_strings(text)
            .first()
            .is_some_and(|&at| at > 0 && text.as_bytes()[at - 1] == b'\r')
    });
    let mut doc = match existing {
        Some(text) => text
            .parse::<DocumentMut>()
            .map_err(|_| EditError::Unparseable)?,
        None => DocumentMut::new(),
    };
    for edit in edits {
        if edit.is_removal() {
            remove_key(&mut doc, edit);
            continue;
        }
        if edit.table.is_empty() {
            set_root_key(&mut doc, edit)?;
            continue;
        }
        // Comments of a file with no entries are the document's trailing
        // text; they move in front of the first table written.
        let mut head = doc.as_table().is_empty().then(|| {
            let mut head = doc.trailing().as_str().unwrap_or_default().to_owned();
            if !head.is_empty() {
                if !head.ends_with('\n') {
                    head.push('\n');
                }
                head.push('\n');
            }
            doc.set_trailing("");
            head
        });
        let table = walk_to_table(&mut doc, edit, &mut head)?;
        set_key(table, edit)?;
    }
    let text = doc.to_string();
    if !crlf {
        return Ok(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut done = 0;
    for at in breaks_outside_strings(&text) {
        out.push_str(&text[done..at]);
        if !out.ends_with('\r') {
            out.push('\r');
        }
        done = at;
    }
    out.push_str(&text[done..]);
    Ok(out)
}

/// Writes `edit.key` in `table`. An existing value is replaced in its slot so
/// the key's own decor (leading comment, spacing) stays, and the old value's
/// decor is kept so a trailing `# comment` survives.
fn set_key(table: &mut dyn TableLike, edit: &ConfigEdit) -> Result<(), EditError> {
    let mut value = edit_value(edit);
    match table.get_mut(edit.key) {
        Some(slot) => {
            if slot.is_table_like() || slot.is_array_of_tables() {
                return Err(EditError::KeyIsTable {
                    table: edit.table,
                    key: edit.key,
                });
            }
            if let Some(old) = slot.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *slot = Item::Value(value);
        }
        None => {
            table.insert(edit.key, Item::Value(value));
        }
    }
    Ok(())
}

/// Deletes `edit.key` from its table; a table left empty goes too. A missing
/// table or key is left alone: a removal never creates anything.
fn remove_key(doc: &mut DocumentMut, edit: &ConfigEdit) {
    fn table_at<'a>(root: &'a mut dyn TableLike, path: &str) -> Option<&'a mut dyn TableLike> {
        path.split('.')
            .try_fold(root, |table, name| table.get_mut(name)?.as_table_like_mut())
    }
    if let Some(table) = table_at(doc.as_table_mut(), edit.table) {
        table.remove(edit.key);
    }
    let (parent, leaf) = edit.table.rsplit_once('.').unwrap_or(("", edit.table));
    let root: &mut dyn TableLike = doc.as_table_mut();
    let parent = if parent.is_empty() {
        Some(root)
    } else {
        table_at(root, parent)
    };
    if let Some(parent) = parent
        && parent
            .get(leaf)
            .and_then(Item::as_table_like)
            .is_some_and(|table| table.is_empty())
    {
        parent.remove(leaf);
    }
}

/// Writes a root key (`edit.table == ""`). `toml_edit` prints the root's
/// key-value pairs before every table header, so a new key lands above the
/// first table. In a file with no entries the comments are the document's
/// trailing text; they stay above the key.
fn set_root_key(doc: &mut DocumentMut, edit: &ConfigEdit) -> Result<(), EditError> {
    let head = doc.as_table().is_empty().then(|| {
        let mut head = doc.trailing().as_str().unwrap_or_default().to_owned();
        if !head.is_empty() && !head.ends_with('\n') {
            head.push('\n');
        }
        doc.set_trailing("");
        head
    });
    set_key(doc.as_table_mut(), edit)?;
    if let Some(head) = head.filter(|head| !head.is_empty())
        && let Some(mut key) = doc.as_table_mut().key_mut(edit.key)
    {
        key.leaf_decor_mut().set_prefix(head);
    }
    Ok(())
}

/// The TOML value of an edit: a boolean key (see [`ConfigEdit::is_flag`])
/// is written as a TOML boolean, every other owned key as a string.
fn edit_value(edit: &ConfigEdit) -> Value {
    if edit.is_flag() {
        Value::from(edit.value == "true")
    } else {
        Value::from(edit.value.as_ref())
    }
}

/// Walks the dotted `edit.table` path from the root and returns the leaf
/// table. A missing segment is created: parents implicit (no empty header),
/// the leaf explicit, dotted under a dotted parent and inline under an inline
/// one. `head` is the pending comment prefix of a comment-only file; it goes
/// on the leaf table when that is created.
fn walk_to_table<'a>(
    doc: &'a mut DocumentMut,
    edit: &ConfigEdit,
    head: &mut Option<String>,
) -> Result<&'a mut dyn TableLike, EditError> {
    let mut table: &mut dyn TableLike = doc.as_table_mut();
    let mut inline = false;
    let mut end = 0;
    let mut segments = edit.table.split('.').peekable();
    while let Some(segment) = segments.next() {
        end += segment.len();
        let last = segments.peek().is_none();
        if !table.contains_key(segment) {
            let child = if inline {
                Item::Value(Value::InlineTable(InlineTable::new()))
            } else {
                let mut child = Table::new();
                child.set_implicit(!last);
                child.set_dotted(table.is_dotted());
                if last && let Some(head) = head.take() {
                    child.decor_mut().set_prefix(head);
                }
                Item::Table(child)
            };
            table.insert(segment, child);
        }
        let item = table
            .get_mut(segment)
            .ok_or(EditError::NotATable(&edit.table[..end]))?;
        inline = matches!(item, Item::Value(Value::InlineTable(_)));
        table = item
            .as_table_like_mut()
            .ok_or(EditError::NotATable(&edit.table[..end]))?;
        end += 1;
    }
    Ok(table)
}

/// Byte offsets of the `\n` that end a line outside any string literal.
/// Comments and strings are skipped, so a quote inside a comment or a line
/// break inside a multi-line string is not mistaken for structure. The text
/// must be valid TOML.
fn breaks_outside_strings(text: &str) -> Vec<usize> {
    let b = text.as_bytes();
    let mut breaks = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\n' => breaks.push(i),
            b'#' => {
                while i + 1 < b.len() && b[i + 1] != b'\n' {
                    i += 1;
                }
            }
            quote @ (b'"' | b'\'') => {
                let triple = [quote; 3];
                let multi = b[i..].starts_with(&triple);
                i += if multi { 3 } else { 1 };
                while i < b.len() {
                    if quote == b'"' && b[i] == b'\\' {
                        i += 2;
                    } else if multi && b[i..].starts_with(&triple) {
                        // Up to two more quotes may belong to the content.
                        i += 3;
                        for _ in 0..2 {
                            i += usize::from(b.get(i) == Some(&quote));
                        }
                        break;
                    } else if !multi && (b[i] == quote || b[i] == b'\n') {
                        // A closing quote; the newline arm is only defensive.
                        i += usize::from(b[i] == quote);
                        break;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    breaks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{BarPosition, Config};

    const fn edit(table: &'static str, value: &'static str) -> ConfigEdit {
        ConfigEdit {
            table,
            key: "position",
            value: std::borrow::Cow::Borrowed(value),
        }
    }

    const COMMENTED: &str = "\
# my shell
mouse = false   # not used yet

[statusline]
# where the status goes
position   =   \"bottom\"   # keep me

[custom]
other = 1
";

    #[test]
    fn replaces_only_the_value_and_keeps_every_other_byte() {
        let out = apply_edit(Some(COMMENTED), &[edit("statusline", "top")]).unwrap();
        assert_eq!(
            out,
            COMMENTED.replace("\"bottom\"   # keep me", "\"top\"   # keep me")
        );
    }

    #[test]
    fn inserts_a_missing_table_and_leaves_the_rest_untouched() {
        let existing = "# hi\n[statusline]\nposition = \"top\"\n";
        let out = apply_edit(Some(existing), &[edit("tabbar", "bottom")]).unwrap();
        // `toml_edit` separates a new table from the previous one with a blank line.
        assert_eq!(
            out,
            format!("{existing}\n[tabbar]\nposition = \"bottom\"\n")
        );
    }

    #[test]
    fn none_builds_a_document_with_only_the_listed_edits() {
        let out = apply_edit(None, &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, "[statusline]\nposition = \"top\"\n");
        assert_eq!(apply_edit(None, &[]).unwrap(), "");
    }

    #[test]
    fn inserts_a_missing_key_into_an_existing_table() {
        let out = apply_edit(Some("[tabbar]\nx = 1\n"), &[edit("tabbar", "top")]).unwrap();
        assert_eq!(out, "[tabbar]\nx = 1\nposition = \"top\"\n");
    }

    #[test]
    fn edits_an_inline_table_in_place() {
        let existing = "statusline = { position = \"bottom\" }\n";
        let out = apply_edit(Some(existing), &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, "statusline = { position = \"top\" }\n");
    }

    #[test]
    fn edits_a_dotted_key_in_place() {
        let existing = "statusline.position = \"bottom\"\n";
        let out = apply_edit(Some(existing), &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, "statusline.position = \"top\"\n");
    }

    #[test]
    fn replaces_an_invalid_value_with_the_valid_one() {
        let out = apply_edit(
            Some("[statusline]\nposition = \"middle\"\n"),
            &[edit("statusline", "bottom")],
        )
        .unwrap();
        assert_eq!(out, "[statusline]\nposition = \"bottom\"\n");
    }

    #[test]
    fn mouse_is_kept_when_present_and_never_added() {
        let with = "mouse = true\n[tabbar]\nposition = \"top\"\n";
        let out = apply_edit(Some(with), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(out, "mouse = true\n[tabbar]\nposition = \"bottom\"\n");
        let out = apply_edit(Some("[tabbar]\n"), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(out, "[tabbar]\nposition = \"bottom\"\n");
    }

    fn mouse(on: bool) -> ConfigEdit {
        ConfigEdit {
            table: "",
            key: "mouse",
            value: std::borrow::Cow::Borrowed(if on { "true" } else { "false" }),
        }
    }

    // Spec: the root key goes before the first table header.
    #[test]
    fn a_new_root_mouse_goes_before_the_first_table() {
        let existing = "# hi\n[statusline]\nposition = \"top\"\n";
        let out = apply_edit(Some(existing), &[mouse(false)]).unwrap();
        assert_eq!(
            out,
            "mouse = false\n# hi\n[statusline]\nposition = \"top\"\n"
        );
        assert!(!Config::parse(&out).unwrap().config.mouse);
    }

    #[test]
    fn a_new_root_mouse_is_appended_when_there_is_no_table() {
        let out = apply_edit(Some("theme = \"x\"\n"), &[mouse(false)]).unwrap();
        assert_eq!(out, "theme = \"x\"\nmouse = false\n");
    }

    #[test]
    fn an_empty_or_missing_file_gets_only_the_mouse_line() {
        assert_eq!(
            apply_edit(None, &[mouse(false)]).unwrap(),
            "mouse = false\n"
        );
        assert_eq!(
            apply_edit(Some(""), &[mouse(false)]).unwrap(),
            "mouse = false\n"
        );
    }

    #[test]
    fn a_comment_only_file_keeps_its_comments_above_the_mouse_line() {
        let out = apply_edit(Some("# my shell\n"), &[mouse(false)]).unwrap();
        assert_eq!(out, "# my shell\nmouse = false\n");
    }

    #[test]
    fn an_existing_root_mouse_is_updated_in_place_with_its_comment() {
        let out = apply_edit(Some(COMMENTED), &[mouse(true)]).unwrap();
        assert_eq!(
            out,
            COMMENTED.replace("mouse = false   #", "mouse = true   #")
        );
    }

    #[test]
    fn a_non_boolean_root_mouse_is_replaced_by_a_boolean() {
        let out = apply_edit(Some("mouse = \"yes\"\n[tabbar]\n"), &[mouse(false)]).unwrap();
        assert_eq!(out, "mouse = false\n[tabbar]\n");
    }

    #[test]
    fn mouse_true_is_written_explicitly_and_other_keys_stay() {
        let existing = "a = 1\n[tabbar]\nposition = \"top\"\n";
        let out = apply_edit(Some(existing), &[mouse(true)]).unwrap();
        assert_eq!(out, "a = 1\nmouse = true\n[tabbar]\nposition = \"top\"\n");
        assert!(Config::parse(&out).unwrap().config.mouse);
    }

    #[test]
    fn a_root_edit_and_a_table_edit_apply_together() {
        let out = apply_edit(
            Some("[tabbar]\nposition = \"top\"\n"),
            &[mouse(false), edit("tabbar", "bottom")],
        )
        .unwrap();
        assert_eq!(out, "mouse = false\n[tabbar]\nposition = \"bottom\"\n");
    }

    #[test]
    fn refuses_a_non_table_entry() {
        let one = [edit("statusline", "top")];
        assert_eq!(
            apply_edit(Some("statusline = 3\n"), &one),
            Err(EditError::NotATable("statusline"))
        );
        assert_eq!(
            apply_edit(Some("[[statusline]]\nposition = \"top\"\n"), &one),
            Err(EditError::NotATable("statusline"))
        );
    }

    #[test]
    fn refuses_a_syntax_error() {
        let one = [edit("statusline", "top")];
        assert_eq!(
            apply_edit(Some("[statusline\n"), &one),
            Err(EditError::Unparseable)
        );
    }

    #[test]
    fn empty_edits_leave_an_lf_file_byte_identical() {
        assert_eq!(apply_edit(Some(COMMENTED), &[]).unwrap(), COMMENTED);
    }

    #[test]
    fn the_last_of_two_edits_of_the_same_key_wins() {
        let out = apply_edit(
            Some("[tabbar]\nposition = \"top\"\n"),
            &[edit("tabbar", "bottom"), edit("tabbar", "top")],
        )
        .unwrap();
        assert_eq!(out, "[tabbar]\nposition = \"top\"\n");
    }

    #[test]
    fn crlf_is_kept_with_empty_edits() {
        let crlf = "# c\r\nx = 1\r\n[tabbar]\r\nposition = \"top\"\r\n";
        assert_eq!(apply_edit(Some(crlf), &[]).unwrap(), crlf);
    }

    #[test]
    fn crlf_is_kept_when_replacing_in_place() {
        let crlf = "# c\r\n[tabbar]\r\nposition = \"top\"  # k\r\nx = 1\r\n";
        let out = apply_edit(Some(crlf), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(
            out,
            "# c\r\n[tabbar]\r\nposition = \"bottom\"  # k\r\nx = 1\r\n"
        );
    }

    #[test]
    fn crlf_is_used_for_inserted_lines() {
        let crlf = "# c\r\n[statusline]\r\nposition = \"top\"\r\n";
        let out = apply_edit(Some(crlf), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(
            out,
            format!("{crlf}\r\n[tabbar]\r\nposition = \"bottom\"\r\n")
        );
    }

    #[test]
    fn a_mixed_file_follows_its_first_line_ending_outside_strings() {
        let out = apply_edit(Some("a = 1\r\nb = 2\n"), &[]).unwrap();
        assert_eq!(out, "a = 1\r\nb = 2\r\n");
        let out = apply_edit(Some("a = 1\nb = 2\r\n"), &[]).unwrap();
        assert_eq!(out, "a = 1\nb = 2\n");
    }

    #[test]
    fn a_crlf_inside_a_string_does_not_make_the_file_crlf() {
        let out = apply_edit(Some("s = '''a\r\nb'''\nx=1\n"), &[edit("tabbar", "top")]).unwrap();
        assert_eq!(
            out,
            "s = '''a\r\nb'''\nx=1\n\n[tabbar]\nposition = \"top\"\n"
        );
    }

    #[test]
    fn an_lf_inside_a_string_is_never_rewritten() {
        let out = apply_edit(Some("s = '''a\nb'''\r\nx=1\r\n"), &[edit("tabbar", "top")]).unwrap();
        assert_eq!(
            out,
            "s = '''a\nb'''\r\nx=1\r\n\r\n[tabbar]\r\nposition = \"top\"\r\n"
        );
    }

    fn appended(text: &str, newline: &str) -> String {
        format!("{text}{newline}[tabbar]{newline}position = \"bottom\"{newline}")
    }

    #[test]
    fn extra_quotes_before_a_closing_delimiter_stay_inside_the_string() {
        for text in [
            "m = \"\"\"a\"\"\"\"\r\nx = 1\r\n",
            "m = '''a'''''\r\nx = 1\r\n",
            "m = \"\"\"a\"\"\"\"\"\r\nx = 1\r\n",
        ] {
            let out = apply_edit(Some(text), &[edit("tabbar", "bottom")]).unwrap();
            assert_eq!(out, appended(text, "\r\n"), "{text:?}");
        }
    }

    #[test]
    fn an_escaped_backslash_before_the_closing_quote_ends_the_string() {
        let text = "s = \"a\\\\\"\r\nx = 1\r\n";
        let out = apply_edit(Some(text), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(out, appended(text, "\r\n"));
    }

    #[test]
    fn a_line_continuation_inside_a_string_keeps_its_crlf() {
        let text = "s = \"\"\"a \\\r\nb\"\"\"\r\nx = 1\r\n";
        let out = apply_edit(Some(text), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(out, appended(text, "\r\n"));
    }

    #[test]
    fn a_hash_in_a_quoted_key_does_not_start_a_comment() {
        let text = "\"a#b\" = \"\"\"x\nq\"\"\"\r\nz = 1\r\n";
        assert_eq!(apply_edit(Some(text), &[]).unwrap(), text);
    }

    #[test]
    fn a_missing_trailing_newline_is_added_in_the_files_style() {
        let out = apply_edit(Some("x = 1"), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(out, "x = 1\n\n[tabbar]\nposition = \"bottom\"\n");
        let out = apply_edit(Some("x = 1\r\ny = 2"), &[edit("tabbar", "bottom")]).unwrap();
        assert_eq!(
            out,
            "x = 1\r\ny = 2\r\n\r\n[tabbar]\r\nposition = \"bottom\"\r\n"
        );
    }

    #[test]
    fn quotes_in_comments_and_strings_do_not_confuse_the_line_ending_scan() {
        let text = "# it's\r\nq = \"say \\\"hi\\\"\"\r\nm = \"\"\"a\nb\"\"\"\"\r\nl = 'x'\r\n";
        assert_eq!(apply_edit(Some(text), &[]).unwrap(), text);
    }

    #[test]
    fn refuses_an_owned_key_that_holds_a_table() {
        let refused = Err(EditError::KeyIsTable {
            table: "statusline",
            key: "position",
        });
        let one = [edit("statusline", "top")];
        assert_eq!(
            apply_edit(Some("[statusline.position]\nfoo = 1\n"), &one),
            refused
        );
        assert_eq!(
            apply_edit(Some("[statusline]\nposition = { foo = 1 }\n"), &one),
            refused
        );
    }

    #[test]
    fn refuses_an_owned_key_that_holds_an_array_of_tables() {
        let out = apply_edit(
            Some("[[statusline.position]]\nfoo = 1\n"),
            &[edit("statusline", "top")],
        );
        assert_eq!(
            out,
            Err(EditError::KeyIsTable {
                table: "statusline",
                key: "position"
            })
        );
    }

    #[test]
    fn replaces_an_inline_array_of_tables() {
        let out = apply_edit(
            Some("[tabbar]\nposition = [{ a = 1 }]\n"),
            &[edit("tabbar", "top")],
        )
        .unwrap();
        assert_eq!(out, "[tabbar]\nposition = \"top\"\n");
    }

    #[test]
    fn comments_stay_before_a_table_inserted_into_a_comment_only_file() {
        let out = apply_edit(Some("# header\n# more\n"), &[edit("tabbar", "top")]).unwrap();
        assert_eq!(out, "# header\n# more\n\n[tabbar]\nposition = \"top\"\n");
        let out = apply_edit(Some("# no newline"), &[edit("tabbar", "top")]).unwrap();
        assert_eq!(out, "# no newline\n\n[tabbar]\nposition = \"top\"\n");
    }

    const BROKEN: [&str; 5] = [
        "[statusline\nposition = \"top\"\n",
        "position = \n",
        "a = 1\na = 2\n",
        "[statusline]\nposition = \"top\n",
        "= 1\n",
    ];

    #[test]
    fn parity_with_config_parse_on_broken_input() {
        for text in BROKEN {
            assert!(Config::parse(text).is_err(), "parse accepted {text:?}");
            assert_eq!(
                apply_edit(Some(text), &[edit("statusline", "top")]),
                Err(EditError::Unparseable),
                "apply_edit accepted {text:?}"
            );
        }
    }

    #[test]
    fn saved_text_parses_back_to_the_draft_positions() {
        let inputs = [
            "",
            COMMENTED,
            "mouse = true\n",
            "statusline = { position = \"middle\" }\n",
            "statusline.position = \"sideways\"\n[tabbar]\nposition = 3\n",
            "[tabbar]\nposition = \"top\"\n",
            "# only a comment",
        ];
        let positions = [BarPosition::Top, BarPosition::Bottom];
        let name = |p| {
            if p == BarPosition::Top {
                "top"
            } else {
                "bottom"
            }
        };
        for text in inputs {
            for statusline in positions {
                for tabbar in positions {
                    let edits = [
                        edit("statusline", name(statusline)),
                        edit("tabbar", name(tabbar)),
                    ];
                    let out = apply_edit(Some(text), &edits).unwrap();
                    let parsed = Config::parse(&out).unwrap();
                    assert_eq!(parsed.config.bars.statusline, statusline, "{text:?}");
                    assert_eq!(parsed.config.bars.tabbar, tabbar, "{text:?}");
                    assert_eq!(parsed.problems, Vec::<String>::new(), "{text:?}");
                }
            }
        }
    }

    // Spec (config-persistence): the AI keys. Characterization: the generic
    // table and key code already covers them, so these pass at once.
    fn ai_edits(engine: &'static str, model: &'static str) -> [ConfigEdit; 2] {
        let owned = |key, value: &'static str| ConfigEdit {
            table: "ai",
            key,
            value: std::borrow::Cow::Borrowed(value),
        };
        [owned("engine", engine), owned("model", model)]
    }

    fn parsed_ai(text: &str) -> (crate::core::ai::config::AiConfig, Option<String>) {
        let (_, ai, notice) = Config::load_with_ai(Ok(text.to_owned()), None);
        (ai, notice)
    }

    #[test]
    fn a_missing_ai_table_is_appended_after_the_existing_bytes() {
        let existing = "# mine\n[statusline]\nposition = \"bottom\"\n";
        let out = apply_edit(Some(existing), &ai_edits("ollama", "llama3:8b")).unwrap();
        assert_eq!(
            out,
            format!("{existing}\n[ai]\nengine = \"ollama\"\nmodel = \"llama3:8b\"\n")
        );
    }

    #[test]
    fn existing_ai_keys_are_updated_in_place() {
        let existing = "# mine\n[ai]\n# pick\nmodel = \"x\"  # m\nengine = \"claude\"\n";
        let out = apply_edit(Some(existing), &ai_edits("ollama", "llama3:8b")).unwrap();
        assert_eq!(
            out,
            "# mine\n[ai]\n# pick\nmodel = \"llama3:8b\"  # m\nengine = \"ollama\"\n"
        );
    }

    #[test]
    fn no_engine_is_written_as_empty_strings_and_reads_back_unset() {
        let out = apply_edit(None, &ai_edits("", "")).unwrap();
        assert_eq!(out, "[ai]\nengine = \"\"\nmodel = \"\"\n");
        assert_eq!(
            parsed_ai(&out),
            (crate::core::ai::config::AiConfig::default(), None)
        );
    }

    #[test]
    fn special_characters_and_the_draft_round_trip_without_a_notice() {
        let out = apply_edit(None, &ai_edits("ollama", "a\"b\\c")).unwrap();
        let (ai, notice) = parsed_ai(&out);
        assert_eq!(
            ai,
            crate::core::ai::config::AiConfig::general(Some("ollama"), Some("a\"b\\c"))
        );
        assert_eq!(notice, None);
    }

    #[test]
    fn a_position_only_save_leaves_the_ai_table_alone() {
        let saved = "[ai]\nengine = \"codex\"\nmodel = \"default\"\n";
        let out = apply_edit(Some(saved), &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, format!("{saved}\n[statusline]\nposition = \"top\"\n"));
        let out = apply_edit(None, &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, "[statusline]\nposition = \"top\"\n");
    }

    #[test]
    fn collisions_with_the_ai_keys_are_refused() {
        assert_eq!(
            apply_edit(Some("ai = 3\n"), &ai_edits("ollama", "m")),
            Err(EditError::NotATable("ai"))
        );
        assert_eq!(
            apply_edit(Some("[ai.engine]\nx = 1\n"), &ai_edits("ollama", "m")),
            Err(EditError::KeyIsTable {
                table: "ai",
                key: "engine"
            })
        );
        let model_table = "[ai]\nengine = \"x\"\n[ai.model]\nx = 1\n";
        assert_eq!(
            apply_edit(Some(model_table), &ai_edits("a", "m")),
            Err(EditError::KeyIsTable {
                table: "ai",
                key: "model"
            })
        );
    }

    #[test]
    fn a_scalar_of_the_wrong_type_is_replaced_and_an_ignored_collision_is_kept() {
        let out = apply_edit(Some("[ai]\nengine = 3\n"), &ai_edits("ollama", "m")).unwrap();
        assert_eq!(out, "[ai]\nengine = \"ollama\"\nmodel = \"m\"\n");
        let existing = "ai = 3\n[statusline]\nposition = \"bottom\"\n";
        let out = apply_edit(Some(existing), &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, "ai = 3\n[statusline]\nposition = \"top\"\n");
    }

    // Spec (config-persistence): nested table edits. `table` is a dotted
    // static path; missing parents become implicit tables.
    fn code_edits(engine: &'static str, model: &'static str) -> [ConfigEdit; 2] {
        let owned = |key, value: &'static str| ConfigEdit {
            table: "ai.tasks.code",
            key,
            value: std::borrow::Cow::Borrowed(value),
        };
        [owned("engine", engine), owned("model", model)]
    }

    #[test]
    fn a_nested_table_is_created_without_empty_parent_headers() {
        let out = apply_edit(None, &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            "[ai.tasks.code]\nengine = \"claude\"\nmodel = \"opus\"\n"
        );
    }

    #[test]
    fn a_nested_table_is_appended_after_an_existing_ai_table() {
        let existing = "# mine\n[ai]\nengine = \"x\"  # keep\nmodel = \"y\"\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            format!("{existing}\n[ai.tasks.code]\nengine = \"claude\"\nmodel = \"opus\"\n")
        );
    }

    #[test]
    fn a_nested_table_keeps_crlf() {
        let existing = "[ai]\r\nengine = \"x\"\r\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            format!("{existing}\r\n[ai.tasks.code]\r\nengine = \"claude\"\r\nmodel = \"opus\"\r\n")
        );
    }

    #[test]
    fn comments_stay_before_a_nested_table_in_a_comment_only_file() {
        let out = apply_edit(Some("# header\n"), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            "# header\n\n[ai.tasks.code]\nengine = \"claude\"\nmodel = \"opus\"\n"
        );
    }

    #[test]
    fn an_existing_nested_table_is_edited_in_place() {
        let existing = "[ai.tasks.code]\n# pick\nmodel = \"old:7b\"  # m\nengine = \"x\"\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            "[ai.tasks.code]\n# pick\nmodel = \"opus\"  # m\nengine = \"claude\"\n"
        );
    }

    #[test]
    fn a_missing_key_is_added_to_an_existing_nested_table() {
        let existing = "[ai]\nengine = \"x\"\n\n[ai.tasks.code]\nmodel = \"old\"\n\n[tabbar]\nposition = \"top\"\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            "[ai]\nengine = \"x\"\n\n[ai.tasks.code]\nmodel = \"opus\"\nengine = \"claude\"\n\n[tabbar]\nposition = \"top\"\n"
        );
    }

    #[test]
    fn a_task_under_an_explicit_tasks_table_leaves_its_siblings_alone() {
        let existing = "[ai.tasks]\nchat = 3\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            format!("{existing}\n[ai.tasks.code]\nengine = \"claude\"\nmodel = \"opus\"\n")
        );
    }

    #[test]
    fn inline_parents_are_edited_in_place() {
        let existing = "[ai]\ntasks = { code = { engine = \"a\", model = \"b\" } }\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            "[ai]\ntasks = { code = { engine = \"claude\", model = \"opus\" } }\n"
        );
        let out = apply_edit(Some("[ai]\ntasks = {}\n"), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            "[ai]\ntasks = { code = { engine = \"claude\", model = \"opus\" } }\n"
        );
    }

    #[test]
    fn dotted_parents_are_edited_in_place() {
        let existing = "ai.tasks.code.engine = \"a\"\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            "ai.tasks.code.engine = \"claude\"\nai.tasks.code.model = \"opus\"\n"
        );
    }

    #[test]
    fn nested_collisions_name_the_full_path() {
        let refused = |text, err| {
            assert_eq!(
                apply_edit(Some(text), &code_edits("claude", "opus")),
                Err(err),
                "{text:?}"
            );
        };
        refused("ai = 3\n", EditError::NotATable("ai"));
        refused("[ai]\ntasks = 3\n", EditError::NotATable("ai.tasks"));
        refused("[[ai.tasks]]\nx = 1\n", EditError::NotATable("ai.tasks"));
        refused(
            "[ai.tasks]\ncode = \"x\"\n",
            EditError::NotATable("ai.tasks.code"),
        );
        refused(
            "[ai.tasks.code.engine]\nx = 1\n",
            EditError::KeyIsTable {
                table: "ai.tasks.code",
                key: "engine",
            },
        );
    }

    #[test]
    fn the_general_table_stays_byte_identical_when_only_a_task_changes() {
        let existing = "[ai]\nengine = \"x\"\nmodel = \"y\"\n";
        let out = apply_edit(Some(existing), &code_edits("claude", "opus")).unwrap();
        assert!(out.starts_with(existing), "{out:?}");
    }

    // Round trip: what the reader gets back is what was written.
    #[test]
    fn written_task_tables_read_back_to_the_same_assignments() {
        use crate::core::ai::config::{AiConfig, Assignment, Task};
        let values = [
            ("claude", "opus"),
            ("ollama", "old:7b"),
            ("", ""),
            ("codex", "default"),
            ("lmstudio", "m"),
        ];
        let some = |v: &str| (!v.is_empty()).then(|| v.to_owned());
        let mut edits = Vec::new();
        let mut want = AiConfig::default();
        for (task, (engine, model)) in Task::ALL.into_iter().zip(values) {
            for (key, value) in [("engine", engine), ("model", model)] {
                edits.push(ConfigEdit {
                    table: task.table(),
                    key,
                    value: std::borrow::Cow::Borrowed(value),
                });
            }
            want.set(
                task,
                Assignment {
                    engine: some(engine),
                    model: some(model),
                },
            );
        }
        for text in [None, Some("# c\n[statusline]\nposition = \"top\"\n")] {
            let out = apply_edit(text, &edits).unwrap();
            let (_, ai, notice) = Config::load_with_ai(Ok(out.clone()), None);
            assert_eq!(ai, want, "{out:?}");
            assert_eq!(notice, None, "{out:?}");
        }
    }

    // Spec (config-persistence): memory keys are owned booleans.
    fn flag(key: &'static str, on: bool) -> ConfigEdit {
        ConfigEdit {
            table: "ai.memory",
            key,
            value: std::borrow::Cow::Borrowed(if on { "true" } else { "false" }),
        }
    }

    fn memory_of(text: &str) -> (crate::core::ai::config::MemoryConfig, Option<String>) {
        let (_, ai, notice) = Config::load_with_ai(Ok(text.to_owned()), None);
        (ai.memory, notice)
    }

    #[test]
    fn a_missing_memory_table_is_appended_with_booleans_only() {
        let existing = "# mine\n[statusline]\nposition = \"bottom\"\n";
        let edits = [flag("enabled", true), flag("acknowledged", true)];
        let out = apply_edit(Some(existing), &edits).unwrap();
        assert_eq!(
            out,
            format!("{existing}\n[ai.memory]\nenabled = true\nacknowledged = true\n")
        );
        let (memory, notice) = memory_of(&out);
        assert!(memory.active(), "{out:?}");
        assert_eq!(notice, None);
    }

    #[test]
    fn only_the_enabled_value_changes_in_place() {
        let existing =
            "[ai.memory]\nenabled = false  # off\n# where\nport = 8123\nacknowledged = true\n";
        let out = apply_edit(Some(existing), &[flag("enabled", true)]).unwrap();
        assert_eq!(out, existing.replace("false  # off", "true  # off"));
    }

    #[test]
    fn turning_memory_off_keeps_the_acknowledgement() {
        let existing = "[ai.memory]\nenabled = true\nacknowledged = true\n";
        let out = apply_edit(Some(existing), &[flag("enabled", false)]).unwrap();
        assert_eq!(out, "[ai.memory]\nenabled = false\nacknowledged = true\n");
        let (memory, _) = memory_of(&out);
        assert!(memory.acknowledged && !memory.enabled);
    }

    #[test]
    fn the_port_is_preserved_byte_for_byte_and_never_added() {
        let with_port = "[ai.memory]\nport   =   8123 # mine\nendpoint = \"x\"\n";
        let out = apply_edit(Some(with_port), &[flag("enabled", true)]).unwrap();
        assert_eq!(
            out,
            "[ai.memory]\nport   =   8123 # mine\nendpoint = \"x\"\nenabled = true\n"
        );
        let out = apply_edit(Some("x = 1\n"), &[flag("enabled", true)]).unwrap();
        assert!(!out.contains("port"), "{out:?}");
    }

    #[test]
    fn no_memory_edits_leave_the_file_untouched() {
        let existing = "[statusline]\nposition = \"top\"\n";
        assert_eq!(apply_edit(Some(existing), &[]).unwrap(), existing);
        let with = "[ai.memory]\nenabled = true\n";
        assert_eq!(apply_edit(Some(with), &[]).unwrap(), with);
    }

    #[test]
    fn a_scalar_memory_or_a_table_valued_flag_is_refused() {
        let one = [flag("enabled", true)];
        assert_eq!(
            apply_edit(Some("[ai]\nmemory = 3\n"), &one),
            Err(EditError::NotATable("ai.memory"))
        );
        assert_eq!(
            apply_edit(Some("[ai.memory.enabled]\nx = 1\n"), &one),
            Err(EditError::KeyIsTable {
                table: "ai.memory",
                key: "enabled"
            })
        );
        assert_eq!(
            apply_edit(
                Some("[ai.memory.acknowledged]\nx = 1\n"),
                &[flag("acknowledged", true)]
            ),
            Err(EditError::KeyIsTable {
                table: "ai.memory",
                key: "acknowledged"
            })
        );
    }

    #[test]
    fn a_wrong_type_flag_is_replaced_and_an_ignored_collision_is_kept() {
        let out = apply_edit(Some("[ai.memory]\nenabled = 3\n"), &[flag("enabled", true)]).unwrap();
        assert_eq!(out, "[ai.memory]\nenabled = true\n");
        let existing = "[ai.memory.enabled]\nx = 1\n[statusline]\nposition = \"bottom\"\n";
        let out = apply_edit(Some(existing), &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, existing.replace("bottom", "top"));
    }

    #[test]
    fn a_memory_flag_keeps_crlf() {
        let existing = "[ai.memory]\r\nenabled = false\r\n";
        let out = apply_edit(Some(existing), &[flag("acknowledged", true)]).unwrap();
        assert_eq!(
            out,
            "[ai.memory]\r\nenabled = false\r\nacknowledged = true\r\n"
        );
    }

    // Review: lossless save for every way TOML can spell `ai.memory`. Each
    // keeps its comments, its port and its unknown keys; both booleans land
    // as real booleans and the file reads back as active.
    #[test]
    fn memory_flags_are_saved_losslessly_in_every_toml_spelling() {
        let cases: [(&str, &[&str]); 6] = [
            (
                "ai.memory = {port=8123, x=1} # c\n",
                &["# c", "port=8123", "x=1"],
            ),
            (
                "# top\n[ai]\nmemory = {port=8123, x=1} # c\n",
                &["# top", "# c", "port=8123", "x=1"],
            ),
            (
                "[ai]\n# mine\nmemory.port = 8123 # p\nmemory.x = 1\n",
                &["# mine", "# p", "port = 8123", "memory.x = 1"],
            ),
            (
                "# root\nai.memory.port = 8123 # p\nai.memory.x = 1\n",
                &["# root", "# p", "port = 8123", "x = 1"],
            ),
            (
                "ai = { memory = {port=8123, x=1} } # c\n",
                &["# c", "port=8123", "x=1"],
            ),
            (
                "[ai.memory]\nport = 8123 # p\nx = 1\n# tail\n[other]\nkeep = true\n",
                &[
                    "# p",
                    "port = 8123",
                    "x = 1",
                    "# tail",
                    "[other]",
                    "keep = true",
                ],
            ),
        ];
        let edits = [flag("enabled", true), flag("acknowledged", true)];
        for (existing, kept) in cases {
            let out = apply_edit(Some(existing), &edits).unwrap();
            for needle in kept {
                assert!(out.contains(needle), "{needle:?} lost in {out:?}");
            }
            let (_, ai, notice) = Config::load_with_ai(Ok(out.clone()), None);
            assert!(ai.memory.active(), "{out:?}");
            assert_eq!(ai.memory.port, 8123, "{out:?}");
            assert_eq!(notice, None, "{out:?}");
            assert!(!out.contains("\"true\""), "booleans, not strings: {out:?}");
        }
    }

    // Only the memory flags are booleans: other string edits stay strings.
    #[test]
    fn other_keys_named_enabled_stay_strings() {
        let other = ConfigEdit {
            table: "custom",
            key: "enabled",
            value: std::borrow::Cow::Borrowed("true"),
        };
        assert_eq!(
            apply_edit(None, &[other]).unwrap(),
            "[custom]\nenabled = \"true\"\n"
        );
    }

    // Spec (config-persistence): the profile is an owned key. The generic
    // table and key code covers it; these pin the scenarios.
    fn profile_edit(value: &'static str) -> [ConfigEdit; 1] {
        [ConfigEdit {
            table: "ai",
            key: "profile",
            value: std::borrow::Cow::Borrowed(value),
        }]
    }

    #[test]
    fn a_profile_is_created_at_the_end_of_a_new_ai_table() {
        let existing = "# mine\n[statusline]\nposition = \"bottom\"\n";
        let out = apply_edit(Some(existing), &profile_edit("analyst")).unwrap();
        assert_eq!(out, format!("{existing}\n[ai]\nprofile = \"analyst\"\n"));
    }

    #[test]
    fn a_profile_is_updated_in_place() {
        let existing = "[ai]\nengine = \"ollama\"\n# who\nprofile = \"analyst\"  # p\n";
        let out = apply_edit(Some(existing), &profile_edit("programmer")).unwrap();
        assert_eq!(
            out,
            "[ai]\nengine = \"ollama\"\n# who\nprofile = \"programmer\"  # p\n"
        );
    }

    #[test]
    fn the_default_profile_is_written_explicitly_and_never_removed() {
        let existing = "[ai]\nprofile = \"analyst\"\n";
        let out = apply_edit(Some(existing), &profile_edit("programmer")).unwrap();
        assert_eq!(out, "[ai]\nprofile = \"programmer\"\n");
        let out = apply_edit(None, &profile_edit("programmer")).unwrap();
        assert_eq!(out, "[ai]\nprofile = \"programmer\"\n");
    }

    #[test]
    fn a_profile_only_save_writes_no_engine_keys() {
        for text in [None, Some("[ai]\nengine = \"x\"\n"), Some("# c\n")] {
            let out = apply_edit(text, &profile_edit("analyst")).unwrap();
            let (_, ai, notice) = Config::load_with_ai(Ok(out.clone()), None);
            assert_eq!(notice, None, "{out:?}");
            assert_eq!(ai.profile(), crate::core::ai::config::Profile::Analyst);
            let had_engine = text.is_some_and(|t| t.contains("engine"));
            assert_eq!(out.matches("engine").count(), usize::from(had_engine));
            assert!(!out.contains("model"), "{out:?}");
        }
    }

    #[test]
    fn a_position_only_save_leaves_the_profile_line_alone() {
        let saved = "[ai]\nprofile = \"programmer\"\n";
        let out = apply_edit(Some(saved), &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, format!("{saved}\n[statusline]\nposition = \"top\"\n"));
        let out = apply_edit(Some("# c\n"), &[edit("statusline", "top")]).unwrap();
        assert!(!out.contains("profile"), "{out:?}");
    }

    // Spec (config-persistence): Profile key collisions.
    #[test]
    fn a_profile_table_is_refused_and_a_scalar_is_replaced() {
        let refused = Err(EditError::KeyIsTable {
            table: "ai",
            key: "profile",
        });
        for text in [
            "[ai.profile]\nx = 1\n",
            "[ai]\nprofile = { x = 1 }\n",
            "[[ai.profile]]\nx = 1\n",
        ] {
            assert_eq!(
                apply_edit(Some(text), &profile_edit("analyst")),
                refused,
                "{text:?}"
            );
        }
        let out = apply_edit(Some("[ai]\nprofile = 3\n"), &profile_edit("analyst")).unwrap();
        assert_eq!(out, "[ai]\nprofile = \"analyst\"\n");
        assert_eq!(
            apply_edit(Some("ai = 3\n"), &profile_edit("analyst")),
            Err(EditError::NotATable("ai"))
        );
    }

    #[test]
    fn a_profile_collision_is_ignored_when_the_profile_is_not_edited() {
        let existing = "[ai.profile]\nx = 1\n[statusline]\nposition = \"bottom\"\n";
        let out = apply_edit(Some(existing), &[edit("statusline", "top")]).unwrap();
        assert_eq!(out, existing.replace("\"bottom\"", "\"top\""));
        let out = apply_edit(Some("ai = 3\n"), &[edit("statusline", "top")]).unwrap();
        assert!(out.starts_with("ai = 3\n"), "{out:?}");
    }

    // Spec (prompt-profiles, Context toggle): `ai.context` is an owned
    // boolean key: the value text `true`/`false` is written as a TOML boolean.
    fn context_edit(value: &'static str) -> [ConfigEdit; 1] {
        [ConfigEdit {
            table: "ai",
            key: "context",
            value: std::borrow::Cow::Borrowed(value),
        }]
    }

    #[test]
    fn the_context_is_written_as_a_boolean() {
        let out = apply_edit(None, &context_edit("false")).unwrap();
        assert_eq!(out, "[ai]\ncontext = false\n");
        let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
        assert!(!ai.context());
        assert_eq!(notice, None);
        let out = apply_edit(Some("[ai]\nprofile = \"analyst\"\n"), &context_edit("true")).unwrap();
        assert_eq!(out, "[ai]\nprofile = \"analyst\"\ncontext = true\n");
        let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
        assert!(ai.context());
        assert_eq!(notice, None);
    }

    #[test]
    fn the_context_is_updated_in_place_and_a_scalar_is_replaced() {
        let existing = "[ai]\n# c\ncontext = true  # on\nmodel = \"m\"\n";
        let out = apply_edit(Some(existing), &context_edit("false")).unwrap();
        assert_eq!(out, "[ai]\n# c\ncontext = false  # on\nmodel = \"m\"\n");
        let out = apply_edit(Some("[ai]\ncontext = \"x\"\n"), &context_edit("true")).unwrap();
        assert_eq!(out, "[ai]\ncontext = true\n");
    }

    #[test]
    fn a_context_table_is_refused_and_an_unchanged_one_is_ignored() {
        for text in [
            "[ai.context]\nx = 1\n",
            "[ai]\ncontext = { x = 1 }\n",
            "[[ai.context]]\nx = 1\n",
        ] {
            assert_eq!(
                apply_edit(Some(text), &context_edit("false")),
                Err(EditError::KeyIsTable {
                    table: "ai",
                    key: "context"
                }),
                "{text:?}"
            );
        }
        let position = [edit("statusline", "top")];
        let existing = "[ai.context]\nx = 1\n";
        let out = apply_edit(Some(existing), &position).unwrap();
        assert!(out.starts_with(existing), "{out:?}");
    }

    // Spec (chat-notify, config-persistence): `ai.notify` is an owned string.
    fn notify_edit(value: &'static str) -> [ConfigEdit; 1] {
        [ConfigEdit {
            table: "ai",
            key: "notify",
            value: std::borrow::Cow::Borrowed(value),
        }]
    }

    #[test]
    fn the_notify_mode_is_written_as_a_string() {
        let out = apply_edit(None, &notify_edit("desktop")).unwrap();
        assert_eq!(out, "[ai]\nnotify = \"desktop\"\n");
        let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
        assert_eq!(ai.notify(), crate::core::ai::config::NotifyMode::Desktop);
        assert_eq!(notice, None);
        let out = apply_edit(Some("[ai]\ncontext = false\n"), &notify_edit("off")).unwrap();
        assert_eq!(out, "[ai]\ncontext = false\nnotify = \"off\"\n");
    }

    #[test]
    fn notify_sound_round_trips_as_a_toml_boolean() {
        for (value, want) in [("false", false), ("true", true)] {
            let edit = [ConfigEdit {
                table: "ai",
                key: "notify_sound",
                value: std::borrow::Cow::Borrowed(value),
            }];
            let out = apply_edit(Some("[ai]\ncontext = false\n"), &edit).unwrap();
            assert_eq!(
                out,
                format!("[ai]\ncontext = false\nnotify_sound = {value}\n")
            );
            let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
            assert_eq!(ai.notify_sound(), want);
            assert_eq!(notice, None);
        }
        let out = apply_edit(
            Some("[ai]\nnotify_sound = \"yes\" # old\n"),
            &[ConfigEdit {
                table: "ai",
                key: "notify_sound",
                value: std::borrow::Cow::Borrowed("false"),
            }],
        )
        .unwrap();
        assert_eq!(out, "[ai]\nnotify_sound = false # old\n");
    }

    #[test]
    fn a_legacy_boolean_is_replaced_in_place_by_the_mode() {
        let existing = "[ai]\n# c\nnotify = true  # on\nmodel = \"m\"\n";
        let out = apply_edit(Some(existing), &notify_edit("bell")).unwrap();
        assert_eq!(out, "[ai]\n# c\nnotify = \"bell\"  # on\nmodel = \"m\"\n");
        let out = apply_edit(Some("[ai]\nnotify = \"x\"\n"), &notify_edit("auto")).unwrap();
        assert_eq!(out, "[ai]\nnotify = \"auto\"\n");
    }

    #[test]
    fn a_notify_table_is_refused_and_an_unchanged_one_is_ignored() {
        for text in [
            "[ai.notify]\nx = 1\n",
            "[ai]\nnotify = { x = 1 }\n",
            "[[ai.notify]]\nx = 1\n",
        ] {
            assert_eq!(
                apply_edit(Some(text), &notify_edit("auto")),
                Err(EditError::KeyIsTable {
                    table: "ai",
                    key: "notify"
                }),
                "{text:?}"
            );
        }
        let position = [edit("statusline", "top")];
        let existing = "[ai.notify]\nx = 1\n";
        let out = apply_edit(Some(existing), &position).unwrap();
        assert!(out.starts_with(existing), "{out:?}");
    }

    // Spec (agent-tools, config-persistence): `ai.tools.enabled` is an owned
    // boolean and the only key the menu writes about the tools.
    fn tools_edit(on: bool) -> [ConfigEdit; 1] {
        [ConfigEdit::tools_flag(on)]
    }

    #[test]
    fn the_tools_flag_is_written_as_a_boolean_in_its_own_table() {
        let out = apply_edit(None, &tools_edit(false)).unwrap();
        assert_eq!(out, "[ai.tools]\nenabled = false\n");
        let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
        assert!(!ai.tools_enabled());
        assert_eq!(notice, None);
    }

    #[test]
    fn the_tools_flag_is_updated_in_place_and_a_scalar_is_replaced() {
        let existing = "[ai.tools]\n# c\nenabled = false  # off\n";
        let out = apply_edit(Some(existing), &tools_edit(true)).unwrap();
        assert_eq!(out, "[ai.tools]\n# c\nenabled = true  # off\n");
        let out = apply_edit(Some("[ai.tools]\nenabled = \"x\"\n"), &tools_edit(true)).unwrap();
        assert_eq!(out, "[ai.tools]\nenabled = true\n");
    }

    #[test]
    fn saving_the_tools_flag_leaves_the_mcp_servers_byte_for_byte() {
        let mcp = "[[ai.mcp.servers]]\nname = \"docs\"\ncommand = \"/usr/bin/docs-mcp\"\nargs = [\"--x\"]\n";
        let existing = format!("[ai]\nnotify = true\n\n{mcp}");
        let out = apply_edit(Some(&existing), &tools_edit(false)).unwrap();
        assert!(out.contains(mcp), "{out:?}");
        assert!(out.contains("enabled = false"), "{out:?}");
        let again = apply_edit(Some(&out), &tools_edit(false)).unwrap();
        assert_eq!(again, out, "saving the same value is a no-op");
    }

    #[test]
    fn a_tools_flag_that_holds_a_table_is_refused() {
        let existing = "[ai.tools.enabled]\nx = 1\n";
        assert_eq!(
            apply_edit(Some(existing), &tools_edit(true)),
            Err(EditError::KeyIsTable {
                table: "ai.tools",
                key: "enabled"
            })
        );
    }

    // Spec (agent-tools, config-persistence): `ai.tools.approval` is an owned
    // string, written next to `enabled` and left untouched when unchanged.
    fn approval_edit(mode: crate::core::ai::config::ApprovalMode) -> [ConfigEdit; 1] {
        [ConfigEdit::approval(mode)]
    }

    #[test]
    fn the_run_mode_is_written_as_a_string_and_reads_back() {
        use crate::core::ai::config::ApprovalMode;
        let out = apply_edit(None, &approval_edit(ApprovalMode::Run)).unwrap();
        assert_eq!(out, "[ai.tools]\napproval = \"run\"\n");
        let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
        assert_eq!(ai.approval(), ApprovalMode::Run);
        assert_eq!(notice, None);
    }

    #[test]
    fn the_approval_key_is_created_in_its_own_table_as_a_string() {
        use crate::core::ai::config::ApprovalMode;
        let out = apply_edit(None, &approval_edit(ApprovalMode::Auto)).unwrap();
        assert_eq!(out, "[ai.tools]\napproval = \"auto\"\n");
        let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
        assert_eq!(ai.approval(), ApprovalMode::Auto);
        assert_eq!(notice, None);
    }

    #[test]
    fn the_approval_key_is_created_next_to_enabled() {
        use crate::core::ai::config::ApprovalMode;
        let existing = "[ai.tools]\nenabled = true  # on\n\n[ai]\nnotify = \"bell\"\n";
        let out = apply_edit(Some(existing), &approval_edit(ApprovalMode::Auto)).unwrap();
        assert_eq!(
            out,
            "[ai.tools]\nenabled = true  # on\napproval = \"auto\"\n\n[ai]\nnotify = \"bell\"\n"
        );
        let (_, ai, notice) = crate::core::config::Config::load_with_ai(Ok(out), None);
        assert_eq!(ai.approval(), ApprovalMode::Auto);
        assert!(ai.tools_enabled());
        assert_eq!(notice, None);
    }

    #[test]
    fn an_existing_approval_is_updated_in_place_and_keeps_its_comment() {
        use crate::core::ai::config::ApprovalMode;
        let existing = "[ai.tools]\n# how\napproval = \"auto\"  # careful\nenabled = true\n";
        let out = apply_edit(Some(existing), &approval_edit(ApprovalMode::Ask)).unwrap();
        assert_eq!(
            out,
            "[ai.tools]\n# how\napproval = \"ask\"  # careful\nenabled = true\n"
        );
    }

    #[test]
    fn the_default_is_written_explicitly_and_saving_it_twice_is_a_no_op() {
        use crate::core::ai::config::ApprovalMode;
        let out = apply_edit(None, &approval_edit(ApprovalMode::Ask)).unwrap();
        assert_eq!(out, "[ai.tools]\napproval = \"ask\"\n");
        let again = apply_edit(Some(&out), &approval_edit(ApprovalMode::Ask)).unwrap();
        assert_eq!(again, out);
    }

    #[test]
    fn saving_the_approval_leaves_the_mcp_servers_and_enabled_byte_for_byte() {
        use crate::core::ai::config::ApprovalMode;
        let mcp = "[[ai.mcp.servers]]\nname = \"docs\"\ncommand = \"/usr/bin/docs-mcp\"\n";
        let existing = format!("[ai.tools]\nenabled = false\n\n{mcp}");
        let out = apply_edit(Some(&existing), &approval_edit(ApprovalMode::Auto)).unwrap();
        assert!(out.contains(mcp), "{out:?}");
        assert!(out.contains("enabled = false"), "{out:?}");
    }

    #[test]
    fn an_approval_key_that_holds_a_table_is_refused() {
        use crate::core::ai::config::ApprovalMode;
        let existing = "[ai.tools.approval]\nx = 1\n";
        assert_eq!(
            apply_edit(Some(existing), &approval_edit(ApprovalMode::Auto)),
            Err(EditError::KeyIsTable {
                table: "ai.tools",
                key: "approval"
            })
        );
    }

    // Spec (config-persistence): MCP server lists are never written.
    #[test]
    fn every_save_keeps_the_mcp_servers_byte_for_byte_whatever_they_hold() {
        let mcp = "# my servers\n[[ai.mcp.servers]]\nname = \"docs\" # first\ncommand = \"/usr/bin/docs-mcp\"\nenv = [\"TOKEN\"]\nproject = \"/home/u/p\"\nfuture = 1\n\n\
            # broken one\n[[ai.mcp.servers]]\nname = 5\ncommand = { x = 1 }\n\n\
            [[ai.mcp.servers]]\nname = \"b\"\ncommand = \"b\"\n";
        let existing = format!("[statusline]\nposition = \"top\"\n\n{mcp}");
        let edits = [
            ConfigEdit::tools_flag(false),
            ConfigEdit::approval(crate::core::ai::config::ApprovalMode::Auto),
            edit("statusline", "bottom"),
        ];
        let out = apply_edit(Some(&existing), &edits).unwrap();
        assert!(out.contains(mcp), "{out:?}");
        assert!(
            out.contains("enabled = false") && out.contains("approval = \"auto\""),
            "{out:?}"
        );
        // A file with no ai.mcp gains none.
        let out = apply_edit(Some("[ai]\nnotify = true\n"), &edits).unwrap();
        assert!(!out.contains("mcp"), "{out:?}");
        // Container of the wrong type is not touched either.
        let odd = "[ai]\nmcp = 3\n";
        let out = apply_edit(Some(odd), &edits).unwrap();
        assert!(out.contains("mcp = 3"), "{out:?}");
    }

    // W2: the saved text re-parses to the same servers and the new keys, and
    // the server block keeps its bytes and its place after the edits.
    #[test]
    fn a_save_reparses_with_the_new_keys_and_the_servers_untouched() {
        let mcp = "# my servers\n[[ai.mcp.servers]]\nname = \"docs\" # first\ncommand = \"/usr/bin/docs-mcp\"\nenv = [\"TOKEN\"]\nproject = \"/home/u/p\"\n\n\
            # broken one\n[[ai.mcp.servers]]\nname = 5\n\n\
            [[ai.mcp.servers]]\nname = \"b\"\ncommand = \"b\"\n";
        let layouts = [
            format!("[statusline]\nposition = \"top\"\n\n{mcp}"),
            format!("[ai]\nnotify = true\n\n{mcp}"),
            format!("[ai]\nnotify = true\n\n[ai.tools]\nenabled = true\n\n{mcp}"),
        ];
        let edits = [
            ConfigEdit::tools_flag(false),
            ConfigEdit::approval(crate::core::ai::config::ApprovalMode::Auto),
        ];
        for existing in layouts {
            let out = apply_edit(Some(&existing), &edits).unwrap();
            let (_, before, _) = Config::load_with_ai(Ok(existing.clone()), None);
            let (_, after, _) = Config::load_with_ai(Ok(out.clone()), None);
            assert!(!after.tools_enabled(), "{out:?}");
            assert_eq!(
                after.approval(),
                crate::core::ai::config::ApprovalMode::Auto,
                "{out:?}"
            );
            assert_eq!(after.mcp_servers(), before.mcp_servers(), "{out:?}");
            assert_eq!(
                after.mcp_servers().len(),
                2,
                "the broken entry stays skipped"
            );
            let at = out.find("# my servers").unwrap();
            // Where the table is new it is appended after the servers, so the
            // block keeps its offset; an existing table is edited above them.
            let start = existing.find("# my servers").unwrap();
            if existing.contains("[ai.tools]") {
                assert_eq!(&out[at..], mcp, "{out:?}");
            } else {
                assert_eq!(at, start, "{out:?}");
                assert_eq!(&out[at..at + mcp.len()], mcp, "{out:?}");
                assert_eq!(
                    &out[at + mcp.len()..],
                    "\n[ai.tools]\nenabled = false\napproval = \"auto\"\n"
                );
            }
        }
    }

    // Spec (config-persistence): the orchestrate slot and the orchestrator keys.
    fn orchestrate_edits(engine: &'static str, model: &'static str) -> [ConfigEdit; 2] {
        let table = crate::core::ai::config::Task::Orchestrate.table();
        let owned = |key, value: &'static str| ConfigEdit {
            table,
            key,
            value: std::borrow::Cow::Borrowed(value),
        };
        [owned("engine", engine), owned("model", model)]
    }

    // Spec: Save the slot.
    #[test]
    fn the_orchestrate_slot_is_saved_in_its_own_table_and_reads_back() {
        use crate::core::ai::config::{Assignment, Task};
        let existing = "# mine\n[ai]\nengine = \"x\"  # keep\nunknown = 1\n";
        let out = apply_edit(Some(existing), &orchestrate_edits("claude", "opus")).unwrap();
        assert_eq!(
            out,
            format!("{existing}\n[ai.tasks.orchestrate]\nengine = \"claude\"\nmodel = \"opus\"\n")
        );
        let (_, ai, notice) = Config::load_with_ai(Ok(out), None);
        assert_eq!(notice, None);
        assert_eq!(
            ai.get(Task::Orchestrate),
            &Assignment {
                engine: Some("claude".into()),
                model: Some("opus".into())
            }
        );
    }

    // Spec: Clearing removes the keys.
    #[test]
    fn clearing_the_orchestrate_slot_removes_its_keys_and_leaves_the_rest() {
        let rest = "# mine\n[ai]\nengine = \"x\"\n\n[ai.tasks.code]\nengine = \"codex\"\n";
        let with_slot =
            format!("{rest}\n[ai.tasks.orchestrate]\nengine = \"claude\"\nmodel = \"opus\"\n");
        let out = apply_edit(Some(&with_slot), &orchestrate_edits("", "")).unwrap();
        assert_eq!(out.trim_end(), rest.trim_end());
        // Nothing to remove: no table is created and the text is untouched.
        let out = apply_edit(Some(rest), &orchestrate_edits("", "")).unwrap();
        assert_eq!(out, rest);
        // Another key of the table survives; a half-cleared slot keeps the rest.
        let kept = "[ai.tasks.orchestrate]\nengine = \"claude\"\nmodel = \"opus\"\nextra = 1\n";
        let out = apply_edit(Some(kept), &orchestrate_edits("", "")).unwrap();
        assert_eq!(out, "[ai.tasks.orchestrate]\nextra = 1\n");
    }

    #[test]
    fn the_other_tasks_still_write_empty_strings_when_cleared() {
        let out = apply_edit(None, &code_edits("", "")).unwrap();
        assert_eq!(out, "[ai.tasks.code]\nengine = \"\"\nmodel = \"\"\n");
    }

    // Spec: Notice acceptance saved, with no other change pending.
    #[test]
    fn the_orchestrator_flags_are_written_as_booleans() {
        let flags = [
            ConfigEdit::orchestrator_flag("enabled", true),
            ConfigEdit::orchestrator_flag("notice_accepted", true),
        ];
        let out = apply_edit(None, &flags).unwrap();
        assert_eq!(
            out,
            "[ai.orchestrator]\nenabled = true\nnotice_accepted = true\n"
        );
        let (_, ai, notice) = Config::load_with_ai(Ok(out), None);
        assert_eq!(notice, None);
        assert!(ai.orchestrator.enabled && ai.orchestrator.notice_accepted);
        // Alone, in place; the hand-edited timeout is never touched.
        let existing = "[ai.orchestrator]\nworker_timeout_secs = 90  # mine\n";
        let out = apply_edit(
            Some(existing),
            &[ConfigEdit::orchestrator_flag("notice_accepted", true)],
        )
        .unwrap();
        assert_eq!(out, format!("{existing}notice_accepted = true\n"));
    }
}
