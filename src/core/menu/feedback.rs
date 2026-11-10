//! The `Feedback` form of the menu (config-menu, Feedback): a bug report or
//! an idea, a title and a description, with the environment attached. Pure:
//! Ctrl+S builds the issue for the runtime to send with the user's own `gh`
//! (open-links S4), shaped like the repository's issue forms (S6); Ctrl+O
//! builds the prefilled GitHub new-issue link for the app to open in the
//! browser. Without `gh` the same link is the fallback of Ctrl+S (S5).
//!
//! The title and the description go through the redaction pass before any
//! issue or link is made; a change stays in the form, so the user sees what
//! was masked before pressing Ctrl+S again.
//!
//! Ctrl+S first asks for the open issues like the new one (issue
//! duplicates): when GitHub has some, the form lists them, Enter adds the
//! report to the chosen one as a comment, and Ctrl+S again creates the issue
//! anyway. With none, the runtime creates the issue at once.

use crate::core::desktop::is_wsl;
use std::borrow::Cow;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::ai::chat::composer::Composer;
use crate::core::ai::memory::{redact::redact, route::percent_encode};
use crate::core::ai::{Detection, Status};

/// The most characters of a title.
pub const TITLE_CAP: usize = 120;
/// The most characters of a description.
pub const DESCRIPTION_CAP: usize = 2000;
/// The longest link made: GitHub refuses much longer new-issue URLs.
pub const MAX_LINK: usize = 8000;
/// The most characters of an issue body GitHub takes.
pub const MAX_BODY: usize = 65536;

pub const TITLE_REQUIRED: &str = "Title is required";
pub const DESCRIPTION_REQUIRED: &str = "Description is required";
pub const REDACTED: &str = "Secrets were masked; review and press Ctrl+S again";
pub const TOO_LONG: &str = "Too long for a link: shorten the description";
pub const TOO_LONG_BODY: &str = "Too long for GitHub: shorten the description";
/// Without `gh`, a form too long for the fallback link.
pub const NO_GH_TOO_LONG: &str = "No gh, and too long for a link: shorten it";

/// The footer, by the focused field; each fits the 48 columns of the
/// narrow popup and says what Esc does. Ctrl+O works on every field; only
/// the text fields have room to name it.
pub const HINT_KIND: &str = "←/→ kind · Tab next · Ctrl+S send · Esc back";
pub const HINT_TEXT: &str = "Tab next · Ctrl+S send · Ctrl+O open · Esc back";
pub const HINT_ATTACH: &str = "Space attach · Tab next · Ctrl+S send · Esc back";
/// The footer while the issue is being sent; every key waits for the
/// answer.
pub const SENDING: &str = "Sending to GitHub…";
/// The footer while Ctrl+S looks for similar issues. With none, the same
/// request goes on to send the issue, so the text says both.
pub const SEARCHING: &str = "Looking for similar issues, then sending…";
/// The footer once the link is copied or opened.
pub const HINT_DONE: &str = "Esc back";
/// The footer once the issue is sent, by whether the host has an opener.
pub const HINT_SENT: &str = "Ctrl+O open · Esc back";
pub const HINT_SENT_COPY: &str = "Ctrl+O copy link · Esc back";
/// The footer columns a message of `gh` is cut to.
const FOOTER_WIDTH: usize = 48;

/// What the form says once the link is opened in the browser.
pub const OPENED: [&str; 2] = [
    "Opened in your browser. GitHub shows the issue",
    "form filled in, for you to review and send.",
];

/// What the form says once the link is copied.
pub const COPIED: [&str; 3] = [
    "Link copied. Paste it in your browser:",
    "GitHub opens the issue form filled in, for you",
    "to review and send.",
];

/// What the form says when `gh` is missing or not logged in and the
/// prefilled page was opened instead.
pub const NO_GH_OPENED: [&str; 3] = [
    "gh is not installed or not logged in:",
    "opened the form in your browser, for you to",
    "review and send.",
];

/// What the form says when `gh` is missing or not logged in and the
/// prefilled link was copied instead (the host has no opener).
pub const NO_GH_COPIED: [&str; 3] = [
    "gh is not installed or not logged in: link",
    "copied. Paste it in your browser to review",
    "and send the issue.",
];

/// The view once the issue is created: its link has a row of its own.
pub const SENT: &str = "Sent:";
/// The view once the report is added to an existing issue.
pub const COMMENTED: &str = "Added to the existing issue:";

/// The most similar issues listed.
pub const SIMILAR_CAP: usize = 5;
/// The view when GitHub has open issues like the new one.
pub const SIMILAR: &str = "Open issues like this one on GitHub:";
pub const SIMILAR_ADD: &str = "Enter adds your report to the selected one.";
pub const SIMILAR_NEW: &str = "Ctrl+S creates a new issue anyway.";
/// The footer of the similar issues.
pub const HINT_SIMILAR: &str = "↑/↓ pick · Enter add · Ctrl+S new · Esc back";

/// The most words of the title a search asks for: GitHub takes at most five
/// `OR` in one query.
const SEARCH_WORDS: usize = 6;
/// The words of a title that say nothing about the problem.
const STOP_WORDS: [&str; 44] = [
    "the", "and", "for", "with", "when", "from", "that", "this", "not", "are", "was", "but", "its",
    "into", "does", "can", "all", "any", "has", "have", "how", "why", "what", "after", "una",
    "uno", "los", "las", "del", "que", "con", "por", "para", "cuando", "como", "pero", "sin",
    "más", "mas", "hay", "este", "esta", "eso", "no",
];

/// The columns the form wraps its text to: the narrow popup's 48, less the
/// indent.
const TEXT_WIDTH: usize = 46;
/// The rows of the description shown; it scrolls with the cursor.
const DESCRIPTION_ROWS: usize = 6;

/// Where new issues are opened.
const NEW_ISSUE: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new");
/// The repository's web address, `https://github.com/<owner>/<repo>`.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
/// Where every GitHub link starts.
const GITHUB: &str = "https://github.com/";

/// The `<owner>/<repo>` issues are sent to.
fn repo() -> &'static str {
    REPOSITORY
        .strip_prefix(GITHUB)
        .unwrap_or(REPOSITORY)
        .trim_end_matches('/')
}

/// What is being sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Bug,
    Idea,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Bug => "Bug report",
            Kind::Idea => "Idea or goal",
        }
    }

    /// The issue form it opens, and the id of the form field the
    /// description fills.
    fn form(self) -> (&'static str, &'static str) {
        match self {
            Kind::Bug => ("bug.yml", "what-happened"),
            Kind::Idea => ("idea.yml", "idea"),
        }
    }

    /// The label of the form field the description fills, as the issue
    /// form titles its section, and the issue label asked for.
    fn section(self) -> (&'static str, &'static str) {
        match self {
            Kind::Bug => ("What happened", "bug"),
            Kind::Idea => ("The idea", "idea"),
        }
    }
}

/// The issue Ctrl+S sends: `gh issue create --repo <repo> --title <title>
/// --body-file -` with the body on stdin, and one `--label` per label.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Issue {
    /// `<owner>/<repo>`.
    pub repo: String,
    pub title: String,
    pub body: String,
    /// Asked for; GitHub keeps them only for users with write access.
    pub labels: Vec<String>,
}

/// One open issue GitHub found like the new one. Its title is GitHub's
/// text, untrusted: the form cleans it before showing it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Similar {
    pub number: u64,
    pub title: String,
}

/// What Ctrl+S asks the runtime to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Request {
    /// Look for open issues like `0` (see [`search_query`] and
    /// [`similar`]); with none, create it.
    Find(Issue),
    /// Create the issue, over the similar ones the user saw.
    Create(Issue),
    /// Add the report to the open issue `number`, as a comment (see
    /// [`comment_body`]).
    Comment { issue: Issue, number: u64 },
}

impl Request {
    pub fn issue(&self) -> &Issue {
        match self {
            Request::Find(issue) | Request::Create(issue) | Request::Comment { issue, .. } => issue,
        }
    }
}

/// What became of a send, from the runtime.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SendReply {
    /// Created: the issue's link.
    Sent(String),
    /// Added to an existing issue: the comment's link.
    Commented(String),
    /// Open issues like the new one, nothing sent yet.
    Similar(Vec<Similar>),
    /// `gh` failed: its message, raw (the form sanitizes it).
    Failed(String),
    /// `gh` is not installed or not logged in.
    NoGh,
}

/// `text` from GitHub or `gh` as plain words: CSI sequences (after
/// `ESC [` or `U+009B`) and bidi format characters dropped, other control
/// characters turned into blanks, blanks folded.
fn plain_words(text: &str) -> String {
    let mut plain = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        let csi = c == '\u{9b}' || (c == '\u{1b}' && chars.clone().next() == Some('['));
        if csi {
            if c == '\u{1b}' {
                chars.next();
            }
            // A CSI sequence ends at its final byte, `@` to `~`.
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
            continue;
        }
        if c == '\u{1b}' || is_bidi_format(c) {
            continue;
        }
        plain.push(if c.is_control() { ' ' } else { c });
    }
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The characters that reorder text on screen: marks, embeddings,
/// overrides and isolates.
fn is_bidi_format(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// `message` of `gh` as one footer line: see [`plain_words`], cut to the
/// footer width.
pub fn gh_message(message: &str) -> String {
    let words = plain_words(message);
    if words.is_empty() {
        return "gh failed".to_owned();
    }
    let line = format!("gh: {words}");
    if line.chars().count() <= FOOTER_WIDTH {
        return line;
    }
    let mut cut: String = line.chars().take(FOOTER_WIDTH - 1).collect();
    cut.push('…');
    cut
}

/// The created issue's link from what `gh` printed: exactly one line,
/// `https://github.com/<repo>/issues/<number>`; anything else is `None`.
pub fn issue_url(stdout: &str, repo: &str) -> Option<String> {
    let line = stdout.strip_suffix('\n').unwrap_or(stdout);
    let rest = line.strip_prefix(GITHUB)?;
    let (owner_repo, number) = rest.rsplit_once("/issues/")?;
    let ok = owner_repo.eq_ignore_ascii_case(repo)
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| line.to_owned())
}

/// The words of a title a search keys on: letters and digits only (so no
/// search qualifier gets through), lowercase, three characters or more, no
/// [`STOP_WORDS`], each once, in order.
fn key_words(title: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in title.split(|c: char| !c.is_alphanumeric()) {
        let word = word.to_lowercase();
        if word.chars().count() >= 3
            && !STOP_WORDS.contains(&word.as_str())
            && !words.contains(&word)
        {
            words.push(word);
        }
    }
    words
}

/// The `gh issue list --search` query for the issues like `title`: its first
/// [`SEARCH_WORDS`] key words joined by `OR`, in the titles and bodies.
/// `None` when the title has no key word to search for.
pub fn search_query(title: &str) -> Option<String> {
    let words = key_words(title);
    if words.is_empty() {
        return None;
    }
    let words: Vec<&str> = words
        .iter()
        .take(SEARCH_WORDS)
        .map(String::as_str)
        .collect();
    Some(format!("{} in:title,body", words.join(" OR ")))
}

/// Of the issues a search `found`, those like `title`: their titles share
/// two key words with it (one, when the title has only one), at most
/// [`SIMILAR_CAP`], in the search's order. An `OR` search finds many issues
/// that share one common word; they are not shown.
pub fn similar(title: &str, found: Vec<Similar>) -> Vec<Similar> {
    let words = key_words(title);
    let need = words.len().clamp(1, 2);
    found
        .into_iter()
        .filter(|issue| {
            let theirs = key_words(&issue.title);
            words.iter().filter(|w| theirs.contains(w)).count() >= need
        })
        .take(SIMILAR_CAP)
        .collect()
}

/// The comment that adds `issue` to an existing one: its title in bold, then
/// the body the issue would have had.
pub fn comment_body(issue: &Issue) -> String {
    format!("**{}**\n\n{}", issue.title, issue.body)
}

/// The comment's link from what `gh issue comment` printed: exactly one
/// line, `https://github.com/<repo>/issues/<number>#issuecomment-<id>`, on
/// the issue `number` the report was added to.
pub fn comment_url(stdout: &str, repo: &str, number: u64) -> Option<String> {
    let line = stdout.strip_suffix('\n').unwrap_or(stdout);
    let (issue, id) = line.split_once("#issuecomment-")?;
    let ok = issue_url(issue, repo).is_some()
        && issue.ends_with(&format!("/issues/{number}"))
        && !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| line.to_owned())
}

/// `text` as one row of the form: see [`plain_words`], cut to `width`
/// display columns with `…`. A double-width character that would cross the
/// edge is left out whole.
fn one_row(text: &str, width: usize) -> String {
    let words = plain_words(text);
    if words.width() <= width {
        return words;
    }
    let mut cut = String::new();
    let mut used = 1; // the `…`
    for c in words.chars() {
        used += c.width().unwrap_or(0);
        if used > width {
            break;
        }
        cut.push(c);
    }
    cut.push('…');
    cut
}

/// What lazarobox knows of where it runs. Never a path, a name or a key.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Facts {
    pub version: String,
    /// `std::env::consts::OS`.
    pub os: String,
    pub wsl: bool,
    /// The shell's program name.
    pub shell: Option<String>,
    /// The terminal, as `TERM_PROGRAM` names it.
    pub terminal: Option<String>,
    /// One `<engine> <status>` per detected engine; `None` before any
    /// detection.
    pub engines: Option<Vec<String>>,
}

impl Facts {
    /// The two lines attached to the issue.
    pub fn text(&self) -> String {
        let mut first = vec![format!("lazarobox {}", self.version)];
        first.push(if self.wsl {
            format!("{} (WSL)", self.os)
        } else {
            self.os.clone()
        });
        first.extend(self.shell.iter().chain(&self.terminal).cloned());
        let engines = match &self.engines {
            Some(engines) if !engines.is_empty() => engines.join(" · "),
            _ => "not detected".to_owned(),
        };
        format!("{}\nengines: {engines}", first.join(" · "))
    }
}

/// The detected engines, as attached: `<id> [<version>] <status>`. An
/// engine that is not installed is left out.
pub fn engines(detection: &Detection) -> Vec<String> {
    detection
        .engines
        .iter()
        .filter_map(|engine| {
            let status = match engine.status {
                Status::Available => "available",
                Status::NotRunning => "not running",
                Status::NotInstalled => return None,
            };
            Some(match engine.version.as_deref().and_then(name) {
                Some(version) => format!("{} {version} {status}", engine.id),
                None => format!("{} {status}", engine.id),
            })
        })
        .collect()
}

/// The longest shell or terminal name attached.
const NAME_CAP: usize = 40;

/// The facts of this host, from `var` (the environment) and `osrelease`
/// (`/proc/sys/kernel/osrelease`, when there is one). The engines are added
/// by whoever knows them.
pub fn host(
    version: &str,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
    osrelease: Option<&str>,
) -> Facts {
    let wsl = is_wsl(var("WSL_DISTRO_NAME").as_deref(), osrelease);
    let shell = var("SHELL").and_then(|path| name(path.rsplit('/').next().unwrap_or("")));
    let terminal = var("TERM_PROGRAM")
        .and_then(|t| name(&t))
        .or_else(|| var("WT_SESSION").map(|_| "Windows Terminal".to_owned()));
    Facts {
        version: version.to_owned(),
        os: os.to_owned(),
        wsl,
        shell,
        terminal,
        engines: None,
    }
}

/// `text` as an attached name: printable ASCII only, cut to [`NAME_CAP`];
/// `None` when it is empty or holds anything else.
fn name(text: &str) -> Option<String> {
    let text = text.trim();
    let plain = !text.is_empty() && text.bytes().all(|b| b == b' ' || b.is_ascii_graphic());
    plain.then(|| text.chars().take(NAME_CAP).collect())
}

/// The field the keys act on, in Tab order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Kind,
    Title,
    Description,
    Attach,
}

/// What a key did, for the menu to act on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Consumed; only a redraw.
    Handled,
    /// Ctrl+S, or Enter on a similar issue: what to send; the form waits for
    /// the answer.
    Send(Request),
    /// Ctrl+O: the link to open in the browser.
    Open(String),
    /// Esc: back to the row.
    Close,
}

/// The form.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Feedback {
    facts: Facts,
    kind: Kind,
    title: Composer,
    description: Composer,
    attach: bool,
    field: Field,
    error: Option<Cow<'static, str>>,
    /// An issue is being sent: every key waits for the answer.
    sending: bool,
    /// What is being sent is a [`Request::Find`].
    searching: bool,
    /// The open issues like this one, and the one selected: shown instead of
    /// the fields until Esc, Enter or Ctrl+S.
    similar: Option<(Vec<Similar>, usize)>,
}

impl Feedback {
    pub fn new(facts: Facts) -> Self {
        Feedback {
            facts,
            kind: Kind::Bug,
            title: Composer::with_cap(TITLE_CAP),
            description: Composer::with_cap(DESCRIPTION_CAP),
            attach: true,
            field: Field::Kind,
            error: None,
            sending: false,
            searching: false,
            similar: None,
        }
    }

    /// The form opened with `text` already written for it: the first line is
    /// the title and the rest the description. The form needs both, so a text
    /// of one line fills both, and a first line too long for the title stays
    /// whole in the description.
    pub fn prefilled(facts: Facts, text: &str) -> Self {
        let mut form = Self::new(facts);
        let text = text.trim();
        let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
        let (first, rest) = (first.trim(), rest.trim());
        form.title.paste(first);
        let whole = rest.is_empty() || first.chars().count() > TITLE_CAP;
        form.description.paste(if whole { text } else { rest });
        form
    }

    /// The form opened with a draft of the model's: its kind and its own two
    /// fields, without the first-line rule of [`Self::prefilled`].
    pub fn drafted(facts: Facts, kind: Kind, title: &str, description: &str) -> Self {
        let mut form = Self::new(facts);
        form.kind = kind;
        form.title.paste(title);
        form.description.paste(description);
        form
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn field(&self) -> Field {
        self.field
    }

    pub fn attach(&self) -> bool {
        self.attach
    }

    pub fn facts(&self) -> &Facts {
        &self.facts
    }

    pub fn title(&self) -> String {
        self.title.text()
    }

    pub fn description(&self) -> String {
        self.description.text()
    }

    /// The lines of the form under its title: the focused field marked
    /// with `›`, and the cursor drawn as `▏` in the focused text field. With
    /// similar issues found, those instead, the selected one marked.
    pub fn lines(&self) -> Vec<String> {
        if let Some((issues, at)) = &self.similar {
            let mut lines = vec![SIMILAR.to_owned()];
            for (i, issue) in issues.iter().enumerate() {
                let mark = if i == *at { "› " } else { "  " };
                let row = format!("#{} {}", issue.number, issue.title);
                lines.push(format!("{mark}{}", one_row(&row, TEXT_WIDTH)));
            }
            lines.extend([
                String::new(),
                SIMILAR_ADD.to_owned(),
                SIMILAR_NEW.to_owned(),
            ]);
            return lines;
        }
        let mark = |field| if self.field == field { "› " } else { "  " };
        let mut lines = vec![format!("{}Kind: {}", mark(Field::Kind), self.kind.label())];
        lines.push(format!("{}Title", mark(Field::Title)));
        lines.extend(text_rows(&self.title, self.field == Field::Title, 1));
        lines.push(format!("{}Description", mark(Field::Description)));
        lines.extend(text_rows(
            &self.description,
            self.field == Field::Description,
            DESCRIPTION_ROWS,
        ));
        let check = if self.attach { "[x]" } else { "[ ]" };
        lines.push(format!(
            "{}{check} Attach the environment",
            mark(Field::Attach)
        ));
        if self.attach {
            for line in self.facts.text().lines() {
                lines.extend(pack(line).into_iter().map(|l| format!("      {l}")));
            }
        }
        lines
    }

    /// The footer for the focused field, or [`SEARCHING`] or [`SENDING`].
    pub fn hint(&self) -> &'static str {
        if self.sending {
            return if self.searching { SEARCHING } else { SENDING };
        }
        if self.similar.is_some() {
            return HINT_SIMILAR;
        }
        match self.field {
            Field::Kind => HINT_KIND,
            Field::Title | Field::Description => HINT_TEXT,
            Field::Attach => HINT_ATTACH,
        }
    }

    /// Why the last Ctrl+S or Ctrl+O made nothing, or why the send failed;
    /// cleared by the next edit.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether an issue is being sent.
    pub fn sending(&self) -> bool {
        self.sending
    }

    /// GitHub has open issues like this one (at most [`SIMILAR_CAP`] are
    /// kept): the form lists them, the first selected. None is the same as a
    /// failed search: the form is as before.
    pub fn found(&mut self, issues: Vec<Similar>) {
        self.sending = false;
        let issues: Vec<Similar> = issues.into_iter().take(SIMILAR_CAP).collect();
        self.similar = (!issues.is_empty()).then_some((issues, 0));
    }

    /// The open issues listed, if any.
    pub fn similar_issues(&self) -> Option<&[Similar]> {
        self.similar.as_ref().map(|(issues, _)| issues.as_slice())
    }

    /// The send failed: the form is kept as typed, with `gh`'s message
    /// (see [`gh_message`]) as its error; Ctrl+S sends again.
    pub fn failed(&mut self, message: &str) {
        self.sending = false;
        self.error = Some(Cow::Owned(gh_message(message)));
    }

    /// The send found no `gh`: the prefilled link to open instead, or
    /// `None` with the form kept and why.
    pub fn fallback(&mut self) -> Option<String> {
        self.sending = false;
        let link = self.link();
        if self.error.as_deref() == Some(TOO_LONG) {
            self.error = Some(Cow::Borrowed(NO_GH_TOO_LONG));
        }
        link
    }

    /// One key. `repeat` marks a held key. While sending, none does
    /// anything, Esc included: the answer comes within the runtime's
    /// timeout.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> Outcome {
        if self.sending {
            return Outcome::Handled;
        }
        if self.similar.is_some() {
            return self.on_similar_key(key, repeat);
        }
        let (code, mods) = (key.code, key.modifiers);
        if code == KeyCode::Esc && mods == KeyModifiers::NONE {
            return if repeat {
                Outcome::Handled
            } else {
                Outcome::Close
            };
        }
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        match code {
            KeyCode::Char('s') if mods == KeyModifiers::CONTROL => {
                if !repeat {
                    return self.send(Request::Find);
                }
            }
            KeyCode::Char('o') if mods == KeyModifiers::CONTROL => {
                if !repeat {
                    return self.link().map_or(Outcome::Handled, Outcome::Open);
                }
            }
            KeyCode::Tab if plain => self.move_field(true),
            KeyCode::BackTab => self.move_field(false),
            KeyCode::Left | KeyCode::Right if plain && self.field == Field::Kind => {
                self.kind = match self.kind {
                    Kind::Bug => Kind::Idea,
                    Kind::Idea => Kind::Bug,
                };
            }
            KeyCode::Enter | KeyCode::Char(' ') if plain && self.field == Field::Attach => {
                if !repeat {
                    self.attach = !self.attach;
                }
            }
            KeyCode::Enter if mods == KeyModifiers::NONE => match self.field {
                Field::Title => self.field = Field::Description,
                Field::Description => self.edit(|c| c.newline()),
                Field::Kind | Field::Attach => {}
            },
            KeyCode::Char(ch) if plain => self.edit(|c| c.insert(ch)),
            KeyCode::Backspace if plain => self.edit(|c| c.backspace()),
            KeyCode::Delete if plain => self.edit(|c| c.delete()),
            KeyCode::Left if plain => self.edit(|c| c.left()),
            KeyCode::Right if plain => self.edit(|c| c.right()),
            KeyCode::Home if plain => self.edit(|c| c.home()),
            KeyCode::End if plain => self.edit(|c| c.end()),
            KeyCode::Up if plain && self.field == Field::Description => self.edit(|c| c.up()),
            KeyCode::Down if plain && self.field == Field::Description => self.edit(|c| c.down()),
            _ => {}
        }
        Outcome::Handled
    }

    /// A key while the similar issues are listed: ↑/↓ select, Enter adds
    /// the report to the selected one, Ctrl+S creates the issue anyway, Esc
    /// goes back to the fields as they were.
    fn on_similar_key(&mut self, key: &KeyEvent, repeat: bool) -> Outcome {
        let Some((issues, at)) = &mut self.similar else {
            return Outcome::Handled;
        };
        match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => *at = at.saturating_sub(1),
            (KeyCode::Down, KeyModifiers::NONE) => *at = (*at + 1).min(issues.len() - 1),
            _ if repeat => {}
            (KeyCode::Esc, KeyModifiers::NONE) => self.similar = None,
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let number = issues[*at].number;
                return self.send(|issue| Request::Comment { issue, number });
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => return self.send(Request::Create),
            _ => {}
        }
        Outcome::Handled
    }

    /// The issue made into `request`, and the form sending; or nothing,
    /// with why, when the issue cannot be made. A comment keeps the list: if
    /// it fails, the chosen issue is still selected and Enter tries it again.
    fn send(&mut self, request: impl FnOnce(Issue) -> Request) -> Outcome {
        let Some(issue) = self.issue() else {
            self.similar = None;
            return Outcome::Handled;
        };
        let request = request(issue);
        if !matches!(request, Request::Comment { .. }) {
            self.similar = None;
        }
        self.searching = matches!(request, Request::Find(_));
        self.sending = true;
        Outcome::Send(request)
    }

    /// A paste into the focused text field. The title is one line: a
    /// newline becomes a space.
    pub fn paste(&mut self, text: &str) {
        if self.sending || self.similar.is_some() {
            return;
        }
        match self.field {
            Field::Title => {
                let one_line = text.replace(['\r', '\n'], " ");
                self.edit(|c| c.paste(&one_line));
            }
            Field::Description => self.edit(|c| c.paste(text)),
            Field::Kind | Field::Attach => {}
        }
    }

    fn move_field(&mut self, forward: bool) {
        const ORDER: [Field; 4] = [Field::Kind, Field::Title, Field::Description, Field::Attach];
        let at = ORDER.iter().position(|f| *f == self.field).unwrap_or(0);
        let next = if forward {
            at + 1
        } else {
            at + ORDER.len() - 1
        };
        self.field = ORDER[next % ORDER.len()];
    }

    /// An edit of the focused text field; it clears the error.
    fn edit(&mut self, change: impl FnOnce(&mut Composer)) {
        let composer = match self.field {
            Field::Title => &mut self.title,
            Field::Description => &mut self.description,
            Field::Kind | Field::Attach => return,
        };
        change(composer);
        self.error = None;
    }

    /// Ctrl+S: the issue to send, or `None` with why there is none.
    fn issue(&mut self) -> Option<Issue> {
        let (title, description) = self.checked()?;
        let (section, label) = self.kind.section();
        let mut body = format!("### {section}\n\n{}", description.trim_end());
        if self.attach {
            body.push_str("\n\n### Environment\n\n```text\n");
            body.push_str(&self.facts.text());
            body.push_str("\n```");
        }
        if body.chars().count() > MAX_BODY {
            self.error = Some(Cow::Borrowed(TOO_LONG_BODY));
            return None;
        }
        Some(Issue {
            repo: repo().to_owned(),
            title: title.trim().to_owned(),
            body,
            labels: vec![label.to_owned()],
        })
    }

    /// Ctrl+O, or the fallback of Ctrl+S: the link, or `None` with why
    /// there is none.
    fn link(&mut self) -> Option<String> {
        let (title, description) = self.checked()?;
        let (form, field) = self.kind.form();
        let mut link = format!(
            "{NEW_ISSUE}?template={form}&title={}&{field}={}",
            percent_encode(title.trim()),
            percent_encode(description.trim_end())
        );
        if self.attach {
            link.push_str("&environment=");
            link.push_str(&percent_encode(&self.facts.text()));
        }
        if link.len() > MAX_LINK {
            self.error = Some(Cow::Borrowed(TOO_LONG));
            return None;
        }
        Some(link)
    }

    /// The title and the description, once both are filled and nothing in
    /// them had to be masked; else `None` with why. Clears the error.
    fn checked(&mut self) -> Option<(String, String)> {
        self.error = None;
        let (title, description) = (self.title(), self.description());
        let missing = if title.trim().is_empty() {
            Some((TITLE_REQUIRED, Field::Title))
        } else if description.trim().is_empty() {
            Some((DESCRIPTION_REQUIRED, Field::Description))
        } else {
            None
        };
        if let Some((error, field)) = missing {
            self.error = Some(Cow::Borrowed(error));
            self.field = field;
            return None;
        }
        let (safe_title, safe_description) = (redact(&title).text, redact(&description).text);
        if safe_title != title || safe_description != description {
            replace(&mut self.title, &safe_title);
            replace(&mut self.description, &safe_description);
            self.error = Some(Cow::Borrowed(REDACTED));
            return None;
        }
        Some((title, description))
    }
}

/// The columns a facts line gets under its indent, in the narrow popup.
const FACTS_WIDTH: usize = 42;

/// A facts line cut between its ` · ` items into lines of at most
/// [`FACTS_WIDTH`], so no fact is cut; a single longer item keeps a line of
/// its own.
fn pack(line: &str) -> Vec<String> {
    let mut packed: Vec<String> = Vec::new();
    for item in line.split(" · ") {
        match packed.last_mut() {
            Some(last) if last.chars().count() + 3 + item.chars().count() <= FACTS_WIDTH => {
                last.push_str(" · ");
                last.push_str(item);
            }
            _ => packed.push(item.to_owned()),
        }
    }
    packed
}

/// The rows of a text field, indented, with `▏` at the cursor when
/// `focused`.
fn text_rows(composer: &Composer, focused: bool, max_rows: usize) -> Vec<String> {
    let view = composer.view(TEXT_WIDTH, max_rows);
    let mut rows = if view.rows.is_empty() {
        vec![String::new()]
    } else {
        view.rows
    };
    if focused && let Some(row) = rows.get_mut(view.cursor_row) {
        let at = row
            .char_indices()
            .nth(view.cursor_col)
            .map_or(row.len(), |(i, _)| i);
        row.insert(at, '▏');
    }
    rows.into_iter().map(|row| format!("  {row}")).collect()
}

/// Puts `text` in `composer`, the cursor at its end.
fn replace(composer: &mut Composer, text: &str) {
    composer.clear();
    composer.paste(text);
}
