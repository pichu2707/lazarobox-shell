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
fn the_table_has_twelve_distinct_non_empty_prompts_within_4_kib() {
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
    assert_eq!(count, 12);
}

// Spec (prompt-profiles): Built-ins exist (the orchestrate prompt), one per
// profile, with every coordinator rule; the user's server tools ask each
// call and never reach a worker (S1e-1c).
#[test]
fn the_orchestrate_prompts_hold_the_coordinator_rules() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        assert!(!prompt.trim().is_empty(), "{profile:?}");
        assert_ne!(prompt, builtin(profile, Task::Chat), "{profile:?}");
        for rule in [
            "coordinator",
            "only through the orchestration tools",
            "spawn_agent",
            "wait",
            "collect",
            "cancel",
            "handoff",
            "finish",
            "on_course, off_course or blocked",
            "by kind",
            "Never approve",
            "untrusted data",
            "awaiting_user",
            "tests passed without evidence",
            "the user's own server tools",
            "each call asks the user first",
            "Workers never get these tools",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
    }
}

// Spec (prompt-profiles, Playbook per step; agent-orchestrator S5): one
// playbook per ODD step, in order, with the closing, question and size rules,
// the feature document in the user's words and `implement` as drafts.
#[test]
fn the_orchestrate_prompts_hold_one_playbook_per_odd_step_in_order() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        let mut at = 0;
        for step in [
            "authorize",
            "explore",
            "resolve",
            "classify",
            "track",
            "implement",
            "close",
        ] {
            let found = prompt[at..]
                .find(&format!("\n- {step}"))
                .unwrap_or_else(|| panic!("{profile:?}: playbook {step} in order"));
            at += found + 1;
        }
        for rule in [
            "closes only with its handoff",
            "at most one question to the user at a time",
            "never by counting files or tasks",
            "read_only",
            "scope",
            "size",
            "the user's own words",
            "correction",
            "orchestra/<run>/feature",
            "drafts and review only",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
    }
}

// Spec (agent-orchestrator, Implement is a draft before S7; Write
// integration and change ledger [S7]): the coordinator knows a `code`
// worker's changes reach the disk only through the user's batch review, and
// that only a file `collect` lists as applied was written.
#[test]
fn the_orchestrate_prompts_say_only_an_applied_file_was_written() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        for rule in [
            "propose_batch",
            "nothing reaches the disk until the user approves",
            "files changed",
            "not applied",
            "Only a file listed as applied was written",
            "draft",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
    }
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
        let got = with_tools(chat, FamilySet::LEGACY);
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
        "cd, pwd, ls, tree, find, grep and rg,",
        "one command with plain or quoted paths",
        "without &&, ;, |, $, * or redirections",
        "can run directly in the chat's folder when the user allows it",
        "Anything else, git and mkdir included, waits for the user to review it and press Enter",
        "carries the output of a command that ran, which may be \"not available\"",
        "You cannot change files yourself; any change needs a command the user reviews and runs.",
        "one simple command per call",
        "instead of asking the user to run find, ls or cat",
        "cannot change files",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(!text.contains("mcp__") && !text.contains("run_in_pane"));
    // After S4b the output of a command that ran comes back.
    assert!(!text.contains("unless the user pastes"));
    assert!(!text.contains("except through those commands"));
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
    let got = with_tools("My own prompt.\n\nWorkspace: x\n", FamilySet::LEGACY);
    assert_eq!(
        got,
        format!("My own prompt.\n\nWorkspace: x\n\n- {}", TOOLS.trim_end())
    );
}

#[test]
fn the_workspace_block_after_the_sentence_is_kept() {
    let chat = builtin(Profile::Programmer, Task::Chat).trim_end();
    let system = format!("{chat}\n\nWorkspace facts: cwd /home/u");
    let got = with_tools(&system, FamilySet::LEGACY);
    assert!(got.ends_with("Workspace facts: cwd /home/u"), "{got}");
}

#[test]
fn an_empty_prompt_becomes_the_block_alone() {
    assert_eq!(with_tools("", FamilySet::LEGACY), TOOLS.trim_end());
}

// #439: a turn that offers the write, exec or git write tools says the model
// can do the work and must finish every step, instead of "cannot change
// files"; read, search, git read and pane tools keep the sentence.
#[test]
fn the_coding_families_replace_the_cannot_change_sentence() {
    let chat = builtin(Profile::Programmer, Task::Chat);
    for family in [Family::Write, Family::Exec, Family::GitWrite] {
        let got = with_tools(chat, FamilySet::LEGACY.with(family));
        assert!(!got.contains(NO_CHANGES), "{family:?}");
        assert!(!got.contains("cannot change files"), "{family:?}");
        assert!(!got.contains("{abilities}"), "{family:?}");
        assert_eq!(got.matches("do all of them in this turn").count(), 1);
        assert!(got.contains("one simple command per call"), "{family:?}");
        assert!(got.contains(REPLY_IN_USER_LANGUAGE), "{family:?}");
    }
    for families in [
        FamilySet::LEGACY,
        FamilySet::LEGACY.with(Family::Search),
        FamilySet::LEGACY.with(Family::Git),
    ] {
        let got = with_tools(chat, families);
        assert!(got.contains(TOOLS.trim_end()), "{families:?}");
        assert!(!got.contains("do all of them in this turn"), "{families:?}");
    }
}

// #439: the block names only what the families of the turn allow.
#[test]
fn the_coding_block_names_only_the_abilities_of_the_turn() {
    const WRITE: &str = "write and edit files (they stay staged until you propose the batch, which the user reviews once)";
    let line = |families: FamilySet| with_tools("", families);
    let exec = line(FamilySet::LEGACY.with(Family::Exec));
    assert!(
        exec.contains("given: run tests and builds; the user approves"),
        "{exec}"
    );
    assert!(
        !exec.contains(WRITE) && !exec.contains("commit with git"),
        "{exec}"
    );
    let two = line(FamilySet::LEGACY.with(Family::Write).with(Family::GitWrite));
    assert!(
        two.contains(&format!("given: {WRITE} and add and commit with git;")),
        "{two}"
    );
    let all = line(
        FamilySet::LEGACY
            .with(Family::Write)
            .with(Family::Exec)
            .with(Family::GitWrite),
    );
    assert!(
        all.contains(&format!(
            "given: {WRITE}, run tests and builds and add and commit with git;"
        )),
        "{all}"
    );
}

// #439: the same holds for a user's prompt that gets the block as a bullet.
#[test]
fn a_prompt_without_the_sentence_gets_the_coding_block_too() {
    let families = FamilySet::LEGACY.with(Family::Write);
    let got = with_tools("My own prompt.\n", families);
    assert!(got.starts_with("My own prompt.\n\n- "), "{got}");
    assert!(
        got.contains("do all of them in this turn") && !got.contains(NO_CHANGES),
        "{got}"
    );
}

#[test]
fn the_coding_block_asks_to_finish_every_step_and_claim_only_shown_results() {
    assert!(TOOLS.contains(NO_CHANGES));
    let text = CODING;
    for needle in [
        "with the tools you are given: {abilities};",
        "do all of them in this turn, one tool call after another",
        "Do not stop to say what you will do next: call the tool.",
        "Say a step is done only when a tool result of this turn shows it",
        "never claim a file, a test run or a commit that no result shows",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(
        text.lines().count() == 1 && text.len() < 600,
        "{}",
        text.len()
    );
    assert!(text.chars().all(|c| c == '\n' || (' '..='~').contains(&c)));
}
