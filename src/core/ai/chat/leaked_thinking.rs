//! Reasoning a local model leaked into its answer text. After a tool round,
//! `qwen3` on Ollama sometimes writes the rest of its reasoning as answer
//! text, closed by a bare `</think>`. In a round after tools the answer is
//! held until that tag or the end of the round: what comes before the tag is
//! reasoning, what comes after is the answer.

/// The tag that closes leaked reasoning.
const CLOSE: &str = "</think>";
/// The tag that may open it.
const OPEN: &str = "<think>";

/// One piece of a round's answer text, sorted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Piece {
    Reasoning(String),
    Answer(String),
}

/// Sorts the answer text of one round.
#[derive(Debug)]
pub struct LeakedThinking {
    held: String,
    holding: bool,
    /// Past this many held bytes the text goes out as the answer.
    cap: usize,
}

impl LeakedThinking {
    /// `hold` for a round that follows tool results; any other round passes
    /// its text through as it comes.
    pub fn new(hold: bool, cap: usize) -> Self {
        Self {
            held: String::new(),
            holding: hold,
            cap,
        }
    }

    /// The pieces to send for the text `part`.
    pub fn push(&mut self, part: &str) -> Vec<Piece> {
        if !self.holding {
            return answer(part.to_owned());
        }
        self.held.push_str(part);
        if let Some(at) = self.held.find(CLOSE) {
            self.holding = false;
            let held = std::mem::take(&mut self.held);
            let before = held[..at].trim();
            let before = before.strip_prefix(OPEN).unwrap_or(before).trim();
            let after = held[at + CLOSE.len()..].trim_start();
            let mut pieces = Vec::new();
            if !before.is_empty() {
                pieces.push(Piece::Reasoning(before.to_owned()));
            }
            pieces.extend(answer(after.to_owned()));
            return pieces;
        }
        if self.held.len() > self.cap {
            return self.finish();
        }
        Vec::new()
    }

    /// The text still held, as the answer, at the end of the round.
    pub fn finish(&mut self) -> Vec<Piece> {
        self.holding = false;
        answer(std::mem::take(&mut self.held))
    }
}

fn answer(text: String) -> Vec<Piece> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Piece::Answer(text)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(split: &mut LeakedThinking, parts: &[&str]) -> Vec<Piece> {
        let mut out: Vec<Piece> = parts.iter().flat_map(|p| split.push(p)).collect();
        out.extend(split.finish());
        out
    }

    #[test]
    fn text_before_a_bare_close_tag_is_reasoning_even_split_across_parts() {
        let mut split = LeakedThinking::new(true, 1 << 20);
        assert_eq!(
            all(
                &mut split,
                &[
                    "But the path ",
                    "is fibonacci.py.\n</thi",
                    "nk>\n\nFile created."
                ]
            ),
            [
                Piece::Reasoning("But the path is fibonacci.py.".into()),
                Piece::Answer("File created.".into()),
            ]
        );
    }

    #[test]
    fn an_open_tag_is_dropped_and_text_after_the_split_streams() {
        let mut split = LeakedThinking::new(true, 1 << 20);
        assert_eq!(
            split.push("<think>plan</think>Done"),
            [
                Piece::Reasoning("plan".into()),
                Piece::Answer("Done".into())
            ]
        );
        assert_eq!(split.push(" more"), [Piece::Answer(" more".into())]);
    }

    #[test]
    fn a_round_with_no_tag_gives_its_text_at_the_end() {
        let mut split = LeakedThinking::new(true, 1 << 20);
        assert_eq!(split.push("File "), []);
        assert_eq!(split.push("created."), []);
        assert_eq!(split.finish(), [Piece::Answer("File created.".into())]);
    }

    #[test]
    fn a_round_that_does_not_hold_passes_text_through_untouched() {
        let mut split = LeakedThinking::new(false, 1 << 20);
        assert_eq!(
            split.push("a</think>b"),
            [Piece::Answer("a</think>b".into())]
        );
        assert_eq!(split.finish(), []);
    }

    #[test]
    fn held_text_past_the_cap_goes_out_as_the_answer() {
        let mut split = LeakedThinking::new(true, 4);
        assert_eq!(split.push("abc"), []);
        assert_eq!(split.push("de"), [Piece::Answer("abcde".into())]);
        assert_eq!(split.push("f"), [Piece::Answer("f".into())]);
    }
}
