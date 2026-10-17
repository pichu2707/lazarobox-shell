use super::*;

const CHAT_LIMITS: &str = "no access to files, a shell or the terminal";
const REPLY_IN_USER_LANGUAGE: &str = "Reply in the language the user writes in";

fn text(s: &str) -> Override {
    Override::Text(s.to_owned())
}

fn resolve(profile: Profile, task_file: Override, general_file: Override) -> Resolved {
    resolve_prompt(profile, Task::Chat, task_file, general_file)
}

// Spec (prompt-profiles): Prompt table integrity.
#[test]
fn the_table_has_ten_distinct_non_empty_prompts_within_4_kib() {
    let mut count = 0;
    for task in Task::ALL {
        for profile in Profile::ALL {
            let prompt = builtin(profile, task);
            assert!(!prompt.trim().is_empty(), "{profile:?} {task:?}");
            assert!(
                prompt.len() <= 4096,
                "{profile:?} {task:?}: {}",
                prompt.len()
            );
            count += 1;
        }
        assert_ne!(
            builtin(Profile::Analyst, task),
            builtin(Profile::Programmer, task),
            "{task:?}"
        );
    }
    assert_eq!(count, 10);
}

// Spec (prompt-profiles): Reply in the user's language.
#[test]
fn every_prompt_asks_for_the_users_language() {
    for task in Task::ALL {
        for profile in Profile::ALL {
            assert!(
                builtin(profile, task).contains(REPLY_IN_USER_LANGUAGE),
                "{profile:?} {task:?}"
            );
        }
    }
}

// Spec (prompt-profiles): Chat prompt states the limits.
#[test]
fn the_chat_prompts_state_there_is_no_files_shell_or_terminal() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Chat);
        assert!(prompt.contains(CHAT_LIMITS), "{profile:?}");
        assert!(prompt.contains("workspace facts"), "{profile:?}");
    }
}

// Spec (prompt-profiles): the built-ins are English text, no raw controls.
#[test]
fn the_prompts_are_plain_ascii_text() {
    for task in Task::ALL {
        for profile in Profile::ALL {
            let prompt = builtin(profile, task);
            assert!(
                prompt
                    .chars()
                    .all(|c| c == '\n' || (' '..='~').contains(&c)),
                "{profile:?} {task:?}"
            );
        }
    }
}

// Spec: Built-in when no files.
#[test]
fn no_files_gives_the_builtin_and_no_notice() {
    let got = resolve(Profile::Analyst, Override::Missing, Override::Missing);
    assert_eq!(got.text, builtin(Profile::Analyst, Task::Chat).trim_end());
    assert!(got.notices.is_empty());
}

// Spec: Task override wins.
#[test]
fn the_task_override_wins_over_general() {
    let got = resolve(Profile::Analyst, text("task"), text("general"));
    assert_eq!(got.text, "task");
    assert!(got.notices.is_empty());
}

// Spec: General override is the second level.
#[test]
fn the_general_override_is_the_second_level() {
    let got = resolve(Profile::Analyst, Override::Missing, text("general"));
    assert_eq!(got.text, "general");
}

// Spec: Override content is exact (trailing whitespace trimmed only).
#[test]
fn the_override_is_exact_after_trimming_the_end() {
    let got = resolve(
        Profile::Programmer,
        text("  Be brief.\n\n"),
        Override::Missing,
    );
    assert_eq!(got.text, "  Be brief.");
}

// Spec: Bad task file, good general file.
#[test]
fn a_skipped_task_file_notes_it_and_falls_to_general() {
    let got = resolve(
        Profile::Analyst,
        Override::Skipped(Reason::NotUtf8),
        text("general"),
    );
    assert_eq!(got.text, "general");
    assert_eq!(
        got.notices,
        ["prompt override ignored: analyst/chat.md not valid UTF-8"]
    );
}

// Spec: both levels skipped, the built-in is the last fallback.
#[test]
fn both_files_skipped_fall_to_the_builtin_with_two_notices() {
    let got = resolve(
        Profile::Programmer,
        Override::Skipped(Reason::TooLarge),
        Override::Skipped(Reason::NotRegular),
    );
    assert_eq!(
        got.text,
        builtin(Profile::Programmer, Task::Chat).trim_end()
    );
    assert_eq!(
        got.notices,
        [
            "prompt override ignored: programmer/chat.md larger than 16 KiB",
            "prompt override ignored: programmer/general.md not a regular file",
        ]
    );
}

#[test]
fn every_reason_has_its_text() {
    let reasons = [
        (Reason::NotUtf8, "not valid UTF-8"),
        (Reason::TooLarge, "larger than 16 KiB"),
        (Reason::Unreadable, "cannot be read"),
        (Reason::NotRegular, "not a regular file"),
    ];
    for (reason, want) in reasons {
        let got = resolve(
            Profile::Analyst,
            Override::Skipped(reason),
            Override::Missing,
        );
        assert_eq!(got.notices.len(), 1);
        assert!(got.notices[0].ends_with(want), "{:?}", got.notices);
    }
}

// The general task has one file: it is not read twice, and not noted twice.
#[test]
fn the_general_task_has_a_single_override_level() {
    let got = resolve_prompt(
        Profile::Analyst,
        Task::General,
        Override::Skipped(Reason::Unreadable),
        Override::Skipped(Reason::Unreadable),
    );
    assert_eq!(
        got.text,
        builtin(Profile::Analyst, Task::General).trim_end()
    );
    assert_eq!(
        got.notices,
        ["prompt override ignored: analyst/general.md cannot be read"]
    );
}

// Spec: Profile changes only prompts (the other profile's files are never
// handed to the function; the choice is by the profile argument alone).
#[test]
fn the_profile_picks_the_builtin_and_the_notice_path() {
    let a = resolve(Profile::Analyst, Override::Missing, Override::Missing);
    let p = resolve(Profile::Programmer, Override::Missing, Override::Missing);
    assert_ne!(a.text, p.text);
}

// Spec: Notice is clean.
#[test]
fn notices_hold_no_control_characters() {
    for task in Task::ALL {
        let got = resolve_prompt(
            Profile::Analyst,
            task,
            Override::Skipped(Reason::NotUtf8),
            Override::Skipped(Reason::NotUtf8),
        );
        for notice in got.notices {
            assert!(!notice.chars().any(char::is_control), "{notice:?}");
        }
    }
}

// Spec (agent-tools): a turn with tools stops telling the model it has none.
#[test]
fn the_tools_block_replaces_the_no_access_sentence_in_every_profile() {
    for profile in Profile::ALL {
        let chat = builtin(profile, Task::Chat);
        let got = with_tools(chat);
        assert!(!got.contains(CHAT_LIMITS), "{profile:?}");
        assert!(!got.contains("You are a text assistant."), "{profile:?}");
        assert!(got.contains(TOOLS.trim_end()), "{profile:?}");
        // Everything else of the prompt stays, in its place.
        assert!(got.contains(REPLY_IN_USER_LANGUAGE), "{profile:?}");
        assert!(got.contains("workspace facts"), "{profile:?}");
        assert!(got.starts_with(chat.lines().next().unwrap()), "{profile:?}");
        assert_eq!(got.matches(TOOLS.trim_end()).count(), 1, "{profile:?}");
    }
}

#[test]
fn the_tools_block_names_the_tools_by_purpose_and_the_pane_rule() {
    let text = TOOLS;
    for needle in [
        "find a directory",
        "list a directory",
        "read a file",
        "run a command in the user's shell pane",
        "only typed or typed and run",
        "cd, pwd, ls, tree and mkdir",
        "one command with plain or quoted paths",
        "without &&, ;, |, $ or redirections",
        "can run directly when the user allows it",
        "Anything else waits for the user to review it and press Enter",
        "do not see its output unless the user pastes it",
        "one simple command per call",
        "instead of asking the user to run find, ls or cat",
        "cannot change files",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(!text.contains("mcp__") && !text.contains("run_in_pane"));
    // D18: the same words in every approval mode.
    for mode in ["ask mode", "auto mode", "run mode", "`run`", "`auto`"] {
        assert!(!text.contains(mode), "{mode}");
    }
    assert!(
        text.lines().count() == 1 && text.len() < 900,
        "{}",
        text.len()
    );
    assert!(text.chars().all(|c| c == '\n' || (' '..='~').contains(&c)));
}

// A user's override file need not have the sentence: the block is appended.
#[test]
fn a_prompt_without_the_sentence_gets_the_block_as_a_last_bullet() {
    let got = with_tools("My own prompt.\n\nWorkspace: x\n");
    assert_eq!(
        got,
        format!("My own prompt.\n\nWorkspace: x\n\n- {}", TOOLS.trim_end())
    );
}

#[test]
fn the_workspace_block_after_the_sentence_is_kept() {
    let chat = builtin(Profile::Programmer, Task::Chat).trim_end();
    let system = format!("{chat}\n\nWorkspace facts: cwd /home/u");
    let got = with_tools(&system);
    assert!(got.ends_with("Workspace facts: cwd /home/u"), "{got}");
}

#[test]
fn an_empty_prompt_becomes_the_block_alone() {
    assert_eq!(with_tools(""), TOOLS.trim_end());
}
