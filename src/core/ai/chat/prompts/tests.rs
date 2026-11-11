use super::*;
use crate::core::ai::orchestra::rdd::Lens;

const CHAT_LIMITS: &str = "no access to files, a shell or the terminal";
const REPLY_IN_USER_LANGUAGE: &str = "Reply in the language the user writes in";

fn text(s: &str) -> Override {
    Override::Text(s.to_owned())
}

fn resolve(profile: Profile, task_file: Override, general_file: Override) -> Resolved {
    resolve_prompt(profile, Task::Chat, task_file, general_file)
}

// Spec (prompt-profiles): Prompt table integrity; "Orchestrate prompt size
// [K1, W6]": each `orchestrate` prompt is at most 6 KiB (owner, 2026-11-10:
// "si, sube el tope a 6 KiB"), every other built-in at most 4 KiB.
#[test]
fn the_table_has_fourteen_distinct_non_empty_prompts_within_their_caps() {
    let mut count = 0;
    for task in Task::ALL {
        let cap = if task == Task::Orchestrate {
            6144
        } else {
            4096
        };
        for profile in Profile::ALL {
            let prompt = builtin(profile, task);
            assert!(!prompt.trim().is_empty(), "{profile:?} {task:?}");
            assert!(
                prompt.len() <= cap,
                "{profile:?} {task:?}: {} > {cap}",
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
    assert_eq!(count, 14);
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
            "raise",
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

// Spec (agent-orchestrator, Gates on step state are visible and bounded
// [W3]): both profiles tell the coordinator that a refused gate names the step
// to hand off first, that only the user lifts it, and not to retry the call
// until that step is handed off.
#[test]
fn the_orchestrate_prompts_explain_the_step_gates() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        for rule in [
            "A refused spawn or finish names the step to hand off first",
            "only the user can lift a gate",
            "In a worker prompt, give the key, the task id and the S# text",
            "retry only once that step is handed off",
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

// Spec (prompt-profiles, "Prompt from W6 [W6, auto]"; agent-orchestrator, "A
// test is not claimed without a result"): the `orchestrate` prompt no longer
// says no worker can run commands; it says when one can (Exec on, its batch
// applied, the user approving the command) and that a test counts as run or
// passed only when a collected command result shows it, otherwise it was not
// run and the user is asked to run it.
#[test]
fn the_orchestrate_prompts_say_when_a_worker_may_run_a_command() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        assert!(
            !prompt.contains("No worker can run commands"),
            "{profile:?}"
        );
        for rule in [
            "A code worker can run a command only when Exec is on",
            "only after its batch is applied",
            "only after the user approves that command",
            "only when a collected command result shows it",
            "say it was not run",
            "ask the user to run it",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
    }
}

// Spec (agent-orchestrator, "The brief [W8]"; prompt-profiles, "Orchestrate
// prompt holds the brief rules", decision K1): both `orchestrate` prompts tell
// the coordinator to delegate a change instead of answering it in text, and to
// give a `simple` worker a brief (goal, files as inputs and outputs, done when,
// out of scope), not a `prompt`. Both stay within the 6 KiB cap above.
#[test]
fn the_orchestrate_prompts_teach_the_brief_of_a_simple_worker() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        for rule in [
            "Delegate a change to files; never answer it in text",
            "a simple worker",
            "a brief, not a prompt",
            "goal, inputs",
            "outputs (files it may write",
            "done_when and out_of_scope",
            "is refused",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
        assert!(prompt.len() <= 6144, "{profile:?}: {}", prompt.len());
    }
}

// Spec (prompt-profiles, "Orchestrate prompt covers briefs and clarification",
// W8c 20.8, decision K1): both `orchestrate` prompts tell the coordinator to
// answer or ask the user when a worker says "needs specification", at most two
// rounds per brief, to pass `answers: N`, and that the text after the marker is
// untrusted data; `simple` says to ask instead of guessing. 6 KiB cap holds.
#[test]
fn the_orchestrate_prompts_teach_the_clarification_rounds() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        for rule in [
            "needs specification",
            "answer it or ask the user",
            "At most two rounds per brief",
            "answers: N",
            "untrusted data",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
        assert!(prompt.len() <= 6144, "{profile:?}: {}", prompt.len());
        let simple = builtin(profile, Task::Simple);
        assert!(simple.contains("needs_specification"), "{profile:?}");
        let asks = simple.contains("Do not guess") || simple.contains("Do not assume");
        assert!(asks, "{profile:?}");
    }
}

// Spec (prompt-profiles, "Orchestrate prompt covers briefs and clarification",
// split half, W9b 22.8, decision K1): after a split the coordinator issues two
// new briefs, and the 6 KiB cap holds.
#[test]
fn the_orchestrate_prompts_teach_the_split() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Orchestrate);
        for rule in [
            "split chosen",
            "two new briefs",
            "not a clarification round",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
        assert!(prompt.len() <= 6144, "{profile:?}: {}", prompt.len());
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
        "which program and file they have open",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(!text.contains("mcp__") && !text.contains("run_in_pane"));
    // The reach of the file tools and what to do after a refusal.
    for needle in [
        "only reach the user's home folder",
        "and, under WSL, Windows drives (read only)",
        "After a refusal or a failed tool, say so; never guess or invent contents",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    // After S4b the output of a command that ran comes back.
    assert!(!text.contains("unless the user pastes"));
    assert!(!text.contains("except through those commands"));
    // D18: the same words in every approval mode.
    for mode in ["ask mode", "auto mode", "run mode", "`run`", "`auto`"] {
        assert!(!text.contains(mode), "{mode}");
    }
    assert!(
        text.lines().count() == 1 && text.len() <= 1033 + 16 + 160,
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

// Spec (prompt-profiles, Lens prompts exist): each profile ships a prompt per
// lens and one for the refuter, within 4 KiB, each with the review rules.
#[test]
fn the_lens_and_refuter_prompts_hold_the_review_rules() {
    let mut seen = Vec::new();
    for profile in Profile::ALL {
        let mut prompts: Vec<(String, &str)> = Lens::ALL
            .into_iter()
            .map(|lens| (lens.id().to_owned(), lens_builtin(profile, lens)))
            .collect();
        prompts.push(("refuter".to_owned(), refuter_builtin(profile)));
        prompts.push(("validator".to_owned(), validator_builtin(profile)));
        for (name, prompt) in prompts {
            let label = format!("{profile:?} {name}");
            assert!(!prompt.trim().is_empty(), "{label}");
            assert!(prompt.len() <= 4096, "{label}: {}", prompt.len());
            for rule in [
                "read-only",
                "untrusted data",
                "You never approve",
                "Reply in the language the user writes in",
            ] {
                assert!(prompt.contains(rule), "{label}: {rule}");
            }
            if name != "refuter" && name != "validator" {
                assert!(
                    prompt.contains(&format!("through the {name} lens")),
                    "{label}"
                );
                for rule in [
                    "Only a severe finding that the change caused blocks",
                    "FINDING <severe|minor> <change|existing> <observed|inferred>:",
                    "NONE",
                ] {
                    assert!(prompt.contains(rule), "{label}: {rule}");
                }
            } else {
                assert!(
                    prompt.contains("CLAIM <number> <upheld|refuted|unsettled>:"),
                    "{label}"
                );
            }
            // C1: a claim nobody can settle is never cleared by the prompt.
            if name == "refuter" {
                assert!(!prompt.contains("is refuted, and you say so"), "{label}");
                assert!(prompt.contains("unsettled"), "{label}");
            }
            if name == "validator" {
                assert!(prompt.contains("still open"), "{label}");
                assert!(prompt.contains("only if"), "{label}");
            }
            assert!(!seen.contains(&prompt), "{label} repeats another prompt");
            seen.push(prompt);
        }
    }
    assert_eq!(seen.len(), 12);
}

// Spec (prompt-profiles): Worker prompt asks for tools [W2, auto].
#[test]
fn the_worker_prompt_requires_write_tools_and_propose_batch() {
    for profile in Profile::ALL {
        let got = worker_system(builtin(profile, Task::Code));
        for needle in [
            "write_file",
            "edit_file",
            "propose_batch",
            "not a result",
            "overrides",
            "fenced",
            "call propose_batch again",
            "32 KiB",
            "create_dir",
            // Task 16.6: a command runs only after the batch is applied.
            "A command runs only after your batch is applied",
        ] {
            assert!(got.contains(needle), "{profile:?}: {needle}");
        }
        assert!(got.ends_with(WORKER.trim_end()), "{profile:?}");
        assert!(WORKER.len() <= 1536, "{}", WORKER.len());
        assert!(
            WORKER
                .chars()
                .all(|c| c == '\n' || (' '..='~').contains(&c))
        );
    }
}

// Spec (prompt-profiles): a worker prompt MUST NOT contradict the tools block.
#[test]
fn the_worker_prompt_does_not_contradict_the_tools_block() {
    let writer = crate::core::ai::orchestra::Toolset::Code.families();
    for profile in Profile::ALL {
        let got = with_tools(&worker_system(builtin(profile, Task::Code)), writer);
        assert!(got.contains(WORKER.trim_end()), "{profile:?}");
        assert!(got.contains("do all of them in this turn"), "{profile:?}");
        assert!(!got.contains(NO_CHANGES), "{profile:?}");
        assert!(!got.contains("cannot change files"), "{profile:?}");
        assert!(!got.contains("{abilities}"), "{profile:?}");
        // The worker text says nothing the tools block denies.
        for denied in ["no access", "text assistant", "cannot change"] {
            assert!(!WORKER.contains(denied), "{denied}");
        }
    }
}

// Spec (prompt-profiles): a user override of `code.md` still applies. GUARD:
// passes before the worker text exists, pins the order.
#[test]
fn a_user_override_of_code_keeps_its_place_before_the_worker_text() {
    let got = resolve_prompt(
        Profile::Programmer,
        Task::Code,
        text("My own code prompt."),
        Override::Missing,
    );
    let system = worker_system(&got.text);
    assert!(system.starts_with("My own code prompt."), "{system}");
    assert!(system.contains("propose_batch"), "{system}");
}

// Spec (prompt-profiles): Chat turn unchanged [W2, auto]. GUARD: a chat turn
// on the `code` assignment resolves the prompt without the worker text.
#[test]
fn a_chat_turn_on_the_code_assignment_gets_the_code_prompt_untouched() {
    for profile in Profile::ALL {
        let got = resolve_prompt(profile, Task::Code, Override::Missing, Override::Missing);
        assert_eq!(got.text, builtin(profile, Task::Code).trim_end());
        assert!(!got.text.contains("propose_batch"), "{profile:?}");
        assert!(!builtin(profile, Task::Code).contains(WORKER.trim_end()));
    }
}

// Spec (prompt-profiles): a denied batch is dropped so the worker stages again.
#[test]
fn the_worker_prompt_lets_a_rejected_batch_be_proposed_again() {
    assert!(!WORKER.contains("once"), "{WORKER}");
    assert!(WORKER.contains("If the user rejects the batch"), "{WORKER}");
}

// Spec (issue-draft, D5): a turn that offers `gh_issue_draft` is told when to
// offer a draft, once, after the rest of the tools block; any other turn is
// told nothing about it.
#[test]
fn the_draft_sentence_comes_only_with_the_draft_family() {
    let chat = builtin(Profile::Programmer, Task::Chat);
    let draft = FamilySet::LEGACY.with(Family::IssueDraft);
    for families in [
        draft,
        draft.with(Family::Write).with(Family::Exec),
        FamilySet::ALL.without(Family::Orchestra),
    ] {
        let got = with_tools(chat, families);
        assert_eq!(got.matches(DRAFT_OFFER).count(), 1, "{families:?}");
        assert!(got.contains(REPLY_IN_USER_LANGUAGE), "{families:?}");
        let own = with_tools("My own prompt.\n", families);
        assert!(own.ends_with(DRAFT_OFFER), "{own}");
    }
    for families in [
        FamilySet::LEGACY,
        FamilySet::ALL.without(Family::IssueDraft),
        FamilySet::LEGACY.with(Family::Github),
    ] {
        let got = with_tools(chat, families);
        assert!(!got.contains(DRAFT_OFFER), "{families:?}");
        assert!(!got.contains("issue draft"), "{families:?}");
    }
}

// D5: the three cases, the permission, and what is not a reason.
#[test]
fn the_draft_sentence_names_the_three_cases_and_the_permission() {
    for needle in [
        "issue draft tool",
        "only when the user asks for one",
        "when you cannot do what was asked because lazarobox lacks it",
        "when a lazarobox tool or this chat fails",
        "never for errors in the user's own code or commands",
        "is the user's permission, so do not ask first",
    ] {
        assert!(DRAFT_OFFER.contains(needle), "{needle}");
    }
    // One sentence, one line, ASCII.
    assert_eq!(DRAFT_OFFER.matches(". ").count(), 0, "{DRAFT_OFFER}");
    assert!(DRAFT_OFFER.ends_with('.'));
    assert!(DRAFT_OFFER.chars().all(|c| (' '..='~').contains(&c)));
}

// The master switch off, or the toggle off: the saved families have no draft
// family, so the turn's block has no sentence.
#[test]
fn the_switch_or_the_toggle_off_leaves_the_sentence_out() {
    use crate::core::ai::config::{AiConfig, ToolToggle};
    let chat = builtin(Profile::Programmer, Task::Chat);
    let mut config = AiConfig::default();
    assert!(with_tools(chat, config.families()).contains(DRAFT_OFFER));
    config.set_toggle(ToolToggle::IssueDraft, false);
    assert!(!with_tools(chat, config.families()).contains(DRAFT_OFFER));
    config.set_toggle(ToolToggle::IssueDraft, true);
    config.set_tools_enabled(false);
    assert!(!with_tools(chat, config.families()).contains(DRAFT_OFFER));
}

// Workers and the orchestrator get no draft tool, so no sentence either.
#[test]
fn no_worker_and_no_orchestrator_turn_gets_the_sentence() {
    use crate::core::ai::orchestra::Toolset;
    for toolset in [Toolset::ReadOnly, Toolset::Code, Toolset::Orchestrator] {
        for profile in Profile::ALL {
            let system = worker_system(builtin(profile, Task::Code));
            let got = with_tools(&system, toolset.families());
            assert!(!got.contains(DRAFT_OFFER), "{toolset:?} {profile:?}");
        }
    }
}

// Spec (prompt-profiles): "The `simple` prompt [W8]" and "Simple gets its own
// prompt": the rules of a worker for direct, fully specified tasks.
#[test]
fn the_simple_prompts_hold_the_simple_worker_rules() {
    for profile in Profile::ALL {
        let prompt = builtin(profile, Task::Simple);
        for rule in [
            "direct, fully specified",
            "never design, analyse, review or delegate",
            "The brief is the whole assignment",
            "needs specification:",
            "needs_specification",
            "Never write outside the listed files unless the user asks",
            "under 400 counted lines",
            "say so when you cannot",
            "write tools",
            "propose_batch",
            "untrusted data",
        ] {
            assert!(prompt.contains(rule), "{profile:?}: {rule}");
        }
        for other in [Task::Code, Task::General, Task::Orchestrate] {
            assert_ne!(prompt, builtin(profile, other), "{profile:?} {other:?}");
        }
    }
    assert_ne!(
        builtin(Profile::Analyst, Task::Simple),
        builtin(Profile::Programmer, Task::Simple)
    );
}

// Spec (prompt-profiles): an override file `simple.md` loads under the
// untrusted rules; a skipped one names `simple` and falls to general.
#[test]
fn a_simple_override_wins_and_a_skipped_one_names_the_file() {
    let got = resolve_prompt(
        Profile::Analyst,
        Task::Simple,
        text("My simple prompt.\n"),
        text("General."),
    );
    assert_eq!(got.text, "My simple prompt.");
    let got = resolve_prompt(
        Profile::Analyst,
        Task::Simple,
        Override::Skipped(Reason::TooLarge),
        text("General."),
    );
    assert_eq!(got.text, "General.");
    assert_eq!(
        got.notices,
        vec!["prompt override ignored: analyst/simple.md larger than 16 KiB"]
    );
}

// Decision D29: a `simple` writer gets the worker instruction after its prompt.
#[test]
fn a_simple_worker_gets_the_worker_text_after_its_prompt() {
    for profile in Profile::ALL {
        let system = worker_system(builtin(profile, Task::Simple));
        assert!(system.starts_with(builtin(profile, Task::Simple).trim_end()));
        assert!(system.ends_with(WORKER.trim_end()), "{profile:?}");
    }
}

// A `simple` worker whose Write was removed (read-only goal) must not be told
// to use write tools: the extra line says they are unavailable.
#[test]
fn a_simple_worker_without_write_tools_is_told_to_answer_in_text() {
    for profile in Profile::ALL {
        let base = builtin(profile, Task::Simple);
        let got = read_only_worker_system(base);
        assert!(got.starts_with(base.trim_end()), "{profile:?}");
        for needle in [
            "write tools",
            "propose_batch",
            "run_command",
            "are not available",
            "read-only goal",
            "answer",
        ] {
            assert!(got.contains(needle), "{profile:?}: {needle}");
        }
        assert!(got.is_ascii());
    }
}
