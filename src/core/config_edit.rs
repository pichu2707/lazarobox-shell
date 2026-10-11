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
}

/// Applies `edits` to the existing file text (`None` means no file yet) and
/// returns the new text.
pub fn apply_edit(existing: Option<&str>, edits: &[ConfigEdit]) -> Result<String, EditError> {
    let mut doc = match existing {
        Some(text) => text
            .parse::<DocumentMut>()
            .map_err(|_| EditError::Unparseable)?,
        None => DocumentMut::new(),
    };
    for edit in edits {
        if doc.get(edit.table).is_none() {
            doc.insert(edit.table, Item::Table(Table::new()));
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
    Ok(doc.to_string())
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
