//! The fields form of the Connectors area (mcp-connect, Secret fields): what
//! the list shows, the masked field, the confirmations, the backup action and
//! the sweep that no confidential value is drawn or formatted anywhere.

use super::ai_tests::{ch, press};
use super::connectors_tests::{declared, entry, presence, texts};
use super::footer_tests::hint;
use super::picker_tests::{key, send};
use super::*;
use crate::core::ai::mcp_secrets::SecretText;

const TOKEN: &str = "CLARITY_API_TOKEN";
const MASK: &str = "••••••••";
const CLARITY: &str = "[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"clarity-mcp-server\"\n";
const OTHER: &str = "[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"/tmp/other-server\"\n";
const OLD: &str = "[clarity]\nCLARITY_API_TOKEN = \"old-7f3a9c1e\"\n";

fn secret(text: &str) -> SecretText {
    let mut s = SecretText::new();
    s.push_str(text);
    s
}

fn save(server: &'static str, name: &'static str, value: &str) -> MenuCommand {
    MenuCommand::SaveSecret {
        server,
        name,
        value: secret(value),
    }
}

/// The fields list of entry `at`, with the disk copy given.
fn fields(at: usize, config: &str, secrets: &str) -> MenuState {
    let mut m = entry(Vec::new(), at);
    m.presence_reply(presence(config, secrets));
    assert_eq!(press(&mut m, 'f'), MenuCommand::None);
    m
}

fn type_in(m: &mut MenuState, text: &str) {
    for c in text.chars() {
        assert_eq!(press(m, c), MenuCommand::None);
    }
}

fn enter(m: &mut MenuState) -> MenuCommand {
    send(m, key(KeyCode::Enter))
}

/// The Clarity field open on an empty file.
fn clarity_field() -> MenuState {
    let mut m = fields(0, CLARITY, "");
    assert_eq!(enter(&mut m), MenuCommand::None);
    m
}

// Spec: confidential rows show only `not set` or `set`.
#[test]
fn a_confidential_row_shows_not_set_or_set_and_never_the_value() {
    let m = fields(0, CLARITY, "");
    assert!(
        texts(&m).contains(&format!("{TOKEN}: not set")),
        "{:?}",
        texts(&m)
    );
    let m = fields(0, CLARITY, OLD);
    let all = texts(&m).join("\n");
    assert!(all.contains(&format!("{TOKEN}: set")), "{all}");
    assert!(!all.contains("old-7f3a"), "{all}");
}

// Spec: Non-confidential values shown.
#[test]
fn non_confidential_values_are_shown_in_full_and_the_client_secret_only_as_set() {
    let m = fields(1, "", "[ga4]\nGOOGLE_CLOUD_PROJECT = \"my-proj\"\n");
    assert!(texts(&m).contains(&"GOOGLE_CLOUD_PROJECT: my-proj".to_owned()));
    let google = "[google]\nGOOGLE_OAUTH_CLIENT_ID = \"123-abc.apps.googleusercontent.com\"\n\
                  GOOGLE_OAUTH_CLIENT_SECRET = \"GOCSPX-hidden\"\nUSER_GOOGLE_EMAIL = \"me@x.org\"\n";
    let all = texts(&fields(3, "", google)).join("\n");
    assert!(all.contains("GOOGLE_OAUTH_CLIENT_ID: 123-abc.apps.googleusercontent.com"));
    assert!(all.contains("USER_GOOGLE_EMAIL: me@x.org"));
    assert!(all.contains("GOOGLE_OAUTH_CLIENT_SECRET: set"));
    assert!(!all.contains("GOCSPX"), "{all}");
}

#[test]
fn an_entry_without_values_has_no_fields_and_says_so() {
    let mut m = entry(Vec::new(), 4);
    assert_eq!(press(&mut m, 'f'), MenuCommand::None);
    assert_eq!(hint(&m), "this entry has no values to fill");
}

// Spec: Fixed-width mask.
#[test]
fn a_confidential_field_draws_a_fixed_mask_whatever_the_length() {
    // Characters the title and the hint do not hold, so none can be told apart.
    for pasted in ["9!7?/x".to_owned(), "9!7?/x".repeat(5)] {
        let mut m = clarity_field();
        m.on_paste(&pasted);
        let rows = texts(&m);
        assert!(rows.iter().any(|r| r.trim() == MASK), "{rows:?}");
        let all = rows.concat() + &hint(&m);
        assert!(!pasted.chars().any(|c| all.contains(c)), "{all}");
        assert!(!format!("{m:?}").contains("9!7?"));
    }
}

#[test]
fn a_field_that_is_empty_draws_no_mask_and_a_non_confidential_one_shows_its_text() {
    let m = clarity_field();
    assert!(!texts(&m).concat().contains(MASK));
    let mut m = fields(1, "", "");
    enter(&mut m);
    m.on_paste("my-proj");
    assert!(
        texts(&m).iter().any(|r| r.trim() == "my-proj"),
        "{:?}",
        texts(&m)
    );
}

#[test]
fn the_field_hint_names_the_value_and_the_keys() {
    let m = clarity_field();
    assert_eq!(
        hint(&m),
        "type or paste CLARITY_API_TOKEN; Enter saves · Esc cancels"
    );
}

// Spec: Save a new value.
#[test]
fn enter_on_a_typed_value_sends_the_save_without_asking() {
    let mut m = clarity_field();
    type_in(&mut m, "tok");
    assert_eq!(enter(&mut m), save("clarity", TOKEN, "tok"));
}

// Spec: while a secret field is open, every key goes to the field.
#[test]
fn every_plain_key_is_text_while_the_field_is_open() {
    let mut m = clarity_field();
    type_in(&mut m, "s4jf");
    assert_eq!(enter(&mut m), save("clarity", TOKEN, "s4jf"));
}

#[test]
fn backspace_edits_and_a_control_key_says_so() {
    let mut m = clarity_field();
    type_in(&mut m, "abc");
    send(&mut m, key(KeyCode::Backspace));
    let ctrl = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(send(&mut m, ctrl), MenuCommand::None);
    assert_eq!(hint(&m), "no such key here");
    assert_eq!(enter(&mut m), save("clarity", TOKEN, "ab"));
}

// Spec: empty Enter saves nothing.
#[test]
fn enter_on_an_empty_field_saves_nothing_and_says_how_to_remove_a_value() {
    let mut m = clarity_field();
    assert_eq!(enter(&mut m), MenuCommand::None);
    assert_eq!(
        hint(&m),
        "empty value not saved; edit secrets.toml by hand to remove a value"
    );
}

// Spec: Overwrite asks first.
#[test]
fn an_existing_value_asks_before_it_is_replaced() {
    let ask = |answer: char| {
        let mut m = fields(0, CLARITY, OLD);
        enter(&mut m);
        type_in(&mut m, "new");
        assert_eq!(enter(&mut m), MenuCommand::None);
        assert!(texts(&m).contains(&"replace the saved CLARITY_API_TOKEN? (y/n)".to_owned()));
        let sent = press(&mut m, answer);
        (sent, hint(&m))
    };
    let (sent, said) = ask('n');
    assert_eq!((sent, said.as_str()), (MenuCommand::None, "not replaced"));
    assert_eq!(ask('y').0, save("clarity", TOKEN, "new"));
    assert_eq!(ask('x').0, MenuCommand::None);
}

// Spec: Declared command differs.
#[test]
fn a_declared_command_that_differs_is_named_and_asked_before_every_save() {
    let mut m = fields(0, OTHER, "");
    let rows = texts(&m).join(" ");
    assert!(
        rows.contains("declared command differs from the catalog: /tmp/other-server"),
        "{rows:?}"
    );
    enter(&mut m);
    type_in(&mut m, "tok");
    assert_eq!(enter(&mut m), MenuCommand::None);
    let want = "save CLARITY_API_TOKEN for a server that runs /tmp/other-server? (y/n)";
    assert!(texts(&m).concat().contains(want), "{:?}", texts(&m));
    assert_eq!(press(&mut m, 'y'), save("clarity", TOKEN, "tok"));
}

#[test]
fn a_refused_command_prompt_saves_nothing() {
    let mut m = fields(0, OTHER, "");
    enter(&mut m);
    type_in(&mut m, "tok");
    enter(&mut m);
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(hint(&m), "not saved");
}

#[test]
fn the_command_prompt_comes_before_the_replace_prompt() {
    let mut m = fields(0, OTHER, OLD);
    enter(&mut m);
    type_in(&mut m, "new");
    enter(&mut m);
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    assert!(texts(&m).contains(&"replace the saved CLARITY_API_TOKEN? (y/n)".to_owned()));
    assert_eq!(press(&mut m, 'y'), save("clarity", TOKEN, "new"));
}

#[test]
fn a_matching_declared_command_has_no_warning_row() {
    let m = fields(0, CLARITY, "");
    assert!(!texts(&m).concat().contains("declared command differs"));
}

// Spec: Esc drops the text.
#[test]
fn esc_closes_the_field_and_drops_the_text() {
    let mut m = clarity_field();
    type_in(&mut m, "tok");
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    enter(&mut m);
    assert!(!texts(&m).concat().contains(MASK));
    assert_eq!(enter(&mut m), MenuCommand::None);
    assert!(hint(&m).starts_with("empty value"));
}

// Spec: Refused characters.
#[test]
fn a_paste_with_a_line_break_or_a_control_character_is_refused() {
    for bad in ["sk-1\nsk-2", "sk-1\r", "sk-1\u{7}"] {
        let mut m = clarity_field();
        m.on_paste(bad);
        assert_eq!(
            hint(&m),
            "the value must be one line without control characters"
        );
        assert_eq!(enter(&mut m), MenuCommand::None, "{bad:?} left text behind");
    }
}

#[test]
fn text_past_4096_bytes_is_refused_whole() {
    let mut m = clarity_field();
    m.on_paste(&"a".repeat(4096));
    m.on_paste("b");
    assert_eq!(hint(&m), "the value is longer than 4096 bytes");
    type_in(&mut m, "c");
    assert_eq!(hint(&m), "the value is longer than 4096 bytes");
    let MenuCommand::SaveSecret { value, .. } = enter(&mut m) else {
        panic!("a save")
    };
    assert_eq!(value.len(), 4096);
}

// Spec: preset rows are written by Enter without typing.
#[test]
fn a_preset_row_shows_its_preset_and_enter_sends_it() {
    let mut m = fields(3, "", "");
    assert!(
        texts(&m)
            .contains(&"WORKSPACE_MCP_DISABLE_LOCAL_FILES: not set · Enter writes true".to_owned()),
        "{:?}",
        texts(&m)
    );
    // The cursor starts on the first row; the preset is the fourth.
    for _ in 0..3 {
        press(&mut m, 'j');
    }
    assert_eq!(
        enter(&mut m),
        save("google", "WORKSPACE_MCP_DISABLE_LOCAL_FILES", "true")
    );
}

// Spec: the backup action.
#[test]
fn d_offers_the_backup_deletion_only_while_a_backup_exists() {
    let mut m = entry(Vec::new(), 0);
    m.presence_reply(presence(CLARITY, OLD).with_backup(true));
    press(&mut m, 'f');
    assert!(
        texts(&m).concat().contains("d delete secrets.toml.bak"),
        "{:?}",
        texts(&m)
    );
    assert_eq!(press(&mut m, 'd'), MenuCommand::None);
    assert!(
        texts(&m)
            .contains(&"delete secrets.toml.bak? It holds your previous values (y/n)".to_owned())
    );
    assert_eq!(press(&mut m, 'y'), MenuCommand::DeleteBackup);
    let mut m = entry(Vec::new(), 0);
    m.presence_reply(presence(CLARITY, OLD).with_backup(true));
    press(&mut m, 'f');
    press(&mut m, 'd');
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(hint(&m), "cancelled");
    let mut none = fields(0, CLARITY, OLD);
    assert!(!texts(&none).concat().contains("delete secrets.toml.bak"));
    assert_eq!(press(&mut none, 'd'), MenuCommand::None);
    assert_eq!(hint(&none), "secrets.toml.bak: there is no backup");
}

// Spec: the notice of a save, and the presence read again.
#[test]
fn a_save_reply_shows_its_notice_rereads_the_files_and_says_to_restart() {
    let mut m = fields(0, CLARITY, "");
    let said = "saved CLARITY_API_TOKEN; the previous values stay in secrets.toml.bak (0600) until you delete it";
    assert_eq!(
        m.secret_reply(Ok(said.to_owned())),
        MenuCommand::LoadPresence
    );
    assert_eq!(hint(&m), said);
    assert!(texts(&m).contains(&"restart the chat or probe again to use it".to_owned()));
    m.presence_reply(presence(CLARITY, OLD));
    assert!(texts(&m).contains(&format!("{TOKEN}: set")));
}

#[test]
fn a_refused_save_shows_the_refusal_and_does_not_say_to_restart() {
    let mut m = fields(0, CLARITY, "");
    let refusal = "secrets.toml is a symlink; edit it by hand";
    assert_eq!(m.secret_reply(Err(refusal.to_owned())), MenuCommand::None);
    assert_eq!(hint(&m), refusal);
    assert!(!texts(&m).concat().contains("restart the chat"));
}

#[test]
fn a_backup_reply_shows_its_notice_and_rereads_the_files() {
    let mut m = fields(0, CLARITY, "");
    assert_eq!(
        m.backup_reply(Ok("deleted secrets.toml.bak".to_owned())),
        MenuCommand::LoadPresence
    );
    assert_eq!(hint(&m), "deleted secrets.toml.bak");
}

// Spec: The value appears nowhere else.
#[test]
fn the_value_appears_in_no_row_notice_hint_or_debug_output() {
    let value = "tok-7f3a9c1e2b4d6f80";
    let mut m = clarity_field();
    m.on_paste(value);
    let command = enter(&mut m);
    let mut seen = vec![
        format!("{command:?}"),
        format!("{m:?}"),
        texts(&m).concat(),
        hint(&m),
    ];
    m.secret_reply(Ok("saved CLARITY_API_TOKEN".to_owned()));
    seen.extend([format!("{m:?}"), texts(&m).concat(), hint(&m)]);
    m.presence_reply(presence(
        CLARITY,
        &format!("[clarity]\n{TOKEN} = \"{value}\"\n"),
    ));
    seen.extend([format!("{m:?}"), texts(&m).concat(), hint(&m)]);
    let all = seen.join("\n");
    let parts: Vec<&str> = (0..=value.len() - 4).map(|i| &value[i..i + 4]).collect();
    for part in parts {
        assert!(!all.contains(part), "{part} leaked in {all}");
    }
}

/// The source files (tests apart) that hold `needle`.
fn files_with(needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = path.to_string_lossy().into_owned();
            if path.is_dir() {
                stack.push(path);
            } else if name.ends_with(".rs")
                && !name.contains("tests")
                && std::fs::read_to_string(&path).unwrap().contains(needle)
            {
                found.push(name.rsplit("/src/").next().unwrap().to_owned());
            }
        }
    }
    found.sort();
    found
}

// Spec: No other path to the save. The command is built by the secret field
// alone; the app forwards it, and only the runtime's writer calls the I/O.
#[test]
fn only_the_secret_field_builds_a_save_and_only_the_writer_runs_it() {
    assert_eq!(
        files_with("MenuCommand::SaveSecret"),
        ["app.rs", "core/menu/connectors/fields.rs"]
    );
    assert_eq!(
        files_with("Effect::SaveSecret"),
        ["app.rs", "runtime.rs", "runtime/secrets_write.rs"]
    );
    assert_eq!(files_with("save_secret("), ["runtime/secrets_write.rs"]);
    assert_eq!(files_with("Writer::new("), ["runtime.rs"]);
}

#[test]
fn a_declared_server_of_another_name_does_not_change_the_fields() {
    let mut m = entry(vec![declared("fs", true)], 0);
    press(&mut m, 'f');
    assert!(texts(&m).contains(&format!("{TOKEN}: not set")));
}

/// A key as the hold tracker flags it: the same key again within its window.
fn repeated(m: &mut MenuState, c: char) -> MenuCommand {
    m.on_key(&ch(c), true, &mut Config::default())
}

// A doubled letter is typed text, not a held key.
#[test]
fn a_doubled_character_typed_into_a_field_is_kept() {
    let mut m = clarity_field();
    press(&mut m, 'a');
    assert_eq!(repeated(&mut m, 'a'), MenuCommand::None);
    assert_eq!(enter(&mut m), save("clarity", TOKEN, "aa"));
}

#[test]
fn a_key_flagged_as_held_is_typed_every_time_in_a_field() {
    let mut m = clarity_field();
    press(&mut m, 'z');
    repeated(&mut m, 'z');
    repeated(&mut m, 'z');
    assert_eq!(enter(&mut m), save("clarity", TOKEN, "zzz"));
}

// Outside a field a repeat still only moves, so a held `y` cannot confirm twice.
#[test]
fn a_repeated_y_confirms_nothing_and_a_repeated_enter_opens_no_field() {
    let mut m = fields(0, CLARITY, OLD);
    enter(&mut m);
    type_in(&mut m, "new");
    enter(&mut m);
    assert!(texts(&m).contains(&"replace the saved CLARITY_API_TOKEN? (y/n)".to_owned()));
    assert_eq!(repeated(&mut m, 'y'), MenuCommand::None);
    assert!(texts(&m).contains(&"replace the saved CLARITY_API_TOKEN? (y/n)".to_owned()));
    assert_eq!(press(&mut m, 'y'), save("clarity", TOKEN, "new"));
    let enter_again = m.on_key(&key(KeyCode::Enter), true, &mut Config::default());
    assert_eq!(enter_again, MenuCommand::None);
    assert!(
        !texts(&m)
            .iter()
            .any(|r| r.contains("CLARITY_API_TOKEN") && r.trim().starts_with("••"))
    );
}

// The mask is drawn while the field is open, with the value in it.
#[test]
fn the_rows_drawn_while_the_field_is_open_hold_no_part_of_the_value() {
    let value = "tok-7f3a9c1e2b4d6f80";
    let mut m = clarity_field();
    for c in value.chars() {
        press(&mut m, c);
    }
    let all = format!("{}\n{}\n{m:?}", texts(&m).join("\n"), hint(&m));
    for at in 0..=value.len() - 4 {
        assert!(
            !all.contains(&value[at..at + 4]),
            "{} leaked",
            &value[at..at + 4]
        );
    }
    assert!(texts(&m).iter().any(|r| r.trim() == MASK));
}
