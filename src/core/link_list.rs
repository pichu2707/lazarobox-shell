//! The list of the links a pane shows (`Ctrl+Space u`): the keyboard's way to
//! open or copy one. Pure: the app says what the keys do.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The key of each row, in order. `j`, `k` and `y` have their own job.
const KEYS: &str = "123456789abcdefghilmnopqrstuvwxz";

/// What a key asks of the app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ListAction {
    None,
    Close,
    Open(String),
    Copy(String),
}

/// The links, oldest first, and the selected row.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkList {
    urls: Vec<String>,
    selected: usize,
}

impl LinkList {
    /// The list of `urls`, in reading order: a link shown twice is listed
    /// where it was last seen, and when there are more than keys the newest
    /// stay. `None` with no links. The newest is selected.
    pub fn new(urls: Vec<String>) -> Option<Self> {
        let mut unique: Vec<String> = Vec::new();
        for url in urls {
            unique.retain(|seen| *seen != url);
            unique.push(url);
        }
        let extra = unique.len().saturating_sub(KEYS.chars().count());
        unique.drain(..extra);
        let selected = unique.len().checked_sub(1)?;
        Some(Self {
            urls: unique,
            selected,
        })
    }

    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The key that opens row `index`.
    pub fn key_of(index: usize) -> Option<char> {
        KEYS.chars().nth(index)
    }

    /// `held` is a key kept down: it only moves the selection, so the key
    /// that opened the list cannot pick a row.
    pub fn on_key(&mut self, key: &KeyEvent, held: bool) -> ListAction {
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            return ListAction::Close;
        }
        if !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) {
            return ListAction::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.step(true),
            KeyCode::Char('k') | KeyCode::Up => self.step(false),
            _ if held => ListAction::None,
            KeyCode::Esc => ListAction::Close,
            KeyCode::Enter => ListAction::Open(self.urls[self.selected].clone()),
            KeyCode::Char('y') => ListAction::Copy(self.urls[self.selected].clone()),
            KeyCode::Char(pressed) => KEYS
                .chars()
                .position(|key| key == pressed)
                .and_then(|index| self.urls.get(index))
                .map_or(ListAction::None, |url| ListAction::Open(url.clone())),
            _ => ListAction::None,
        }
    }

    fn step(&mut self, down: bool) -> ListAction {
        let last = self.urls.len() - 1;
        self.selected = match down {
            true => (self.selected + 1).min(last),
            false => self.selected.saturating_sub(1),
        };
        ListAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(urls: &[&str]) -> LinkList {
        LinkList::new(urls.iter().map(|u| (*u).to_owned()).collect()).unwrap()
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn open(url: &str) -> ListAction {
        ListAction::Open(url.to_owned())
    }

    #[test]
    fn no_links_is_no_list() {
        assert_eq!(LinkList::new(Vec::new()), None);
    }

    #[test]
    fn the_newest_link_is_selected() {
        assert_eq!(list(&["a", "b", "c"]).selected(), 2);
    }

    #[test]
    fn a_link_shown_twice_is_listed_where_it_was_last_seen() {
        assert_eq!(list(&["a", "b", "a"]).urls(), ["b", "a"]);
    }

    #[test]
    fn with_more_links_than_keys_the_newest_stay() {
        let urls: Vec<String> = (0..40).map(|n| n.to_string()).collect();
        let list = LinkList::new(urls).unwrap();
        assert_eq!(list.urls().len(), KEYS.chars().count());
        assert_eq!(list.urls().last().unwrap(), "39");
    }

    #[test]
    fn a_rows_key_opens_it() {
        let mut l = list(&["a", "b"]);
        assert_eq!(l.on_key(&press(KeyCode::Char('1')), false), open("a"));
        assert_eq!(l.on_key(&press(KeyCode::Char('2')), false), open("b"));
    }

    #[test]
    fn after_the_digits_come_the_letters() {
        let urls: Vec<String> = (0..12).map(|n| n.to_string()).collect();
        let mut l = LinkList::new(urls).unwrap();
        assert_eq!(l.on_key(&press(KeyCode::Char('a')), false), open("9"));
    }

    #[test]
    fn a_key_with_no_row_does_nothing() {
        let mut l = list(&["a"]);
        assert_eq!(
            l.on_key(&press(KeyCode::Char('2')), false),
            ListAction::None
        );
    }

    #[test]
    fn the_keys_of_the_list_are_not_row_keys() {
        for key in ['j', 'k', 'y'] {
            assert!(!KEYS.contains(key), "{key}");
        }
    }

    #[test]
    fn enter_opens_and_y_copies_the_selected_row() {
        let mut l = list(&["a", "b"]);
        l.on_key(&press(KeyCode::Char('k')), false);
        assert_eq!(l.on_key(&press(KeyCode::Enter), false), open("a"));
        assert_eq!(
            l.on_key(&press(KeyCode::Char('y')), false),
            ListAction::Copy("a".to_owned())
        );
    }

    #[test]
    fn the_selection_stops_at_the_ends() {
        let mut l = list(&["a", "b"]);
        l.on_key(&press(KeyCode::Down), false);
        assert_eq!(l.selected(), 1);
        l.on_key(&press(KeyCode::Up), false);
        l.on_key(&press(KeyCode::Up), false);
        assert_eq!(l.selected(), 0);
    }

    #[test]
    fn esc_and_ctrl_c_close() {
        let mut l = list(&["a"]);
        assert_eq!(l.on_key(&press(KeyCode::Esc), false), ListAction::Close);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(l.on_key(&ctrl_c, false), ListAction::Close);
    }

    // The `u` that opened the list is still down.
    #[test]
    fn a_held_key_picks_no_row() {
        let urls: Vec<String> = (0..32).map(|n| n.to_string()).collect();
        let mut l = LinkList::new(urls).unwrap();
        assert_eq!(l.on_key(&press(KeyCode::Char('u')), true), ListAction::None);
    }

    #[test]
    fn a_held_key_still_moves_the_selection() {
        let mut l = list(&["a", "b"]);
        l.on_key(&press(KeyCode::Char('k')), true);
        assert_eq!(l.selected(), 0);
    }
}
