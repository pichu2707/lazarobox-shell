//! A next step announced in prose (#439). A small local model sometimes ends
//! its answer with what it will do next ("I'll create the test next") and
//! makes no call, so the turn ends with the step undone. The loop asks it
//! once to do the step or to sum up.

/// The system line a pane gets when the model was reminded.
pub const NEXT_STEP_NOTICE: &str =
    "the model said what it would do next without a tool call; it was asked to do it";

/// The message that follows the answer in the reminder request.
pub const NEXT_STEP_REMINDER: &str = "Your answer says what you will do next, but you made no tool call, so nothing happened. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.";

/// Words that announce an action the model is about to take, lowercased.
const STEPS: &[&str] = &[
    "i'll ",
    "i will ",
    "i need to ",
    "i'm going to ",
    "let me ",
    "next, i ",
    "now i ",
    "voy a ",
    "ahora voy",
    "a continuación",
    "lo siguiente",
    "procedo a ",
    "procederé",
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
/// to take. A question or an offer to the user does not count.
pub fn announced_step(text: &str) -> bool {
    let text = text.trim_end();
    if text.is_empty() || text.ends_with('?') {
        return false;
    }
    let last = last_sentence(text).to_lowercase().replace('\u{2019}', "'");
    !OFFERS.iter().any(|offer| last.contains(offer)) && STEPS.iter().any(|step| last.contains(step))
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
        ] {
            assert!(!announced_step(text), "{text}");
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
