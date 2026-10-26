//! The `Hugging Face` row of the AI tab (config-menu, Hugging Face account
//! row): connect an account by pasting a token, see whose it is, disconnect
//! it. Pure: a request leaves as an [`AccountAsk`] and its answer comes back
//! through [`Account::reply`]. The pasted token is drawn as one `•` per
//! character and dropped once its check is done. Nothing here writes a
//! config edit.

use crate::core::ai::hf::{AccountAsk, AccountReply, REJECTED, Secret, TokenFacts};
use crate::core::ai::picker::shown;

/// The title over the token field.
pub const TOKEN_TITLE: &str = "Hugging Face account";
/// The hint under the token field.
pub const TOKEN_HINT: &str = "paste a Hugging Face token";
/// The footer of the token field.
pub const HINT_TOKEN: &str = "Enter connect · Ctrl+U clear · Esc back";
/// The footer of the row, by what Enter does there.
pub const HINT_CONNECT: &str = "Enter connect · j/k move · s save · Esc cancel";
pub const HINT_CHECK: &str = "Enter check · j/k move · s save · Esc cancel";
pub const HINT_DISCONNECT: &str = "Enter disconnect · j/k move · s save · Esc cancel";

/// Longest token taken, in characters.
const MAX_TOKEN: usize = 256;

/// The request in flight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Asking {
    Connect,
    Check,
    Disconnect,
}

/// The account the row shows and the views it opens.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Account {
    facts: TokenFacts,
    /// The account of the saved token, once known in this session.
    name: Option<String>,
    /// The token field, while it is open.
    field: Option<Secret>,
    asking: Option<Asking>,
    /// `disconnect <name>? (y/n)` waits for an answer.
    prompt: bool,
}

impl Account {
    /// The row as the menu opens: what the runtime found, and the account
    /// known from earlier in this session (kept only while a token is
    /// saved).
    pub fn new(facts: TokenFacts, name: Option<String>) -> Self {
        Account {
            facts,
            name: name.filter(|_| facts.saved),
            ..Account::default()
        }
    }

    /// The value of the row.
    pub fn value(&self) -> String {
        if self.facts.env {
            return "token set (HF_TOKEN)".to_owned();
        }
        match &self.name {
            Some(name) => shown(name),
            None if self.facts.saved => "token saved".to_owned(),
            None => "not connected".to_owned(),
        }
    }

    /// The token field as drawn: one `•` per character. `None` while it is
    /// closed.
    pub fn field(&self) -> Option<String> {
        self.field.as_ref().map(|token| "•".repeat(token.chars()))
    }

    /// Whether a request runs.
    pub fn asking(&self) -> bool {
        self.asking.is_some()
    }

    /// The disconnect prompt, while it waits for `y` or `n`.
    pub fn prompt(&self) -> Option<String> {
        let name = self.name.as_deref().filter(|_| self.prompt)?;
        Some(format!("disconnect {}? (y/n)", shown(name)))
    }

    /// The footer of the row (the field and the prompt have their own).
    pub fn hint(&self) -> &'static str {
        if self.name.is_some() {
            HINT_DISCONNECT
        } else if self.facts.saved {
            HINT_CHECK
        } else {
            HINT_CONNECT
        }
    }

    /// Enter on the row: the disconnect prompt for a known account, a check
    /// of the saved token, else the token field.
    pub fn enter(&mut self) -> Option<AccountAsk> {
        if self.asking.is_some() {
            return None;
        }
        if self.name.is_some() {
            self.prompt = true;
            None
        } else if self.facts.saved {
            self.asking = Some(Asking::Check);
            Some(AccountAsk::Check)
        } else {
            self.field = Some(Secret::default());
            None
        }
    }

    /// The answer to the disconnect prompt: only `y` disconnects.
    pub fn answer(&mut self, yes: bool) -> Option<AccountAsk> {
        let asked = std::mem::take(&mut self.prompt);
        if asked && yes && self.asking.is_none() {
            self.asking = Some(Asking::Disconnect);
            return Some(AccountAsk::Disconnect);
        }
        None
    }

    /// Types one character into the field; ignored while asking. A token
    /// is printable ASCII.
    pub fn type_char(&mut self, c: char) {
        if self.asking.is_some() {
            return;
        }
        if let Some(token) = &mut self.field
            && c.is_ascii_graphic()
            && token.chars() < MAX_TOKEN
        {
            token.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.asking.is_none()
            && let Some(token) = &mut self.field
        {
            token.pop();
        }
    }

    /// Ctrl+U: empties the field.
    pub fn clear(&mut self) {
        if self.asking.is_none()
            && let Some(token) = &mut self.field
        {
            token.clear();
        }
    }

    /// A paste into the field: its first line.
    pub fn paste(&mut self, text: &str) {
        let first_line = text.split(['\n', '\r']).next().unwrap_or("");
        for c in first_line.chars() {
            self.type_char(c);
        }
    }

    /// Enter in the field: check the token. Empty text does nothing.
    pub fn submit(&mut self) -> Option<AccountAsk> {
        if self.asking.is_some() {
            return None;
        }
        let token = self.field.as_ref()?.expose().trim();
        if token.is_empty() {
            return None;
        }
        let ask = AccountAsk::Connect(Secret::new(token.to_owned()));
        self.asking = Some(Asking::Connect);
        Some(ask)
    }

    /// Esc in the field: back to the row, the token dropped. A check in
    /// flight is no longer waited for.
    pub fn close(&mut self) {
        self.field = None;
        if self.asking == Some(Asking::Connect) {
            self.asking = None;
        }
    }

    /// What the runtime found. The account follows what really happened,
    /// also for a request nobody waits for any more; a failure comes back
    /// as the message to show only when it was waited for.
    pub fn reply(&mut self, reply: AccountReply) -> Option<String> {
        let waited = |asking: &mut Option<Asking>, kind: Asking| {
            let found = *asking == Some(kind);
            if found {
                *asking = None;
            }
            found
        };
        match reply {
            AccountReply::Connected(result) => {
                let waited = waited(&mut self.asking, Asking::Connect);
                match result {
                    Ok(name) => {
                        self.name = Some(name);
                        self.facts.saved = true;
                        if waited {
                            self.field = None;
                        }
                        None
                    }
                    Err(message) if waited => {
                        // The token is not kept once its check is done.
                        if let Some(token) = &mut self.field {
                            token.clear();
                        }
                        Some(message)
                    }
                    Err(_) => None,
                }
            }
            AccountReply::Checked(result) => {
                let waited = waited(&mut self.asking, Asking::Check);
                match result {
                    Ok(name) => {
                        self.name = Some(name);
                        None
                    }
                    Err(message) if waited => {
                        // A saved token the Hub refuses: paste another.
                        if message == REJECTED {
                            self.field = Some(Secret::default());
                        }
                        Some(message)
                    }
                    Err(_) => None,
                }
            }
            AccountReply::Disconnected(result) => {
                let waited = waited(&mut self.asking, Asking::Disconnect);
                match result {
                    Ok(()) => {
                        self.name = None;
                        self.facts.saved = false;
                        None
                    }
                    Err(message) if waited => Some(message),
                    Err(_) => None,
                }
            }
        }
    }
}
