//! Pure edit of `config.toml` text: changes only the menu-owned keys and
//! keeps everything else byte-for-byte (comments, order, whitespace).

use toml_edit::{DocumentMut, Item, Table, Value};

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
/// Line endings: if the text contains any `\r\n`, the whole result uses
/// `\r\n` (a mixed file is made uniformly CRLF); otherwise it uses `\n`.
///
/// Known limits, not fixed on purpose: a missing trailing newline gets
/// added, and a leading UTF-8 BOM is dropped.
pub fn apply_edit(existing: Option<&str>, edits: &[ConfigEdit]) -> Result<String, EditError> {
    let crlf = existing.is_some_and(|text| text.contains("\r\n"));
    let mut doc = match existing {
        Some(text) => text
            .replace("\r\n", "\n")
            .parse::<DocumentMut>()
            .map_err(|_| EditError::Unparseable)?,
        None => DocumentMut::new(),
    };
    for edit in edits {
        if doc.get(edit.table).is_none() {
            let mut table = Table::new();
            if doc.as_table().is_empty() {
                // Comments of a file with no entries are the document's
                // trailing text; move them in front of the new table.
                let mut head = doc.trailing().as_str().unwrap_or_default().to_owned();
                if !head.is_empty() {
                    if !head.ends_with('\n') {
                        head.push('\n');
                    }
                    head.push('\n');
                }
                doc.set_trailing("");
                table.decor_mut().set_prefix(head);
            }
            doc.insert(edit.table, Item::Table(table));
        }
        let table = doc
            .get_mut(edit.table)
            .and_then(Item::as_table_like_mut)
            .ok_or(EditError::NotATable(edit.table))?;
        let mut value = Value::from(edit.value);
        match table.get_mut(edit.key) {
            // Replace in the slot so the key's own decor (leading comment,
            // spacing) stays, and keep the old value's decor so a trailing
            // `# comment` survives.
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
    }
    let text = doc.to_string();
    Ok(if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{BarPosition, Config};

    const fn edit(table: &'static str, value: &'static str) -> ConfigEdit {
        ConfigEdit {
            table,
            key: "position",
            value,
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
    fn a_mixed_file_with_any_crlf_is_written_with_crlf_throughout() {
        let out = apply_edit(Some("a = 1\r\nb = 2\n"), &[]).unwrap();
        assert_eq!(out, "a = 1\r\nb = 2\r\n");
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
}
