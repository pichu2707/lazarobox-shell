//! An answer that stops short (#439). A small local model sometimes ends its
//! answer with what it will do next ("I'll create the test next"), or sends
//! an empty one, and makes no call, so the turn ends with the step undone.
//! The loop asks it once to do the step or to sum up.

/// The system line a pane gets when the model was reminded.
pub const NEXT_STEP_NOTICE: &str =
    "the model said what it would do next without a tool call; it was asked to do it";

/// The message that follows the answer in the reminder request.
pub const NEXT_STEP_REMINDER: &str = "Your answer says what you will do next, but you made no tool call, so nothing happened. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.";

/// The system line a pane gets when an empty answer was answered.
pub const EMPTY_NOTICE: &str = "the model sent an empty answer; it was asked to go on";

/// The message that follows an empty answer in the reminder request.
pub const EMPTY_REMINDER: &str = "Your answer was empty. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.";

/// The system line a pane gets when a "ready for" sign-off was answered.
pub const STOPPED_NOTICE: &str = "the model stopped before finishing; it was asked to go on";

/// The message that follows a "ready for" sign-off in the reminder request.
pub const STOPPED_REMINDER: &str = "You stopped before finishing. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.";

/// The system line and the reminder for an answer that stopped short: an
/// empty one, or one that announces a next step. `None` for anything else.
pub fn go_on(text: &str) -> Option<(&'static str, &'static str)> {
    if text.trim().is_empty() {
        Some((EMPTY_NOTICE, EMPTY_REMINDER))
    } else if announced_step(text) {
        Some((NEXT_STEP_NOTICE, NEXT_STEP_REMINDER))
    } else if signed_off(text) {
        Some((STOPPED_NOTICE, STOPPED_REMINDER))
    } else {
        None
    }
}

/// Words that announce an action the model is about to take, lowercased and
/// matched as whole words.
const STEPS: &[&str] = &[
    "i'll",
    "i will",
    "i need to",
    "i'm going to",
    "i am going to",
    "let me",
    "next i",
    "next step",
    "voy a",
    "vamos a",
    "ahora voy",
    "a continuación",
    "lo siguiente",
    "procedo a",
    "procederé",
];

/// A word after a step that makes it a courtesy, not an action: "I'll be
/// happy to help", "I'll wait for you", "I need to know", "let me explain".
const NOT_ACTIONS: &[&str] = &[
    "be",
    "help",
    "wait",
    "know",
    "summarize",
    "explain",
    "ayudar",
    "esperar",
    "explicar",
    "resumir",
    "dar",
];

/// Words that make the sentence an offer to the user, not a step left undone.
const OFFERS: &[&str] = &[
    "if you",
    "let me know",
    "do you want",
    "would you like",
    "si quieres",
    "si lo deseas",
    "si te parece",
];

/// Whether the last sentence of `text` announces an action the model is about
/// to take. A question, an offer or a courtesy to the user does not count.
pub fn announced_step(text: &str) -> bool {
    let text = text.trim_end();
    if text.is_empty() || text.ends_with('?') {
        return false;
    }
    let words = words(last_sentence(text));
    if OFFERS
        .iter()
        .any(|offer| words.contains(&format!(" {offer} ")))
    {
        return false;
    }
    STEPS.iter().any(|step| {
        let step = format!(" {step} ");
        words.match_indices(&step).any(|(at, _)| {
            let next = words[at + step.len()..].split(' ').next().unwrap_or("");
            !next.is_empty() && !NOT_ACTIONS.contains(&next)
        })
    })
}

/// Words of a sign-off that hands the next step to the user, lowercased and
/// matched as whole words: "Ready for next steps.", "Listo para el commit."
const SIGN_OFFS: &[&str] = &[
    "ready for",
    "ready to",
    "listo para",
    "lista para",
    "listos para",
];

/// Whether the last sentence of `text` is a "ready for" sign-off. A question
/// does not count.
fn signed_off(text: &str) -> bool {
    let text = text.trim_end();
    if text.ends_with('?') {
        return false;
    }
    let words = words(last_sentence(text));
    SIGN_OFFS
        .iter()
        .any(|sign_off| words.starts_with(&format!(" {sign_off} ")))
}

/// `text` lowercased, with punctuation as spaces and one space between words,
/// padded with a space on each side so a phrase matches whole words only.
fn words(text: &str) -> String {
    let text = text.to_lowercase().replace('\u{2019}', "'");
    let spaced: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    format!(
        " {} ",
        spaced.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

/// The text after the last sentence end: a `.`, `!` or `?` followed by
/// whitespace, or a line break.
fn last_sentence(text: &str) -> &str {
    let body = text.trim_end_matches(['.', '!']);
    let start = body
        .char_indices()
        .rev()
        .find(|&(i, c)| {
            c == '\n'
                || (matches!(c, '.' | '!' | '?')
                    && body[i + c.len_utf8()..].starts_with(char::is_whitespace))
        })
        .map_or(0, |(i, c)| i + c.len_utf8());
    &body[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_that_ends_announcing_its_next_action_is_found() {
        for text in [
            "The tests haven't been created yet. The `tests` directory exists but contains no test files. I'll need to create a `test_fibonacci.py` file next.",
            "Created fibonacci.py. Now I will write the test.",
            "Next, I run pytest to check it.",
            "Let me create the test file.",
            "I’m going to commit the change.",
            "He creado el archivo. Ahora voy a crear el test.",
            "A continuación ejecuto pytest.",
            "Listo el script; lo siguiente es hacer el commit.",
            "I am going to create the test now.",
            "Next step: create test_fibonacci.py.",
            "Vamos a crear el test.",
            "I'll check whether it works if your env is set.",
        ] {
            assert!(announced_step(text), "{text}");
        }
    }

    #[test]
    fn a_summary_a_question_or_an_offer_is_not_an_announcement() {
        for text in [
            "✅ fibonacci.py has been successfully created in the worktree. Ready for next steps!",
            "Done: fibonacci.py and test_fibonacci.py are written and pytest passed.",
            "Shall I commit it?",
            "I'll commit it if you want. Do you want me to?",
            "If you want, I'll add more tests.",
            "Let me know if you need anything else.",
            "¿Quieres que haga el commit?",
            "Si quieres, voy a añadir más tests.",
            "",
            // #439 verifier: summaries and courtesies that announce nothing.
            "Now I have created fibonacci.py and the tests pass.",
            "I hope this helps! I'll be happy to help with anything else.",
            "Feel free to ask and I'll help.",
            "Shall I commit? I'll wait for your confirmation before committing.",
            "I need to know which Python version you use.",
            "Both files are written and pytest passed. Let me summarize: 2 files.",
            "Te voy a dar un ejemplo.",
        ] {
            assert!(!announced_step(text), "{text}");
        }
    }

    // #439: an empty answer gets its own words; a step announced gets the
    // next-step ones; anything else nothing.
    #[test]
    fn go_on_picks_the_reminder_for_an_empty_or_announcing_answer() {
        for empty in ["", "  \n\t"] {
            assert_eq!(
                go_on(empty),
                Some((EMPTY_NOTICE, EMPTY_REMINDER)),
                "{empty:?}"
            );
        }
        assert_eq!(
            go_on("I'll create the test next."),
            Some((NEXT_STEP_NOTICE, NEXT_STEP_REMINDER))
        );
        assert_eq!(go_on("Done: the test passed."), None);
    }

    // #439: qwen3:4b ends a turn it has not finished with "Ready for next
    // steps." or "Ready for commit."; a probe showed a reminder there makes it
    // call the next tool (3 of 3).
    #[test]
    fn a_ready_sign_off_stopped_before_finishing() {
        for text in [
            "test_fibonacci.py created and staged. Ready for next steps.",
            "✅ fibonacci.py has been successfully created in the worktree. Ready for next steps!",
            "Changes applied: `fibonacci.py` (+9). Ready for commit.",
            "Ready to commit.",
            "Archivo creado. Listo para el commit.",
        ] {
            assert_eq!(
                go_on(text),
                Some((STOPPED_NOTICE, STOPPED_REMINDER)),
                "{text}"
            );
        }
        for text in [
            "Ready for the commit?",
            "The ready flag is set in config.py.",
            "Done: committed as abc123.",
        ] {
            assert_eq!(go_on(text), None, "{text}");
        }
    }

    // Only the end of the answer counts: an early "I'll" followed by the
    // work's result is not a step left undone.
    #[test]
    fn an_announcement_early_in_a_long_answer_does_not_count() {
        let text = format!(
            "I'll look at the folder first.\n\n{}\n\nAll three files are in place and the test passed.",
            "Details of the listing. ".repeat(20)
        );
        assert!(!announced_step(&text));
    }
}
