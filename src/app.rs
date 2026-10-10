use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::{
    copy::{CopyCommand, CopyState},
    keys::{encode_key, encode_paste},
    layout::{
        Axis, Direction, Node, PaneId, PaneIds, Rect, Removal, SplitError, Tiling, neighbour,
        pane_at, tile,
    },
    pane::{CursorKind, CursorShape, Pane, PaneSize},
    prefix::{self, Binding, Group, PREFIX_KEY, PREFIX_TREE, PrefixAction, Step},
    pty::{PtyEvent, SpawnSpec},
};

/// Rows of history kept by the emulator.
const SCROLLBACK_LINES: usize = 10_000;

/// Byte sent to the child when the prefix is pressed twice (NUL, Ctrl+Space).
const PREFIX_LITERAL: u8 = 0x00;

/// Where the user's keys go. Independent from `AppMode`, which is what is viewed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputMode {
    /// Keys are encoded and sent to the child.
    #[default]
    Terminal,
    /// The prefix was pressed; the next key picks an action.
    Prefix,
    /// Read-only scrollback navigation.
    Copy(CopyState),
    /// A group is open (WINDOW, TAB, GO, BUFFER); the next key picks its binding.
    Group(&'static Group),
    /// Pane resize mode; its keys arrive with the resize slice.
    Resize,
    /// Waiting for a yes/no answer.
    Confirm(Confirm),
}

/// What a pending confirmation asks about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confirm {
    /// Waiting for `y` to quit.
    Quit,
    /// Waiting for `y` to close the focused pane.
    ClosePane,
    /// Waiting for `y` to close the active tab.
    CloseTab,
}

impl InputMode {
    /// Text of the statusline mode block.
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "TERMINAL",
            Self::Prefix => "PREFIX",
            Self::Copy(_) => "COPY",
            Self::Group(group) => group.label,
            Self::Resize => "RESIZE",
            Self::Confirm(Confirm::Quit) => "Quit? (y/n)",
            Self::Confirm(Confirm::ClosePane) => "Close pane? (y/n)",
            Self::Confirm(Confirm::CloseTab) => "Close tab? (y/n)",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Paste(String),
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Something a pane's child did.
    Pty(PaneId, PtyEvent),
    /// A pane's child working directory, as last read by the runtime.
    Cwd(PaneId, PathBuf),
    /// Spawning the child of a pane failed; carries the error text.
    SpawnFailed(PaneId, String),
}

/// Side effects requested by `App::update`, executed by the runtime.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    WritePty(PaneId, Vec<u8>),
    ResizePty(PaneId, PaneSize),
    /// Start a child for a new pane.
    SpawnPane(PaneId, SpawnSpec),
    /// Terminate a pane's child without blocking the loop.
    ClosePane(PaneId),
    Quit,
}

/// Pane size for a terminal of `cols` x `rows`: the bottom row is the
/// statusline, and the pane is never smaller than 1x1.
fn pane_size(cols: u16, rows: u16) -> PaneSize {
    PaneSize {
        rows: rows.saturating_sub(1).max(1),
        cols: cols.max(1),
    }
}

/// Name shown in the statusline for the shell at `path` (`sh` if unknown).
pub fn shell_basename(path: Option<&str>) -> &str {
    path.and_then(|p| Path::new(p).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("sh")
}

/// Where the parts of the screen go, computed once from the terminal size so
/// the app and the UI cannot disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenLayout {
    /// The top row, present only with more than one tab.
    pub tab_bar: Option<Rect>,
    pub body: Rect,
    pub status: Rect,
}

impl ScreenLayout {
    fn new(cols: u16, rows: u16, bar: bool) -> Self {
        let size = pane_size(cols, rows);
        let bar_rows = u16::from(bar);
        Self {
            tab_bar: bar.then_some(Rect {
                x: 0,
                y: 0,
                width: size.cols,
                height: 1,
            }),
            body: Rect {
                x: 0,
                y: bar_rows,
                width: size.cols,
                height: size.rows.saturating_sub(bar_rows).max(1),
            },
            status: Rect {
                x: 0,
                y: rows.saturating_sub(1),
                width: size.cols,
                height: 1,
            },
        }
    }
}

/// A pane's emulator, the size its PTY was last told, and its last known cwd.
struct PaneState {
    emu: Pane,
    size: PaneSize,
    cwd: Option<PathBuf>,
}

impl PaneState {
    fn new(size: PaneSize) -> Self {
        Self {
            emu: Pane::new(size, SCROLLBACK_LINES),
            size,
            cwd: None,
        }
    }
}

/// The layout of the panes and which one has the focus.
struct Tab {
    tree: Node,
    focus: PaneId,
    /// The focused pane fills the whole body; the others keep running hidden.
    zoom: bool,
}

impl Tab {
    /// The geometry inside `area`: the whole tree, or only the focused pane
    /// when zoomed (no separators).
    fn tiling(&self, area: Rect) -> Tiling {
        if self.zoom {
            Tiling {
                panes: vec![(self.focus, area)],
                separators: Vec::new(),
            }
        } else {
            tile(&self.tree, area)
        }
    }
}

/// Owns the panes and the input state. Pure: it never touches IO.
pub struct App {
    /// Every pane of every tab; ids are global and never reused.
    panes: HashMap<PaneId, PaneState>,
    tabs: Vec<Tab>,
    active: usize,
    /// Host terminal size, kept to recompute the screen when the tab bar toggles.
    term: (u16, u16),
    ids: PaneIds,
    input: InputMode,
    /// Shown in the statusline path segment until the next key press.
    notice: Option<String>,
    dirty: bool,
    screen: ScreenLayout,
    /// Where the first pane starts, and the fallback for panes with no known cwd.
    launch_cwd: Option<PathBuf>,
    home: Option<PathBuf>,
    shell: Option<String>,
}

impl App {
    /// An app for a terminal of `cols` x `rows`, with one pane in the body.
    pub fn new(cols: u16, rows: u16) -> Self {
        let screen = ScreenLayout::new(cols, rows, false);
        let mut ids = PaneIds::default();
        let first = ids.alloc();
        Self {
            panes: HashMap::from([(first, PaneState::new(body_size(screen.body)))]),
            tabs: vec![Tab {
                tree: Node::Leaf(first),
                focus: first,
                zoom: false,
            }],
            active: 0,
            term: (cols, rows),
            ids,
            input: InputMode::default(),
            notice: None,
            dirty: true,
            screen,
            launch_cwd: None,
            home: None,
            shell: None,
        }
    }

    /// Sets the facts the statusline shows. They are inputs, not lookups, so
    /// `update` stays free of IO.
    pub fn with_env(mut self, shell: Option<&str>, home: Option<PathBuf>, cwd: PathBuf) -> Self {
        self.shell = shell.map(str::to_owned);
        self.home = home;
        self.focused_state_mut().cwd = Some(cwd.clone());
        self.launch_cwd = Some(cwd);
        self
    }

    /// The spawn-failure notice, if one is showing.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// What the statusline path segment shows, in order of precedence: the
    /// notice, the key hint while PREFIX or a group is pending (clipped to
    /// `max_cols` cells, so possibly empty), else the cwd.
    pub fn status_path(&self, max_cols: u16) -> String {
        if let Some(notice) = &self.notice {
            return notice.clone();
        }
        match self.input {
            InputMode::Prefix => prefix::hint(PREFIX_TREE, max_cols),
            InputMode::Group(group) => prefix::hint(group.bindings, max_cols),
            _ => self.cwd_label(),
        }
    }

    pub fn shell_name(&self) -> &str {
        shell_basename(self.shell.as_deref())
    }

    /// The pane to start before the first event, and how to start it.
    pub fn initial_spawn(&self) -> (PaneId, SpawnSpec) {
        (
            self.tab().focus,
            self.spawn_spec(self.launch_cwd.clone(), self.focused_size()),
        )
    }

    pub fn input(&self) -> InputMode {
        self.input
    }

    /// Whether a redraw is due. Reading it clears it.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn screen(&self) -> ScreenLayout {
        self.screen
    }

    /// Where every pane and separator of the tab goes inside the body.
    pub fn tiling(&self) -> Tiling {
        self.tab().tiling(self.screen.body)
    }

    /// Whether the focused pane is zoomed over the others.
    pub fn zoomed(&self) -> bool {
        self.tab().zoom
    }

    /// COPY viewport position `(rows above the live bottom, history rows)`;
    /// `None` outside COPY and when the pane has no history (alternate screen).
    pub fn copy_position(&self) -> Option<(usize, usize)> {
        let InputMode::Copy(state) = &self.input else {
            return None;
        };
        let total = self.panes[&self.tab().focus].emu.scrollback_len();
        (total > 0).then_some((state.offset, total))
    }

    /// How many tabs are open.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Index of the active tab.
    pub fn active_tab(&self) -> usize {
        self.active
    }

    fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn focused(&self) -> PaneId {
        self.tab().focus
    }

    /// The emulator of pane `id`, if it exists.
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id).map(|state| &state.emu)
    }

    pub fn focused_pane(&self) -> &Pane {
        &self.focused_state().emu
    }

    pub fn focused_size(&self) -> PaneSize {
        self.focused_state().size
    }

    // The focus always names a live pane: `split` focuses what it adds, and
    // `remove_pane` moves the focus off a pane before dropping it.
    fn focused_state(&self) -> &PaneState {
        &self.panes[&self.tab().focus]
    }

    fn focused_state_mut(&mut self) -> &mut PaneState {
        self.panes
            .get_mut(&self.tabs[self.active].focus)
            .expect("the focused pane exists")
    }

    fn spawn_spec(&self, cwd: Option<PathBuf>, size: PaneSize) -> SpawnSpec {
        let cwd = cwd.unwrap_or_else(|| "/".into());
        SpawnSpec::new(self.shell.as_deref(), cwd, size)
    }

    /// The child's cwd for the statusline, with `$HOME` shown as `~`. Empty
    /// until the first cwd is known.
    pub fn cwd_label(&self) -> String {
        let Some(cwd) = &self.focused_state().cwd else {
            return String::new();
        };
        let home = self.home.as_deref().filter(|home| home.parent().is_some());
        match home.and_then(|home| cwd.strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.display()),
            None => cwd.display().to_string(),
        }
    }

    pub fn update(&mut self, event: AppEvent) -> Vec<Effect> {
        match event {
            AppEvent::Key(key) => self.on_key(key),
            AppEvent::Paste(text) => self.on_paste(&text),
            AppEvent::Resize { cols, rows } => self.on_resize(cols, rows),
            // Events for panes that no longer exist are dropped.
            AppEvent::Pty(id, event) if self.panes.contains_key(&id) => self.on_pty(id, event),
            AppEvent::Cwd(id, cwd) if self.panes.contains_key(&id) => self.on_cwd(id, cwd),
            AppEvent::Pty(..) | AppEvent::Cwd(..) => Vec::new(),
            AppEvent::SpawnFailed(id, error) if self.panes.contains_key(&id) => {
                let effects = self.remove_pane(id);
                self.notice = Some(format!("spawn failed: {error}"));
                effects
            }
            AppEvent::SpawnFailed(..) => Vec::new(),
        }
    }

    /// Cursor shape the outer terminal should show: a steady block in COPY
    /// (the viewport may be away from the child's cursor), otherwise whatever
    /// the child last requested.
    pub fn cursor_shape(&self) -> CursorShape {
        match self.input {
            InputMode::Copy(_) => CursorShape {
                kind: CursorKind::Block,
                blinking: false,
            },
            _ => self.focused_pane().cursor_shape(),
        }
    }

    /// Release is always ignored. Repeat acts like Press where holding a key
    /// is meaningful (TERMINAL typing, COPY scrolling) and is dropped where a
    /// held key could trigger a one-shot decision (PREFIX, GROUP, CONFIRM).
    fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let repeat = match key.kind {
            KeyEventKind::Press => false,
            KeyEventKind::Repeat => true,
            KeyEventKind::Release => return Vec::new(),
        };
        self.dirty |= self.notice.take().is_some();
        if repeat
            && matches!(
                self.input,
                InputMode::Prefix | InputMode::Group(_) | InputMode::Confirm(_)
            )
        {
            return Vec::new();
        }
        self.dirty = true;
        match self.input {
            InputMode::Terminal => self.on_terminal_key(key),
            InputMode::Prefix => self.on_pending_key(PREFIX_TREE, &key),
            InputMode::Group(group) => self.on_pending_key(group.bindings, &key),
            InputMode::Copy(state) => self.on_copy_key(state, &key),
            InputMode::Resize => self.on_resize_key(&key),
            InputMode::Confirm(confirm) => self.on_confirm_key(confirm, &key),
        }
    }

    fn on_terminal_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if PREFIX_KEY.matches(&key) {
            self.input = InputMode::Prefix;
            return Vec::new();
        }
        encode_key(key, self.focused_pane().modes())
            .map(|bytes| vec![Effect::WritePty(self.tab().focus, bytes)])
            .unwrap_or_default()
    }

    /// Resolve a key against the pending table (the root or an open group).
    /// `ShowCommands` is a no-op until the command viewer lands.
    fn on_pending_key(&mut self, table: &'static [Binding], key: &KeyEvent) -> Vec<Effect> {
        let step = prefix::lookup(table, key);
        self.input = match step {
            Step::Enter(group) => InputMode::Group(group),
            Step::Run(PrefixAction::RequestQuit) => InputMode::Confirm(Confirm::Quit),
            Step::Run(PrefixAction::EnterCopy) => InputMode::Copy(CopyState::default()),
            Step::Run(PrefixAction::EnterResize) => InputMode::Resize,
            Step::Run(PrefixAction::ClosePane) if self.tabs.len() > 1 || self.tab_panes() > 1 => {
                InputMode::Confirm(Confirm::ClosePane)
            }
            Step::Run(PrefixAction::CloseTab) if self.tabs.len() > 1 => {
                InputMode::Confirm(Confirm::CloseTab)
            }
            // Closing the last pane of the only tab quits; so does closing the only tab.
            Step::Run(PrefixAction::ClosePane | PrefixAction::CloseTab) => {
                InputMode::Confirm(Confirm::Quit)
            }
            Step::Cancel
            | Step::Run(
                PrefixAction::SendPrefixLiteral
                | PrefixAction::SplitRight
                | PrefixAction::SplitBelow
                | PrefixAction::ToggleZoom
                | PrefixAction::Focus(_),
            ) => InputMode::Terminal,
            Step::Run(
                PrefixAction::ShowCommands
                | PrefixAction::NewTab
                | PrefixAction::NextTab
                | PrefixAction::PrevTab
                | PrefixAction::GotoTab(_),
            ) => InputMode::Terminal,
        };
        match step {
            Step::Run(PrefixAction::SendPrefixLiteral) => {
                vec![Effect::WritePty(self.tab().focus, vec![PREFIX_LITERAL])]
            }
            Step::Run(PrefixAction::SplitRight) => self.split(Axis::X),
            Step::Run(PrefixAction::SplitBelow) => self.split(Axis::Y),
            Step::Run(PrefixAction::Focus(dir)) => self.move_focus(dir),
            Step::Run(PrefixAction::EnterResize) => {
                self.tab_mut().zoom = false;
                self.relayout()
            }
            Step::Run(PrefixAction::ToggleZoom) => {
                let zoom = !self.tab().zoom;
                self.tab_mut().zoom = zoom;
                self.relayout()
            }
            Step::Run(PrefixAction::NewTab) => self.new_tab(),
            Step::Run(PrefixAction::NextTab) => {
                self.activate((self.active + 1) % self.tabs.len());
                Vec::new()
            }
            Step::Run(PrefixAction::PrevTab) => {
                self.activate((self.active + self.tabs.len() - 1) % self.tabs.len());
                Vec::new()
            }
            Step::Run(PrefixAction::GotoTab(n)) => {
                self.activate(usize::from(n).wrapping_sub(1));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// How many panes the active tab holds.
    fn tab_panes(&self) -> usize {
        self.tab().tree.leaves().len()
    }

    /// Makes tab `index` the active one; a missing tab or the active one is a
    /// no-op. Leaving a tab ends COPY (its viewport goes back to the bottom)
    /// and RESIZE.
    fn activate(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active {
            return;
        }
        if let Some(left) = self.panes.get_mut(&self.tabs[self.active].focus) {
            left.emu.set_scrollback(0);
        }
        if matches!(self.input, InputMode::Copy(_) | InputMode::Resize) {
            self.input = InputMode::Terminal;
        }
        self.active = index;
    }

    /// Appends a tab with one pane, activates it and spawns its shell in the
    /// focused pane's cwd. The tab bar may appear, which resizes every tab.
    fn new_tab(&mut self) -> Vec<Effect> {
        let cwd = self
            .focused_state()
            .cwd
            .clone()
            .or_else(|| self.launch_cwd.clone());
        let id = self.ids.alloc();
        self.tabs.push(Tab {
            tree: Node::Leaf(id),
            focus: id,
            zoom: false,
        });
        self.activate(self.tabs.len() - 1);
        self.refresh_screen();
        let size = body_size(self.screen.body);
        self.panes.insert(id, PaneState::new(size));
        let mut effects = self.relayout();
        effects.push(Effect::SpawnPane(id, self.spawn_spec(cwd, size)));
        effects
    }

    /// Drops tab `index` and its panes. The tab that takes its index becomes
    /// active (the last one if it was last); closing a tab before the active
    /// one keeps the active tab. The bar may disappear, which resizes every tab.
    fn drop_tab(&mut self, index: usize) -> Vec<Effect> {
        for id in self.tabs.remove(index).tree.leaves() {
            self.panes.remove(&id);
        }
        if index == self.active {
            self.active = index.min(self.tabs.len() - 1);
            if matches!(self.input, InputMode::Copy(_) | InputMode::Resize) {
                self.input = InputMode::Terminal;
            }
        } else if index < self.active {
            self.active -= 1;
        }
        self.refresh_screen();
        self.relayout()
    }

    /// Recomputes the screen rects: the tab bar exists while there is more
    /// than one tab.
    fn refresh_screen(&mut self) {
        self.screen = ScreenLayout::new(self.term.0, self.term.1, self.tabs.len() > 1);
    }

    /// Splits the focused pane and focuses the new one. A split the layout
    /// refuses (too small) changes nothing.
    fn split(&mut self, axis: Axis) -> Vec<Effect> {
        self.tab_mut().zoom = false;
        let target = self.tab().focus;
        let new = self.ids.alloc();
        let body = self.screen.body;
        match self.tab_mut().tree.split(body, target, new, axis) {
            Ok(()) => {}
            Err(SplitError::TooSmall | SplitError::NotFound) => return self.relayout(),
        }
        let cwd = self
            .focused_state()
            .cwd
            .clone()
            .or_else(|| self.launch_cwd.clone());
        let size = self.layout_sizes()[&new];
        self.panes.insert(new, PaneState::new(size));
        self.set_focus(new);
        let mut effects = self.relayout();
        effects.push(Effect::SpawnPane(new, self.spawn_spec(cwd, size)));
        effects
    }

    /// Moves the focus to the neighbour in `dir`. A zoomed tab is unzoomed
    /// first, even at an edge, so the other panes come back into view.
    fn move_focus(&mut self, dir: Direction) -> Vec<Effect> {
        self.tab_mut().zoom = false;
        let tiling = tile(&self.tab().tree, self.screen.body);
        if let Some(next) = neighbour(&tiling, self.tab().focus, dir) {
            self.set_focus(next);
        }
        self.relayout()
    }

    /// Focuses `next`. Leaving a pane ends COPY (its viewport goes back to the
    /// bottom) and RESIZE.
    fn set_focus(&mut self, next: PaneId) {
        self.set_focus_in(self.active, next);
    }

    /// Focuses `next` in tab `t`. Only a change in the active tab touches the
    /// input mode.
    fn set_focus_in(&mut self, t: usize, next: PaneId) {
        let left = self.tabs[t].focus;
        if next == left {
            return;
        }
        if let Some(left) = self.panes.get_mut(&left) {
            left.emu.set_scrollback(0);
        }
        if t == self.active && matches!(self.input, InputMode::Copy(_) | InputMode::Resize) {
            self.input = InputMode::Terminal;
        }
        self.tabs[t].focus = next;
    }

    /// Drops pane `id` of whichever tab holds it and gives its space to its
    /// sibling. If it had its tab's focus, the pane now covering its old
    /// top-left cell takes it. The last pane of a tab closes the tab; the last
    /// pane of the last tab quits. Any removal cancels a pending confirmation.
    fn remove_pane(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(t) = self
            .tabs
            .iter()
            .position(|tab| tab.tree.leaves().contains(&id))
        else {
            return Vec::new();
        };
        let body = self.screen.body;
        let tiling = tile(&self.tabs[t].tree, body);
        let Some(&(_, old)) = tiling.panes.iter().find(|(pane, _)| *pane == id) else {
            return Vec::new();
        };
        let effects = match self.tabs[t].tree.remove(id) {
            Removal::NotFound => return Vec::new(),
            Removal::WasLast if self.tabs.len() == 1 => return vec![Effect::Quit],
            Removal::WasLast => self.drop_tab(t),
            Removal::Removed => {
                self.panes.remove(&id);
                self.tabs[t].zoom = false;
                if id == self.tabs[t].focus {
                    let tiling = tile(&self.tabs[t].tree, body);
                    let next =
                        pane_at(&tiling, old.x, old.y).unwrap_or(self.tabs[t].tree.leaves()[0]);
                    self.set_focus_in(t, next);
                }
                self.relayout()
            }
        };
        self.dirty = true;
        if matches!(self.input, InputMode::Confirm(_)) {
            self.input = InputMode::Terminal;
        }
        effects
    }

    /// What each pane's PTY size should be, from the current layout.
    fn layout_sizes(&self) -> HashMap<PaneId, PaneSize> {
        self.tab()
            .tiling(self.screen.body)
            .panes
            .into_iter()
            .map(|(id, rect)| (id, body_size(rect)))
            .collect()
    }

    /// Brings every pane to its layout size, telling only the panes that
    /// changed. Panes new to the layout keep the size they were created with.
    fn relayout(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        for tab in &self.tabs {
            for (id, rect) in tab.tiling(self.screen.body).panes {
                let size = body_size(rect);
                let Some(state) = self.panes.get_mut(&id) else {
                    continue;
                };
                if state.size != size {
                    state.emu.resize(size);
                    state.size = size;
                    effects.push(Effect::ResizePty(id, size));
                }
            }
        }
        effects
    }

    /// RESIZE is sticky: `h/j/k/l` move the focused pane's border by one cell,
    /// Esc leaves, and every other key (Ctrl+Space included) is swallowed.
    fn on_resize_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            self.input = InputMode::Terminal;
            return Vec::new();
        }
        let dir = match (key.code, key.modifiers) {
            (KeyCode::Char('h'), KeyModifiers::NONE) => Direction::Left,
            (KeyCode::Char('j'), KeyModifiers::NONE) => Direction::Down,
            (KeyCode::Char('k'), KeyModifiers::NONE) => Direction::Up,
            (KeyCode::Char('l'), KeyModifiers::NONE) => Direction::Right,
            _ => return Vec::new(),
        };
        let (body, focus) = (self.screen.body, self.tab().focus);
        if self.tab_mut().tree.resize_step(body, focus, dir) {
            return self.relayout();
        }
        Vec::new()
    }

    fn on_confirm_key(&mut self, confirm: Confirm, key: &KeyEvent) -> Vec<Effect> {
        self.input = InputMode::Terminal;
        if key.code != KeyCode::Char('y') || key.modifiers != KeyModifiers::NONE {
            return Vec::new();
        }
        match confirm {
            Confirm::Quit => vec![Effect::Quit],
            Confirm::CloseTab => self.close_tab(),
            Confirm::ClosePane => {
                let id = self.tab().focus;
                let mut effects = self.remove_pane(id);
                effects.push(Effect::ClosePane(id));
                effects
            }
        }
    }

    /// Closes the active tab with all its panes; the only tab quits instead.
    fn close_tab(&mut self) -> Vec<Effect> {
        if self.tabs.len() == 1 {
            return vec![Effect::Quit];
        }
        let ids = self.tab().tree.leaves();
        let mut effects = self.drop_tab(self.active);
        effects.extend(ids.into_iter().map(Effect::ClosePane));
        effects
    }

    fn on_copy_key(&mut self, mut state: CopyState, key: &KeyEvent) -> Vec<Effect> {
        match state.on_key(key) {
            CopyCommand::Move(motion) => {
                let rows = self.focused_size().rows;
                let pane = &mut self.focused_state_mut().emu;
                state.apply(motion, pane.scrollback_len(), rows);
                pane.set_scrollback(state.offset);
                self.input = InputMode::Copy(state);
            }
            CopyCommand::Exit => {
                self.focused_state_mut().emu.set_scrollback(0);
                self.input = InputMode::Terminal;
            }
            CopyCommand::Ignore => self.input = InputMode::Copy(state),
        }
        Vec::new()
    }

    fn on_paste(&mut self, text: &str) -> Vec<Effect> {
        match self.input {
            InputMode::Terminal => vec![Effect::WritePty(
                self.tab().focus,
                encode_paste(text, self.focused_pane().modes()),
            )],
            _ => Vec::new(),
        }
    }

    fn on_resize(&mut self, cols: u16, rows: u16) -> Vec<Effect> {
        self.term = (cols, rows);
        self.refresh_screen();
        let effects = self.relayout();
        self.sync_copy_offset();
        self.dirty = true;
        effects
    }

    /// The emulator moves its own offset when output arrives while scrolled
    /// (it pins the viewport) and clamps it at the history cap. In COPY, the
    /// pane is the source of truth, so the state follows it.
    fn sync_copy_offset(&mut self) {
        if let InputMode::Copy(state) = &mut self.input {
            state.offset = self.panes[&self.tabs[self.active].focus]
                .emu
                .scrollback_offset();
        }
    }

    fn on_cwd(&mut self, id: PaneId, cwd: PathBuf) -> Vec<Effect> {
        if let Some(state) = self.panes.get_mut(&id)
            && state.cwd.as_ref() != Some(&cwd)
        {
            state.cwd = Some(cwd);
            self.dirty |= id == self.tab().focus;
        }
        Vec::new()
    }

    fn on_pty(&mut self, id: PaneId, event: PtyEvent) -> Vec<Effect> {
        match event {
            PtyEvent::Output(bytes) => {
                self.dirty = true;
                let reply = self
                    .panes
                    .get_mut(&id)
                    .map(|state| state.emu.feed(&bytes))
                    .unwrap_or_default();
                if id == self.tab().focus {
                    self.sync_copy_offset();
                }
                if reply.is_empty() {
                    Vec::new()
                } else {
                    vec![Effect::WritePty(id, reply)]
                }
            }
            PtyEvent::Exited => self.remove_pane(id),
        }
    }
}

/// PTY size for a layout rect. Rects may be empty in a tiny terminal, but a
/// PTY is never smaller than 1x1.
fn body_size(rect: Rect) -> PaneSize {
    PaneSize {
        rows: rect.height.max(1),
        cols: rect.width.max(1),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    #[default]
    Normal,
    AiChat,
    Metrics,
    Settings,
}

impl AppMode {
    pub const ALL: [AppMode; 4] = [Self::Normal, Self::AiChat, Self::Metrics, Self::Settings];

    /// Returns the next mode, wrapping from the last back to the first.
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::AiChat,
            Self::AiChat => Self::Metrics,
            Self::Metrics => Self::Settings,
            Self::Settings => Self::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::AiChat => "AI CHAT",
            Self::Metrics => "METRICS",
            Self::Settings => "SETTINGS",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_cycles_through_all_modes_and_wraps() {
        assert_eq!(AppMode::Normal.next(), AppMode::AiChat);
        assert_eq!(AppMode::AiChat.next(), AppMode::Metrics);
        assert_eq!(AppMode::Metrics.next(), AppMode::Settings);
        assert_eq!(AppMode::Settings.next(), AppMode::Normal);
    }

    #[test]
    fn labels_are_uppercase_names() {
        let labels: Vec<_> = AppMode::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(labels, ["NORMAL", "AI CHAT", "METRICS", "SETTINGS"]);
    }

    #[test]
    fn default_is_normal() {
        assert_eq!(AppMode::default(), AppMode::Normal);
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use crate::core::{
        copy::CopyState,
        pane::{CursorKind, CursorShape, PaneSize},
        pty::PtyEvent,
    };

    fn app() -> App {
        App::new(80, 25)
    }

    fn key(c: char) -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn ctrl_space() -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
    }

    fn esc() -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    }

    fn in_prefix() -> App {
        let mut a = app();
        a.update(ctrl_space());
        a
    }

    fn in_copy() -> App {
        let mut a = in_prefix();
        a.update(key('['));
        a
    }

    fn with_history(a: &mut App, lines: usize) {
        for i in 0..lines {
            a.update(AppEvent::Pty(
                PaneId::FIRST,
                PtyEvent::Output(format!("line{i}\r\n").into_bytes()),
            ));
        }
    }

    fn is_copy(a: &App) -> bool {
        matches!(a.input(), InputMode::Copy(_))
    }

    #[test]
    fn a_new_app_has_a_24_row_body_and_a_statusline_row() {
        let a = app();
        assert_eq!(
            a.screen(),
            ScreenLayout {
                tab_bar: None,
                body: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24
                },
                status: Rect {
                    x: 0,
                    y: 24,
                    width: 80,
                    height: 1
                },
            }
        );
        assert_eq!(a.focused_size(), PaneSize { rows: 24, cols: 80 });
    }

    #[test]
    fn resize_moves_the_screen_layout() {
        let mut a = app();
        a.update(AppEvent::Resize {
            cols: 100,
            rows: 40,
        });
        let screen = a.screen();
        assert_eq!((screen.body.width, screen.body.height), (100, 39));
        assert_eq!((screen.status.y, screen.status.width), (39, 100));
    }

    #[test]
    fn a_tiny_terminal_keeps_a_1x1_body() {
        let a = App::new(0, 0);
        assert_eq!((a.screen().body.width, a.screen().body.height), (1, 1));
        assert_eq!(a.focused_size(), PaneSize { rows: 1, cols: 1 });
    }

    #[test]
    fn the_initial_spawn_describes_the_shell_in_the_focused_pane() {
        let a = app().with_env(Some("/bin/zsh"), None, "/work".into());
        let (id, spec) = a.initial_spawn();
        assert_eq!(id, a.focused());
        assert_eq!(spec.program, PathBuf::from("/bin/zsh"));
        assert_eq!(spec.cwd, PathBuf::from("/work"));
        assert_eq!(spec.size, PaneSize { rows: 24, cols: 80 });
    }

    #[test]
    fn the_initial_spawn_falls_back_to_sh_in_the_root() {
        let (_, spec) = app().initial_spawn();
        assert_eq!(spec.program, PathBuf::from("/bin/sh"));
        assert_eq!(spec.cwd, PathBuf::from("/"));
    }

    #[test]
    fn take_dirty_reports_once_until_something_changes() {
        let mut a = app();
        assert!(a.take_dirty(), "a new app needs a first draw");
        assert!(!a.take_dirty());
        a.update(key('l'));
        assert!(a.take_dirty());
        assert!(!a.take_dirty());
    }

    #[test]
    fn effects_name_the_focused_pane() {
        let mut a = app();
        let id = a.focused();
        assert_eq!(
            a.update(key('l')),
            vec![Effect::WritePty(id, b"l".to_vec())]
        );
    }

    #[test]
    fn initial_mode_is_terminal() {
        assert_eq!(app().input(), InputMode::Terminal);
    }

    #[test]
    fn terminal_passes_keys_to_the_pty() {
        let mut a = app();
        assert_eq!(
            a.update(key('l')),
            vec![Effect::WritePty(PaneId::FIRST, b"l".to_vec())]
        );
        assert_eq!(a.input(), InputMode::Terminal);
        let enter = AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            a.update(enter),
            vec![Effect::WritePty(PaneId::FIRST, b"\r".to_vec())]
        );
    }

    #[test]
    fn unencodable_keys_write_nothing() {
        let mut a = app();
        let f20 = AppEvent::Key(KeyEvent::new(KeyCode::F(20), KeyModifiers::NONE));
        assert_eq!(a.update(f20), vec![]);
    }

    #[test]
    fn terminal_encodes_with_the_pane_modes() {
        let mut a = app();
        let up = AppEvent::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            a.update(up.clone()),
            vec![Effect::WritePty(PaneId::FIRST, b"\x1b[A".to_vec())]
        );
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1h".to_vec()),
        ));
        assert_eq!(
            a.update(up),
            vec![Effect::WritePty(PaneId::FIRST, b"\x1bOA".to_vec())]
        );
    }

    #[test]
    fn prefix_key_enters_prefix_without_writing() {
        let mut a = app();
        assert_eq!(a.update(ctrl_space()), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
    }

    #[test]
    fn prefix_twice_sends_a_literal_nul() {
        let mut a = in_prefix();
        assert_eq!(
            a.update(ctrl_space()),
            vec![Effect::WritePty(PaneId::FIRST, vec![0x00])]
        );
        assert_eq!(a.input(), InputMode::Terminal);
    }

    #[test]
    fn prefix_cancel_and_unmapped_keys_are_swallowed() {
        for ev in [esc(), key('z')] {
            let mut a = in_prefix();
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn prefix_q_asks_for_confirmation() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('q')), vec![]);
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
    }

    fn group_label(a: &App) -> &'static str {
        match a.input() {
            InputMode::Group(group) => group.label,
            other => panic!("expected a group, got {other:?}"),
        }
    }

    fn in_group(c: char) -> App {
        let mut a = in_prefix();
        a.update(key(c));
        a
    }

    #[test]
    fn group_keys_open_their_group_without_effects() {
        for (c, label) in [('w', "WINDOW"), ('t', "TAB"), ('g', "GO"), ('b', "BUFFER")] {
            let mut a = in_prefix();
            assert_eq!(a.update(key(c)), vec![], "{c}");
            assert_eq!(group_label(&a), label, "{c}");
        }
    }

    #[test]
    fn a_binding_in_a_group_returns_to_terminal_with_no_effect_yet() {
        for (g, c) in [('w', 'z'), ('g', 'b'), ('b', '3')] {
            let mut a = in_group(g);
            assert_eq!(a.update(key(c)), vec![], "{g} {c}");
            assert_eq!(a.input(), InputMode::Terminal, "{g} {c}");
        }
    }

    #[test]
    fn group_esc_unmapped_and_prefix_key_cancel_and_are_swallowed() {
        for ev in [esc(), key('x'), ctrl_space()] {
            let mut a = in_group('w');
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn question_mark_returns_to_terminal_with_no_effect() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('?')), vec![]);
        assert_eq!(a.input(), InputMode::Terminal);
    }

    #[test]
    fn root_focus_keys_with_one_pane_are_no_ops() {
        for c in ['h', 'j', 'k', 'l'] {
            let mut a = in_prefix();
            assert_eq!(a.update(key(c)), vec![], "{c}");
            assert_eq!(a.input(), InputMode::Terminal, "{c}");
        }
    }

    #[test]
    fn repeat_is_ignored_in_group_and_keeps_the_group_pending() {
        let mut a = in_group('w');
        assert_eq!(a.update(repeat('v')), vec![]);
        assert_eq!(group_label(&a), "WINDOW");
        let held = kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        );
        assert_eq!(a.update(held), vec![]);
        assert_eq!(group_label(&a), "WINDOW");
    }

    #[test]
    fn release_is_ignored_in_group() {
        let mut a = in_group('w');
        let up = kinded(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(a.update(up), vec![]);
        assert_eq!(group_label(&a), "WINDOW");
    }

    #[test]
    fn input_mode_labels_cover_group_resize_and_confirm() {
        assert_eq!(in_group('t').input().label(), "TAB");
        assert_eq!(InputMode::Resize.label(), "RESIZE");
        assert_eq!(InputMode::Confirm(Confirm::Quit).label(), "Quit? (y/n)");
    }

    // Deliberate (design ADR 12, spec "SHIFT MUST be ignored for character
    // keys"): terminals report a char with or without SHIFT.
    #[test]
    fn shift_is_ignored_for_character_keys_in_the_prefix_flow() {
        let mut a = app();
        let ctrl_shift_space = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char(' '),
            ctrl_shift_space,
        )));
        assert_eq!(a.input(), InputMode::Prefix);

        let shift_q = AppEvent::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::SHIFT));
        assert_eq!(a.update(shift_q), vec![]);
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
    }

    #[test]
    fn confirm_quit_y_quits() {
        let mut a = in_prefix();
        a.update(key('q'));
        assert_eq!(a.update(key('y')), vec![Effect::Quit]);
    }

    #[test]
    fn confirm_quit_declined_by_n_esc_or_other_keys() {
        for ev in [key('n'), esc(), key('x')] {
            let mut a = in_prefix();
            a.update(key('q'));
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn confirm_quit_uppercase_y_declines() {
        let shift_y = AppEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT));
        let caps_y = AppEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE));
        for ev in [shift_y, caps_y] {
            let mut a = in_prefix();
            a.update(key('q'));
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn prefix_open_bracket_enters_copy_without_writing() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('[')), vec![]);
        assert_eq!(a.input(), InputMode::Copy(CopyState::default()));
    }

    #[test]
    fn paste_goes_to_the_pty_only_in_terminal() {
        let mut a = app();
        assert_eq!(
            a.update(AppEvent::Paste("hi".into())),
            vec![Effect::WritePty(PaneId::FIRST, b"hi".to_vec())]
        );
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?2004h".to_vec()),
        ));
        assert_eq!(
            a.update(AppEvent::Paste("hi".into())),
            vec![Effect::WritePty(
                PaneId::FIRST,
                b"\x1b[200~hi\x1b[201~".to_vec()
            )]
        );
        for mut a in [in_prefix(), in_copy()] {
            assert_eq!(a.update(AppEvent::Paste("hi".into())), vec![]);
        }
        let mut a = in_prefix();
        a.update(key('q'));
        assert_eq!(a.update(AppEvent::Paste("hi".into())), vec![]);
    }

    #[test]
    fn resize_emits_resize_pty_in_every_mode() {
        let mut confirm = in_prefix();
        confirm.update(key('q'));
        for mut a in [app(), in_prefix(), in_copy(), confirm] {
            let effects = a.update(AppEvent::Resize {
                cols: 100,
                rows: 40,
            });
            assert_eq!(
                effects,
                vec![Effect::ResizePty(
                    PaneId::FIRST,
                    PaneSize {
                        rows: 39,
                        cols: 100
                    }
                )]
            );
        }
    }

    #[test]
    fn resize_resizes_the_emulator_too() {
        let mut a = app();
        a.update(AppEvent::Resize {
            cols: 100,
            rows: 40,
        });
        assert!(a.focused_pane().cell(38, 99).is_some());
        assert!(a.focused_pane().cell(39, 0).is_none());
    }

    #[test]
    fn pane_size_reserves_the_statusline_row_with_a_1x1_minimum() {
        assert_eq!(
            pane_size(100, 40),
            PaneSize {
                rows: 39,
                cols: 100
            }
        );
        assert_eq!(pane_size(80, 2), PaneSize { rows: 1, cols: 80 });
        assert_eq!(pane_size(1, 1), PaneSize { rows: 1, cols: 1 });
        assert_eq!(pane_size(0, 0), PaneSize { rows: 1, cols: 1 });
    }

    #[test]
    fn pty_exit_quits() {
        let mut a = app();
        assert_eq!(
            a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited)),
            vec![Effect::Quit]
        );
    }

    #[test]
    fn pty_event_for_the_current_pane_is_handled() {
        let mut a = app();
        let id = a.focused();
        assert_eq!(
            a.update(AppEvent::Pty(id, PtyEvent::Output(b"x".to_vec()))),
            vec![]
        );
        assert_eq!(a.focused_pane().cell(0, 0).unwrap().text, "x");
    }

    #[test]
    fn pty_output_is_parsed_and_marks_dirty() {
        let mut a = app();
        a.take_dirty();
        assert_eq!(
            a.update(AppEvent::Pty(
                PaneId::FIRST,
                PtyEvent::Output(b"hi".to_vec())
            )),
            vec![]
        );
        assert_eq!(a.focused_pane().cell(0, 0).unwrap().text, "h");
        assert!(a.take_dirty());
    }

    #[test]
    fn pty_output_with_a_query_writes_the_reply_back() {
        let mut a = app();
        let effects = a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[5n".to_vec()),
        ));
        assert_eq!(
            effects,
            vec![Effect::WritePty(PaneId::FIRST, b"\x1b[0n".to_vec())]
        );
    }

    fn kinded(code: KeyCode, mods: KeyModifiers, kind: KeyEventKind) -> AppEvent {
        AppEvent::Key(KeyEvent::new_with_kind(code, mods, kind))
    }

    fn repeat(c: char) -> AppEvent {
        kinded(KeyCode::Char(c), KeyModifiers::NONE, KeyEventKind::Repeat)
    }

    #[test]
    fn release_key_events_are_ignored_in_every_mode() {
        let release = |c| kinded(KeyCode::Char(c), KeyModifiers::NONE, KeyEventKind::Release);
        let mut a = app();
        assert_eq!(a.update(release('l')), vec![]);
        let mut a = in_prefix();
        assert_eq!(a.update(release('[')), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
        let mut a = app();
        a.update(kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
        ));
        assert_eq!(
            a.input(),
            InputMode::Terminal,
            "release must not enter PREFIX"
        );
    }

    #[test]
    fn repeat_in_terminal_writes_the_encoded_bytes() {
        let mut a = app();
        assert_eq!(
            a.update(repeat('l')),
            vec![Effect::WritePty(PaneId::FIRST, b"l".to_vec())]
        );
    }

    #[test]
    fn repeat_of_k_in_copy_moves_the_view() {
        let mut a = in_copy();
        with_history(&mut a, 40);
        a.update(repeat('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), 1);
        a.update(repeat('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), 2);
    }

    #[test]
    fn repeat_in_prefix_does_nothing_and_stays_in_prefix() {
        let mut a = in_prefix();
        assert_eq!(a.update(repeat('[')), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
        let held = kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        );
        assert_eq!(a.update(held), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
    }

    #[test]
    fn held_prefix_key_enters_prefix_once_and_repeats_are_dropped() {
        let mut a = app();
        a.update(ctrl_space());
        let held = kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        );
        assert_eq!(a.update(held), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
    }

    #[test]
    fn repeat_of_y_in_confirm_quit_does_not_quit() {
        let mut a = in_prefix();
        a.update(key('q'));
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
        assert_eq!(a.update(repeat('y')), vec![]);
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
    }

    #[test]
    fn copy_swallows_keys() {
        for ev in [
            key('j'),
            key('x'),
            AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        ] {
            let mut a = in_copy();
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert!(is_copy(&a), "{ev:?}");
        }
    }

    #[test]
    fn copy_motions_scroll_the_pane() {
        let mut a = in_copy();
        with_history(&mut a, 60);
        let history = a.focused_pane().scrollback_len();
        assert!(history > 30);
        a.update(key('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), 1);
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(a.focused_pane().scrollback_offset(), 13, "half of 24 rows");
        a.update(key('g'));
        a.update(key('g'));
        assert_eq!(a.focused_pane().scrollback_offset(), history);
        a.update(key('G'));
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
        a.update(key('j'));
        assert_eq!(
            a.focused_pane().scrollback_offset(),
            0,
            "clamped at the bottom"
        );
    }

    #[test]
    fn copy_exit_returns_to_terminal_at_the_bottom() {
        for ev in [key('q'), key('i'), esc()] {
            let mut a = in_copy();
            with_history(&mut a, 60);
            a.update(key('k'));
            a.update(key('k'));
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
            assert_eq!(a.focused_pane().scrollback_offset(), 0, "{ev:?}");
        }
    }

    #[test]
    fn copy_on_the_alternate_screen_stays_at_zero() {
        let mut a = app();
        with_history(&mut a, 60);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1049h".to_vec()),
        ));
        a.update(ctrl_space());
        a.update(key('['));
        assert!(is_copy(&a));
        for ev in [key('k'), key('j'), key('G')] {
            a.update(ev);
            assert_eq!(a.focused_pane().scrollback_offset(), 0);
        }
        a.update(key('g'));
        a.update(key('g'));
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
    }

    fn copy_offset(a: &App) -> usize {
        match a.input() {
            InputMode::Copy(state) => state.offset,
            other => panic!("not in COPY: {other:?}"),
        }
    }

    #[test]
    fn copy_resize_clamps_the_offset_and_keeps_it_in_sync_with_the_pane() {
        let mut a = in_copy();
        with_history(&mut a, 60);
        a.update(key('g'));
        a.update(key('g'));
        let before = copy_offset(&a);
        a.update(AppEvent::Resize { cols: 80, rows: 60 });
        let after = copy_offset(&a);
        assert!(after <= a.focused_pane().scrollback_len());
        assert_eq!(after, a.focused_pane().scrollback_offset());
        a.update(key('k'));
        assert!(copy_offset(&a) <= a.focused_pane().scrollback_len());
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        assert!(before > 0);
    }

    #[test]
    fn copy_offset_follows_the_pane_when_output_arrives_while_scrolled() {
        let mut a = in_copy();
        with_history(&mut a, 40);
        for _ in 0..10 {
            a.update(key('k'));
        }
        assert_eq!(copy_offset(&a), 10);
        with_history(&mut a, 5);
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        let anchored = a.focused_pane().scrollback_offset();
        assert!(anchored > 10, "the emulator pins the viewport");
        a.update(key('j'));
        assert_eq!(a.focused_pane().scrollback_offset(), anchored - 1);
        assert_eq!(copy_offset(&a), anchored - 1);
        a.update(key('k'));
        a.update(key('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), anchored + 1);
        assert_eq!(copy_offset(&a), anchored + 1);
    }

    #[test]
    fn copy_output_past_the_scrollback_cap_does_not_panic_and_stays_in_sync() {
        let mut a = in_copy();
        with_history(&mut a, SCROLLBACK_LINES + 100);
        a.update(key('g'));
        a.update(key('g'));
        with_history(&mut a, 50);
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        a.update(key('k'));
        a.update(key('j'));
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        assert!(a.focused_pane().scrollback_offset() <= a.focused_pane().scrollback_len());
    }

    fn visible_text(a: &App) -> Vec<String> {
        (0..a.focused_size().rows)
            .map(|row| {
                (0..a.focused_size().cols)
                    .filter_map(|col| a.focused_pane().cell(row, col))
                    .map(|cell| cell.text)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    // Spec: output during COPY keeps the same content visible. The anchoring
    // comes from the emulator pinning the viewport plus `sync_copy_offset`.
    #[test]
    fn output_during_copy_keeps_the_same_content_visible() {
        let mut a = in_copy();
        with_history(&mut a, 60);
        for _ in 0..10 {
            a.update(key('k'));
        }
        let before = visible_text(&a);
        assert_eq!(before.first().map(String::as_str), Some("line27"));
        with_history(&mut a, 5);
        assert_eq!(visible_text(&a), before);
        assert!(is_copy(&a));
        a.update(key('j'));
        assert_eq!(visible_text(&a)[0], "line28");
    }

    // Spec: entering COPY on the alternate screen shows the current screen.
    #[test]
    fn entering_copy_on_the_alternate_screen_shows_the_current_screen() {
        let mut a = app();
        with_history(&mut a, 60);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1049h\x1b[HALT SCREEN".to_vec()),
        ));
        a.update(ctrl_space());
        a.update(key('['));
        assert!(is_copy(&a));
        assert_eq!(visible_text(&a)[0], "ALT SCREEN");
    }

    // Spec: Position indicator follows the motions and live output.
    #[test]
    fn the_copy_position_tracks_motions_and_output_while_scrolled() {
        let mut a = app();
        with_history(&mut a, 60);
        assert_eq!(a.copy_position(), None, "outside COPY");
        a.update(ctrl_space());
        a.update(key('['));
        let total = a.focused_pane().scrollback_len();
        assert!(total > 0);
        assert_eq!(a.copy_position(), Some((0, total)));
        for _ in 0..10 {
            a.update(key('k'));
        }
        assert_eq!(a.copy_position(), Some((10, total)));
        with_history(&mut a, 5);
        let grown = a.focused_pane().scrollback_len();
        assert_eq!(a.copy_position(), Some((15, grown)));
        a.update(key('G'));
        assert_eq!(a.copy_position(), Some((0, grown)));
        a.update(key('g'));
        a.update(key('g'));
        assert_eq!(a.copy_position(), Some((grown, grown)));
    }

    // Spec: No history shows the bare label (alternate screen).
    #[test]
    fn the_copy_position_is_absent_without_history() {
        let mut a = app();
        with_history(&mut a, 60);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1049h".to_vec()),
        ));
        a.update(ctrl_space());
        a.update(key('['));
        assert!(is_copy(&a));
        assert_eq!(a.copy_position(), None);
    }

    fn cwd_event(path: &str) -> AppEvent {
        AppEvent::Cwd(PaneId::FIRST, PathBuf::from(path))
    }

    // Spec: stale events dropped.
    #[test]
    fn pty_events_from_unknown_panes_are_dropped() {
        let mut a = app();
        a.take_dirty();
        let unknown = PaneId::for_test(9);
        for event in [PtyEvent::Output(b"late".to_vec()), PtyEvent::Exited] {
            assert_eq!(a.update(AppEvent::Pty(unknown, event)), vec![]);
        }
        assert!(!a.take_dirty());
    }

    #[test]
    fn cwd_event_updates_the_cwd_and_marks_dirty() {
        let mut a = app();
        a.take_dirty();
        assert_eq!(a.update(cwd_event("/tmp")), vec![]);
        assert_eq!(a.cwd_label(), "/tmp");
        assert!(a.take_dirty());
    }

    #[test]
    fn unchanged_cwd_does_not_mark_dirty() {
        let mut a = app();
        a.update(cwd_event("/tmp"));
        a.take_dirty();
        a.update(cwd_event("/tmp"));
        assert!(!a.take_dirty());
    }

    #[test]
    fn cwd_label_is_empty_until_the_first_cwd() {
        assert_eq!(app().cwd_label(), "");
    }

    // The runtime only sends `Cwd` when the lookup succeeds, so a failed
    // lookup is the absence of an event: the previous value stays.
    #[test]
    fn cwd_is_retained_when_no_new_cwd_arrives() {
        let mut a = app();
        a.update(cwd_event("/tmp"));
        a.update(key('l'));
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"x".to_vec()),
        ));
        assert_eq!(a.cwd_label(), "/tmp");
    }

    #[test]
    fn home_is_shown_as_a_tilde() {
        let mut a = app().with_env(
            Some("/bin/zsh"),
            Some("/home/ana".into()),
            "/home/ana".into(),
        );
        assert_eq!(a.cwd_label(), "~");
        a.update(cwd_event("/home/ana/src/app"));
        assert_eq!(a.cwd_label(), "~/src/app");
        a.update(cwd_event("/home/anabel"));
        assert_eq!(
            a.cwd_label(),
            "/home/anabel",
            "prefix must be a path prefix"
        );
        a.update(cwd_event("/tmp"));
        assert_eq!(a.cwd_label(), "/tmp");
    }

    #[test]
    fn a_root_or_missing_home_never_produces_a_tilde() {
        for home in [None, Some(PathBuf::from("/")), Some(PathBuf::new())] {
            let a = app().with_env(None, home, "/usr/bin".into());
            assert_eq!(a.cwd_label(), "/usr/bin");
        }
    }

    #[test]
    fn shell_name_is_the_basename_of_the_shell_with_an_sh_fallback() {
        assert_eq!(shell_basename(Some("/bin/zsh")), "zsh");
        assert_eq!(shell_basename(Some("fish")), "fish");
        assert_eq!(shell_basename(Some("/usr/bin/")), "bin");
        for shell in [None, Some(""), Some("/")] {
            assert_eq!(shell_basename(shell), "sh", "{shell:?}");
        }
        assert_eq!(app().shell_name(), "sh");
        let a = app().with_env(Some("/bin/zsh"), None, "/".into());
        assert_eq!(a.shell_name(), "zsh");
    }

    fn shape(kind: CursorKind, blinking: bool) -> CursorShape {
        CursorShape { kind, blinking }
    }

    #[test]
    fn cursor_shape_follows_the_child_in_terminal_and_prefix() {
        let mut a = app();
        assert_eq!(a.cursor_shape(), CursorShape::default());
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[6 q".to_vec()),
        ));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
        a.update(ctrl_space());
        assert_eq!(a.input(), InputMode::Prefix);
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn cursor_shape_follows_the_focused_pane_and_ignores_the_others() {
        let mut a = app();
        let second = PaneId::for_test(2);
        let output = |id, bytes: &[u8]| AppEvent::Pty(id, PtyEvent::Output(bytes.to_vec()));
        a.update(output(PaneId::FIRST, b"\x1b[6 q"));
        a.update(ctrl_space());
        a.update(key('w'));
        a.update(key('v'));
        assert_eq!(a.focused(), second);
        assert_eq!(a.cursor_shape(), CursorShape::default());
        a.update(output(second, b"\x1b[2 q"));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        // The unfocused left pane changes shape: the outer shape stays.
        a.update(output(PaneId::FIRST, b"\x1b[4 q"));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.update(ctrl_space());
        a.update(key('h'));
        assert_eq!(a.focused(), PaneId::FIRST);
        assert_eq!(a.cursor_shape(), shape(CursorKind::Underline, false));
    }

    #[test]
    fn cursor_shape_is_a_steady_block_in_copy_and_restores_on_exit() {
        let mut a = app();
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[6 q".to_vec()),
        ));
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.update(key('q'));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn child_shape_requests_during_copy_do_not_leak_until_exit() {
        let mut a = in_copy();
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[3 q".to_vec()),
        ));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.update(key('q'));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Underline, true));
    }

    // --- S5a: split and focus ---

    const LAUNCH: &str = "/launch";

    fn id2() -> PaneId {
        PaneId::for_test(2)
    }

    fn sized(rows: u16, cols: u16) -> PaneSize {
        PaneSize { rows, cols }
    }

    fn launched(cols: u16, rows: u16) -> App {
        App::new(cols, rows).with_env(Some("/bin/zsh"), None, LAUNCH.into())
    }

    fn spec(cwd: &str, size: PaneSize) -> SpawnSpec {
        SpawnSpec::new(Some("/bin/zsh"), cwd.into(), size)
    }

    /// Press the prefix, then each of `keys`.
    fn press(a: &mut App, keys: &str) -> Vec<Effect> {
        a.update(ctrl_space());
        keys.chars().flat_map(|c| a.update(key(c))).collect()
    }

    fn split_right() -> App {
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        a
    }

    fn split_below() -> App {
        let mut a = launched(80, 25);
        press(&mut a, "wh");
        a
    }

    // Spec: Split right; Focus follows the new pane.
    #[test]
    fn split_right_spawns_a_side_by_side_pane_and_focuses_it() {
        let mut a = launched(80, 25);
        let effects = press(&mut a, "wv");
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 39)),
                Effect::SpawnPane(id2(), spec(LAUNCH, sized(24, 40))),
            ]
        );
        assert_eq!(a.focused(), id2());
        assert_eq!(a.input(), InputMode::Terminal);
    }

    // Spec: Split below.
    #[test]
    fn split_below_stacks_the_new_pane_and_focuses_it() {
        let mut a = launched(80, 25);
        let effects = press(&mut a, "wh");
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(11, 80)),
                Effect::SpawnPane(id2(), spec(LAUNCH, sized(12, 80))),
            ]
        );
        assert_eq!(a.focused(), id2());
    }

    // Spec: Split inherits cwd.
    #[test]
    fn a_split_spawns_in_the_cwd_of_the_focused_pane() {
        let mut a = launched(80, 25);
        a.update(cwd_event("/tmp"));
        let effects = press(&mut a, "wv");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec("/tmp", sized(24, 40))))
        );
    }

    // Spec: Unknown cwd falls back to the launch cwd.
    #[test]
    fn a_split_from_a_pane_without_a_known_cwd_uses_the_launch_cwd() {
        let mut a = split_right();
        let effects = press(&mut a, "wv");
        match effects.last() {
            Some(Effect::SpawnPane(id, spawn)) => {
                assert_eq!(*id, PaneId::for_test(3));
                assert_eq!(spawn.cwd, PathBuf::from(LAUNCH));
            }
            other => panic!("expected a spawn, got {other:?}"),
        }
    }

    // Spec: Refused below minimum.
    #[test]
    fn a_refused_split_is_a_no_op() {
        // 20 columns cannot hold two panes plus a separator; 4 rows neither.
        for (cols, rows, keys) in [(20, 25, "wv"), (80, 5, "wh")] {
            let mut a = launched(cols, rows);
            a.take_dirty();
            assert_eq!(press(&mut a, keys), vec![], "{keys}");
            assert_eq!(a.focused(), PaneId::FIRST, "{keys}");
            assert_eq!(a.input(), InputMode::Terminal, "{keys}");
        }
    }

    // Spec: Focus keys.
    #[test]
    fn focus_keys_move_to_the_geometric_neighbour() {
        let mut a = split_right();
        assert_eq!(press(&mut a, "h"), vec![]);
        assert_eq!(a.focused(), PaneId::FIRST);
        assert_eq!(a.input(), InputMode::Terminal);
        press(&mut a, "l");
        assert_eq!(a.focused(), id2());
        let mut a = split_below();
        press(&mut a, "k");
        assert_eq!(a.focused(), PaneId::FIRST);
        press(&mut a, "j");
        assert_eq!(a.focused(), id2());
    }

    // Spec: Edge no-op.
    #[test]
    fn focus_at_an_edge_stays_put() {
        let mut a = split_right();
        for keys in ["l", "j", "k"] {
            assert_eq!(press(&mut a, keys), vec![], "{keys}");
            assert_eq!(a.focused(), id2(), "{keys}");
            assert_eq!(a.input(), InputMode::Terminal, "{keys}");
        }
    }

    // Spec: Input goes to the focused pane only.
    #[test]
    fn keys_and_paste_go_to_the_focused_pane_only() {
        let mut a = split_right();
        assert_eq!(
            a.update(key('x')),
            vec![Effect::WritePty(id2(), b"x".to_vec())]
        );
        assert_eq!(
            a.update(AppEvent::Paste("p".into())),
            vec![Effect::WritePty(id2(), b"p".to_vec())]
        );
        press(&mut a, "h");
        assert_eq!(
            a.update(key('x')),
            vec![Effect::WritePty(PaneId::FIRST, b"x".to_vec())]
        );
    }

    // Spec: Only changed panes resized; Unaffected pane untouched.
    #[test]
    fn a_host_resize_resizes_only_the_panes_whose_size_changed() {
        let mut a = split_below();
        let effects = a.update(AppEvent::Resize { cols: 80, rows: 26 });
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(13, 80))]);
        assert_eq!(a.update(AppEvent::Resize { cols: 80, rows: 26 }), vec![]);
    }

    #[test]
    fn a_host_resize_resizes_every_pane_that_changed() {
        let mut a = split_right();
        let effects = a.update(AppEvent::Resize {
            cols: 100,
            rows: 40,
        });
        let sizes: Vec<_> = effects
            .iter()
            .map(|e| match e {
                Effect::ResizePty(id, size) => (*id, size.rows, size.cols),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(sizes, vec![(PaneId::FIRST, 39, 48), (id2(), 39, 51)]);
    }

    // Spec: Stale events dropped; events route to their own pane.
    #[test]
    fn pty_output_is_routed_to_its_own_pane() {
        let mut a = split_right();
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"a".to_vec()),
        ));
        a.update(AppEvent::Pty(id2(), PtyEvent::Output(b"b".to_vec())));
        assert_eq!(a.pane(PaneId::FIRST).unwrap().cell(0, 0).unwrap().text, "a");
        assert_eq!(a.pane(id2()).unwrap().cell(0, 0).unwrap().text, "b");
    }

    #[test]
    fn events_for_unknown_panes_are_still_dropped_with_several_panes() {
        let mut a = split_right();
        a.take_dirty();
        let unknown = PaneId::for_test(9);
        assert_eq!(
            a.update(AppEvent::Pty(unknown, PtyEvent::Output(b"x".to_vec()))),
            vec![]
        );
        assert_eq!(a.update(AppEvent::Cwd(unknown, "/x".into())), vec![]);
        assert!(!a.take_dirty());
    }

    #[test]
    fn the_cwd_label_follows_the_focused_pane() {
        let mut a = split_right();
        a.update(AppEvent::Cwd(PaneId::FIRST, "/a".into()));
        assert_eq!(a.cwd_label(), "");
        a.update(AppEvent::Cwd(id2(), "/b".into()));
        assert_eq!(a.cwd_label(), "/b");
        press(&mut a, "h");
        assert_eq!(a.cwd_label(), "/a");
    }
    fn exited(id: PaneId) -> AppEvent {
        AppEvent::Pty(id, PtyEvent::Exited)
    }

    fn id3() -> PaneId {
        PaneId::for_test(3)
    }

    /// Left pane 1, top-right pane 2, bottom-right pane 3 (focused).
    fn three_panes() -> App {
        let mut a = split_right();
        press(&mut a, "wh");
        a
    }

    // Spec: Close pane confirmed; Last pane prompts quit.
    #[test]
    fn close_pane_asks_then_y_closes_the_focused_pane() {
        let mut a = split_right();
        press(&mut a, "wq");
        assert_eq!(a.input(), InputMode::Confirm(Confirm::ClosePane));
        assert_eq!(a.input().label(), "Close pane? (y/n)");
        let effects = a.update(key('y'));
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 80)),
                Effect::ClosePane(id2())
            ]
        );
        assert_eq!(
            (a.focused(), a.input()),
            (PaneId::FIRST, InputMode::Terminal)
        );
        assert!(a.pane(id2()).is_none());
        let mut one = launched(80, 25);
        press(&mut one, "wq");
        assert_eq!(one.input(), InputMode::Confirm(Confirm::Quit));
        assert_eq!(one.update(key('y')), vec![Effect::Quit]);
    }

    // Spec: Close pane declined; Confirm is strict lowercase y.
    #[test]
    fn close_pane_is_declined_by_anything_but_a_plain_y() {
        for decline in [key('n'), esc(), key('x'), key('Y')] {
            let mut a = split_right();
            press(&mut a, "wq");
            assert_eq!(a.update(decline.clone()), vec![], "{decline:?}");
            assert_eq!(a.input(), InputMode::Terminal);
            assert!(a.pane(id2()).is_some());
        }
    }

    // Spec: Only tab prompts quit.
    #[test]
    fn close_tab_on_the_only_tab_asks_to_quit() {
        let mut a = launched(80, 25);
        press(&mut a, "tc");
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
    }

    // Spec: One of two shells exits; sibling expands.
    #[test]
    fn an_unfocused_shell_exit_collapses_its_pane_and_keeps_focus() {
        let mut a = split_right();
        let effects = a.update(exited(PaneId::FIRST));
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(24, 80))]);
        assert_eq!(a.focused(), id2());
        assert!(a.pane(PaneId::FIRST).is_none());
    }

    // Spec: focus goes to the pane covering the closed pane's top-left.
    #[test]
    fn a_focused_shell_exit_focuses_the_pane_covering_its_top_left() {
        let mut a = three_panes();
        assert_eq!(
            a.update(exited(id3())),
            vec![Effect::ResizePty(id2(), sized(24, 40))]
        );
        assert_eq!(a.focused(), id2());
        press(&mut a, "h");
        let effects = a.update(exited(PaneId::FIRST));
        assert!(!effects.contains(&Effect::Quit));
        assert_eq!(a.focused(), id2());
    }

    // Spec: Last shell exits.
    #[test]
    fn the_last_shell_exiting_quits() {
        let mut a = split_right();
        a.update(exited(PaneId::FIRST));
        assert_eq!(a.update(exited(id2())), vec![Effect::Quit]);
    }

    // Spec: Removal cancels the prompt.
    #[test]
    fn any_removal_cancels_a_pending_confirmation() {
        for victim in [PaneId::FIRST, id2()] {
            let mut a = split_right();
            press(&mut a, "wq");
            a.update(exited(victim));
            assert_eq!(a.input(), InputMode::Terminal, "{victim:?}");
            let survivor = a.focused();
            assert_eq!(
                a.update(key('y')),
                vec![Effect::WritePty(survivor, b"y".to_vec())]
            );
        }
    }

    fn scrolled_split() -> App {
        let mut a = split_right();
        for id in [PaneId::FIRST, id2()] {
            for i in 0..60 {
                let line = format!("l{i}\r\n").into_bytes();
                a.update(AppEvent::Pty(id, PtyEvent::Output(line)));
            }
        }
        a.update(ctrl_space());
        a.update(key('['));
        a.update(key('k'));
        a
    }

    // Spec: Focused pane exits while in COPY; Pane close in COPY.
    #[test]
    fn the_focused_pane_exiting_in_copy_returns_to_a_live_terminal() {
        let mut a = scrolled_split();
        assert!(is_copy(&a));
        a.update(exited(id2()));
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.focused(), PaneId::FIRST);
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
    }

    // Spec: Offsets are independent; Other panes keep running; Keys act on the focused pane.
    #[test]
    fn copy_is_per_pane_and_other_panes_keep_running() {
        let mut a = scrolled_split();
        assert_eq!(a.pane(PaneId::FIRST).unwrap().scrollback_offset(), 0);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"live".to_vec()),
        ));
        assert_eq!(a.pane(PaneId::FIRST).unwrap().cell(0, 0).unwrap().text, "l");
        a.update(key('k'));
        assert_eq!(a.pane(PaneId::FIRST).unwrap().scrollback_offset(), 0);
        assert_eq!(a.pane(id2()).unwrap().scrollback_offset(), 2);
        // Removing another pane keeps COPY when the focus does not move.
        a.update(exited(PaneId::FIRST));
        assert!(is_copy(&a));
    }

    // Spec: Focus change / removal that keeps focus, in RESIZE.
    #[test]
    fn resize_ends_when_the_focus_moves_but_not_when_it_stays() {
        let mut a = three_panes();
        press(&mut a, "wr");
        assert_eq!(a.input(), InputMode::Resize);
        a.update(exited(PaneId::FIRST));
        assert_eq!(a.input(), InputMode::Resize);
        a.update(exited(id3()));
        assert_eq!(a.input(), InputMode::Terminal);
    }

    // Spec: Failed split removes the pane.
    #[test]
    fn a_failed_split_removes_the_pane_and_restores_focus() {
        let mut a = split_right();
        let effects = a.update(AppEvent::SpawnFailed(id2(), "boom".into()));
        assert_eq!(
            effects,
            vec![Effect::ResizePty(PaneId::FIRST, sized(24, 80))]
        );
        assert_eq!(a.focused(), PaneId::FIRST);
        assert!(a.pane(id2()).is_none());
        assert_eq!(a.notice(), Some("spawn failed: boom"));
        assert_eq!(a.status_path(80), "spawn failed: boom");
        // A failed pane that is not focused leaves the focus alone.
        let mut b = three_panes();
        b.update(AppEvent::SpawnFailed(id2(), "x".into()));
        assert_eq!(b.focused(), id3());
    }

    // Spec: Notice clears on next key; a newer notice replaces an older one.
    #[test]
    fn the_notice_lasts_until_the_next_press_or_repeat() {
        let mut a = three_panes();
        a.update(AppEvent::SpawnFailed(id2(), "a".into()));
        a.update(AppEvent::SpawnFailed(id3(), "b".into()));
        assert_eq!(a.notice(), Some("spawn failed: b"));
        a.update(kinded(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(a.notice(), Some("spawn failed: b"));
        a.update(key('x'));
        assert_eq!(a.notice(), None);
        a.update(AppEvent::SpawnFailed(id2(), "c".into()));
        a.update(repeat('x'));
        assert_eq!(a.notice(), None);
    }

    #[test]
    fn a_failed_spawn_for_the_only_pane_quits() {
        let mut a = launched(80, 25);
        assert_eq!(
            a.update(AppEvent::SpawnFailed(PaneId::FIRST, "no".into())),
            vec![Effect::Quit]
        );
    }

    // A refused split must leave no trace: ids are never reused, but the
    // consumed id is the only side effect.
    #[test]
    fn a_refused_split_leaves_the_tree_and_panes_unchanged() {
        let mut a = launched(20, 25);
        press(&mut a, "wv");
        assert_eq!(a.tab().tree, Node::Leaf(PaneId::FIRST));
        assert_eq!(a.panes.len(), 1);
        a.update(AppEvent::Resize { cols: 80, rows: 25 });
        press(&mut a, "wv");
        assert_eq!(a.focused(), id3());
    }

    const ROOT_HINT: &str = "w window · t tab · g go · b buffer · [ copy · q quit";
    const WINDOW_HINT: &str = "v split right · h split below · q close · r resize · z zoom";

    // Spec: Root hint in PREFIX; Group hint; Hint gone after the group resolves.
    #[test]
    fn the_path_segment_shows_the_root_then_the_group_hint_then_the_cwd() {
        let mut a = app().with_env(None, None, "/work".into());
        assert_eq!(a.status_path(200), "/work");
        a.update(ctrl_space());
        assert_eq!(a.status_path(200), ROOT_HINT);
        a.update(key('w'));
        assert_eq!(a.status_path(200), WINDOW_HINT);
        // A new pane has no cwd yet, so resolve with Esc to keep the focus.
        a.update(esc());
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.status_path(200), "/work");
    }

    // Spec: Root hint clipped and cleared; Hint clipped by whole entries.
    #[test]
    fn the_hint_is_clipped_by_whole_entries_to_the_budget() {
        let mut a = in_prefix().with_env(None, None, "/work".into());
        assert_eq!(a.status_path(30), "w window · t tab · g go · …");
        a.update(key('w'));
        assert_eq!(a.status_path(30), "v split right · …");
        // Not even one entry fits: just the ellipsis, then nothing; never the cwd.
        assert_eq!(a.status_path(2), "…");
        assert_eq!(a.status_path(0), "");
        a.update(esc());
        assert_eq!(a.status_path(2), "/work");
    }

    // Spec: Notice shown. ADR 30: notice > hint > cwd.
    #[test]
    fn a_notice_wins_over_the_hint() {
        let mut a = split_right();
        a.update(ctrl_space());
        a.update(AppEvent::SpawnFailed(id2(), "late".into()));
        assert_eq!(a.input(), InputMode::Prefix);
        assert_eq!(a.status_path(200), "spawn failed: late");
    }

    // Spec: COPY, RESIZE and prompts show the cwd, not a hint.
    #[test]
    fn modes_without_a_hint_show_the_cwd() {
        let mut a = split_right().with_env(None, None, "/work".into());
        a.update(ctrl_space());
        a.update(key('w'));
        a.update(key('q'));
        assert_eq!(a.input(), InputMode::Confirm(Confirm::ClosePane));
        assert_eq!(a.status_path(200), "/work");
    }

    // --- RESIZE mode (S7) --------------------------------------------------

    fn in_resize(mut a: App) -> App {
        press(&mut a, "wr");
        a
    }

    /// Two panes, left 39 and right 40 wide; the focus is on the right one.
    fn resizing() -> App {
        in_resize(split_right())
    }

    // Spec: Sticky (entering RESIZE emits nothing).
    #[test]
    fn w_r_enters_resize_without_effects() {
        let mut a = split_right();
        assert_eq!(press(&mut a, "wr"), vec![]);
        assert_eq!(a.input(), InputMode::Resize);
    }

    // Spec: Sticky; Border moves one cell (both panes told, in tile order).
    #[test]
    fn h_and_l_move_the_border_and_tell_both_panes() {
        let mut a = resizing();
        assert_eq!(
            a.update(key('l')),
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 40)),
                Effect::ResizePty(id2(), sized(24, 39)),
            ]
        );
        assert_eq!(
            a.update(key('h')),
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 39)),
                Effect::ResizePty(id2(), sized(24, 40)),
            ]
        );
        assert_eq!(a.update(key('h')).len(), 2);
        assert_eq!(a.tiling().panes[0].1.width, 38);
    }

    #[test]
    fn j_and_k_move_a_stacked_border() {
        let mut a = in_resize(split_below());
        let before = a.tiling().panes[0].1.height;
        assert_eq!(a.update(key('k')).len(), 2);
        assert_eq!(a.tiling().panes[0].1.height, before - 1);
        assert_eq!(a.update(key('j')).len(), 2);
        assert_eq!(a.tiling().panes[0].1.height, before);
    }

    // Spec: Sticky until Esc.
    #[test]
    fn resize_stays_until_esc_across_many_steps() {
        let mut a = resizing();
        for c in "hhlkjhl".chars() {
            a.update(key(c));
            assert_eq!(a.input(), InputMode::Resize, "after {c}");
        }
    }

    // Spec: Repeat allowed.
    #[test]
    fn repeat_steps_like_press_in_resize() {
        let mut a = resizing();
        assert_eq!(a.update(repeat('l')).len(), 2);
        assert_eq!(a.update(repeat('l')).len(), 2);
        assert_eq!(a.tiling().panes[0].1.width, 41);
        assert_eq!(a.input(), InputMode::Resize);
    }

    // Spec: Esc exits.
    #[test]
    fn esc_returns_to_terminal_without_effects() {
        let mut a = resizing();
        assert_eq!(a.update(esc()), vec![]);
        assert_eq!(a.input(), InputMode::Terminal);
    }

    // Spec: Keys swallowed (including Ctrl+Space and unknown keys).
    #[test]
    fn other_keys_and_ctrl_space_are_swallowed_and_resize_stays() {
        let mut a = resizing();
        let swallowed = [
            key('x'),
            key('H'),
            key('w'),
            key('q'),
            ctrl_space(),
            kinded(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Press),
            kinded(
                KeyCode::Char('l'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
        ];
        for event in swallowed {
            assert_eq!(a.update(event.clone()), vec![], "{event:?}");
            assert_eq!(a.input(), InputMode::Resize, "{event:?}");
        }
        assert_eq!(a.tiling().panes[0].1.width, 39);
    }

    #[test]
    fn release_is_ignored_in_resize() {
        let mut a = resizing();
        let up = kinded(
            KeyCode::Char('l'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(a.update(up), vec![]);
        assert_eq!(a.tiling().panes[0].1.width, 39);
        assert_eq!(a.input(), InputMode::Resize);
    }

    // Spec: a step with no border on that axis emits nothing.
    #[test]
    fn a_step_without_a_border_on_that_axis_emits_nothing() {
        let mut a = in_resize(split_below());
        assert_eq!(a.update(key('h')), vec![]);
        assert_eq!(a.update(key('l')), vec![]);
        assert_eq!(a.input(), InputMode::Resize);
    }

    // Spec: Single pane RESIZE.
    #[test]
    fn resize_with_a_single_pane_is_allowed_and_a_no_op() {
        let mut a = in_resize(launched(80, 25));
        assert_eq!(a.input(), InputMode::Resize);
        for c in "hjkl".chars() {
            assert_eq!(a.update(key(c)), vec![]);
        }
        assert_eq!(a.input(), InputMode::Resize);
        a.update(esc());
        assert_eq!(a.input(), InputMode::Terminal);
    }

    #[test]
    fn a_step_marks_the_screen_dirty() {
        let mut a = resizing();
        a.take_dirty();
        a.update(key('l'));
        assert!(a.take_dirty());
    }

    // --- Zoom (S7) ---------------------------------------------------------

    fn body(a: &App) -> crate::core::layout::Rect {
        a.screen().body
    }

    // Spec: Zoom and restore; Zoom resizes only the zoomed pane.
    #[test]
    fn w_z_zooms_the_focused_pane_over_the_whole_body() {
        let mut a = split_right();
        let effects = press(&mut a, "wz");
        assert!(a.zoomed());
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(24, 80))]);
        let tiling = a.tiling();
        assert_eq!(tiling.panes, vec![(id2(), body(&a))]);
        assert_eq!(tiling.separators, vec![]);
    }

    #[test]
    fn zooming_twice_restores_the_layout_and_the_pane_size() {
        let mut a = split_right();
        let before = a.tiling();
        press(&mut a, "wz");
        let effects = press(&mut a, "wz");
        assert!(!a.zoomed());
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(24, 40))]);
        assert_eq!(a.tiling(), before);
    }

    #[test]
    fn a_host_resize_while_zoomed_resizes_only_the_zoomed_pane_until_unzoom() {
        let mut a = split_right();
        press(&mut a, "wz");
        let effects = a.update(AppEvent::Resize { cols: 90, rows: 25 });
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(24, 90))]);
        let effects = press(&mut a, "wz");
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 43)),
                Effect::ResizePty(id2(), sized(24, 46)),
            ]
        );
    }

    #[test]
    fn zooming_a_single_pane_changes_no_size() {
        let mut a = launched(80, 25);
        assert_eq!(press(&mut a, "wz"), vec![]);
        assert!(a.zoomed());
        assert_eq!(press(&mut a, "wz"), vec![]);
        assert!(!a.zoomed());
    }

    // Spec: Split while zoomed.
    #[test]
    fn a_split_while_zoomed_unzooms_first_and_then_splits() {
        let mut a = split_right();
        press(&mut a, "wz");
        let effects = press(&mut a, "wh");
        assert!(!a.zoomed());
        assert_eq!(a.tiling().panes.len(), 3);
        assert!(effects.iter().any(|e| matches!(e, Effect::SpawnPane(..))));
    }

    #[test]
    fn a_refused_split_while_zoomed_still_unzooms() {
        let mut a = launched(20, 25);
        press(&mut a, "wz");
        let effects = press(&mut a, "wv");
        assert!(!a.zoomed());
        assert_eq!(a.tiling().panes.len(), 1);
        assert!(!effects.iter().any(|e| matches!(e, Effect::SpawnPane(..))));
    }

    // Spec: Focus change while zoomed.
    #[test]
    fn a_focus_key_while_zoomed_unzooms_and_moves_to_the_neighbour() {
        let mut a = split_right();
        press(&mut a, "wz");
        let effects = press(&mut a, "h");
        assert!(!a.zoomed());
        assert_eq!(a.focused(), PaneId::FIRST);
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(24, 40))]);
    }

    // Spec: RESIZE while zoomed.
    #[test]
    fn entering_resize_while_zoomed_unzooms_first() {
        let mut a = split_right();
        press(&mut a, "wz");
        let effects = press(&mut a, "wr");
        assert!(!a.zoomed());
        assert_eq!(a.input(), InputMode::Resize);
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(24, 40))]);
    }

    // Spec: any pane removal while zoomed unzooms first.
    #[test]
    fn removing_a_pane_while_zoomed_unzooms() {
        let mut a = three_panes();
        press(&mut a, "wz");
        a.update(exited(PaneId::FIRST));
        assert!(!a.zoomed());
        assert_eq!(a.tiling().panes.len(), 2);
        let mut b = three_panes();
        press(&mut b, "wz");
        press(&mut b, "wq");
        b.update(key('y'));
        assert!(!b.zoomed());
        assert_eq!(b.tiling().panes.len(), 2);
    }

    #[test]
    fn a_zoom_toggle_marks_the_screen_dirty() {
        let mut a = split_right();
        a.take_dirty();
        press(&mut a, "wz");
        assert!(a.take_dirty());
    }

    // --- S8a: tabs ---

    /// `n` tabs, the last one active; tab k holds pane k.
    fn tabs(n: usize) -> App {
        let mut a = launched(80, 25);
        for _ in 1..n {
            press(&mut a, "tn");
        }
        a
    }

    fn pid(n: u32) -> PaneId {
        PaneId::for_test(n)
    }

    // Spec: New tab; Hidden tabs stay sized.
    #[test]
    fn new_tab_is_appended_activated_and_spawns_in_the_focused_cwd() {
        let mut a = launched(80, 25);
        a.update(AppEvent::Cwd(PaneId::FIRST, "/tmp".into()));
        let effects = press(&mut a, "tn");
        assert_eq!((a.tab_count(), a.active_tab()), (2, 1));
        assert_eq!(a.focused(), id2());
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(23, 80)),
                Effect::SpawnPane(id2(), spec("/tmp", sized(23, 80))),
            ]
        );
    }

    // Spec: Unknown cwd falls back to the launch cwd.
    #[test]
    fn new_tab_falls_back_to_the_launch_cwd() {
        let mut a = tabs(1);
        let effects = press(&mut a, "tn");
        assert!(effects.contains(&Effect::SpawnPane(id2(), spec(LAUNCH, sized(23, 80)))));
    }

    // Spec: New tab is appended.
    #[test]
    fn a_new_tab_never_shifts_the_numbers() {
        let mut a = tabs(3);
        press(&mut a, "b1");
        press(&mut a, "tn");
        assert_eq!((a.tab_count(), a.active_tab()), (4, 3));
        for (keys, pane) in [("b1", 1), ("b2", 2), ("b3", 3), ("b4", 4)] {
            press(&mut a, keys);
            assert_eq!(a.focused(), pid(pane), "{keys}");
        }
    }

    // Spec: Close tab confirmed.
    #[test]
    fn close_tab_asks_then_y_closes_the_tab_and_its_panes() {
        let mut a = tabs(2);
        press(&mut a, "wv");
        press(&mut a, "tc");
        assert_eq!(a.input(), InputMode::Confirm(Confirm::CloseTab));
        let effects = a.update(key('y'));
        assert_eq!((a.tab_count(), a.active_tab()), (1, 0));
        assert_eq!(a.focused(), PaneId::FIRST);
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.screen().tab_bar, None);
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 80)),
                Effect::ClosePane(id2()),
                Effect::ClosePane(id3()),
            ]
        );
        assert!(a.pane(id2()).is_none() && a.pane(id3()).is_none());
    }

    #[test]
    fn close_tab_prompt_reads_close_tab() {
        assert_eq!(
            InputMode::Confirm(Confirm::CloseTab).label(),
            "Close tab? (y/n)"
        );
    }

    // Spec: Next tab becomes active; closing the last tab picks the new last.
    #[test]
    fn closing_a_tab_activates_the_one_that_takes_its_index() {
        let mut a = tabs(3);
        press(&mut a, "b2");
        press(&mut a, "tc");
        a.update(key('y'));
        assert_eq!((a.tab_count(), a.active_tab()), (2, 1));
        assert_eq!(a.focused(), pid(3));
        press(&mut a, "tc");
        a.update(key('y'));
        assert_eq!((a.tab_count(), a.active_tab()), (1, 0));
        assert_eq!(a.focused(), PaneId::FIRST);
    }

    // Spec: Close tab declined.
    #[test]
    fn close_tab_is_declined_by_anything_but_a_plain_y() {
        for decline in [key('n'), esc(), key('Y')] {
            let mut a = tabs(2);
            press(&mut a, "tc");
            assert_eq!(a.update(decline.clone()), vec![], "{decline:?}");
            assert_eq!((a.tab_count(), a.input()), (2, InputMode::Terminal));
        }
    }

    // Spec: Close the only tab quits.
    #[test]
    fn closing_the_only_tab_quits() {
        let mut a = tabs(1);
        press(&mut a, "tc");
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
        assert_eq!(a.update(key('y')), vec![Effect::Quit]);
    }

    // Spec: Next wraps; Previous wraps.
    #[test]
    fn next_and_previous_tab_wrap_around() {
        let mut a = tabs(3);
        press(&mut a, "gb");
        assert_eq!(a.active_tab(), 0);
        press(&mut a, "gB");
        assert_eq!(a.active_tab(), 2);
        press(&mut a, "gB");
        assert_eq!(a.active_tab(), 1);
        assert_eq!(a.input(), InputMode::Terminal);
    }

    // Spec: Go to tab N; Missing tab is a no-op.
    #[test]
    fn goto_selects_the_tab_and_ignores_a_missing_one() {
        let mut a = tabs(3);
        press(&mut a, "b2");
        assert_eq!(a.active_tab(), 1);
        press(&mut a, "b9");
        assert_eq!((a.active_tab(), a.input()), (1, InputMode::Terminal));
    }

    // Spec: Single tab navigation.
    #[test]
    fn navigation_with_one_tab_changes_nothing() {
        let mut a = tabs(1);
        for keys in ["gb", "gB", "b1", "b2"] {
            assert_eq!(press(&mut a, keys), vec![], "{keys}");
            assert_eq!((a.active_tab(), a.input()), (0, InputMode::Terminal));
        }
    }

    // Spec: State preserved (layout, focus and zoom per tab).
    #[test]
    fn each_tab_keeps_its_layout_focus_and_zoom() {
        let mut a = split_right();
        press(&mut a, "wz");
        press(&mut a, "tn");
        assert!(!a.zoomed());
        assert_eq!(a.tiling().panes.len(), 1);
        press(&mut a, "b1");
        assert_eq!(a.focused(), id2());
        assert!(a.zoomed());
        press(&mut a, "wz");
        assert_eq!(a.tiling().panes.len(), 2);
    }

    // Spec: Background output.
    #[test]
    fn output_for_a_background_tab_is_parsed() {
        let mut a = tabs(2);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"x".to_vec()),
        ));
        press(&mut a, "b1");
        assert_eq!(a.focused_pane().cell(0, 0).unwrap().text, "x");
    }

    // Spec: Inactive tab removal keeps the active tab.
    #[test]
    fn an_inactive_tab_closing_keeps_the_active_tab() {
        let mut a = tabs(3);
        a.update(exited(PaneId::FIRST));
        assert_eq!((a.tab_count(), a.active_tab()), (2, 1));
        assert_eq!(a.focused(), pid(3));
        let mut b = tabs(3);
        press(&mut b, "b1");
        b.update(exited(id2()));
        assert_eq!((b.tab_count(), b.active_tab()), (2, 0));
        assert_eq!(b.focused(), PaneId::FIRST);
    }

    // Spec: Last pane of a tab with sibling tabs; Bar disappears on close.
    #[test]
    fn the_last_pane_of_a_tab_closes_the_tab_without_quitting() {
        let mut a = tabs(2);
        let effects = a.update(exited(id2()));
        assert_eq!((a.tab_count(), a.active_tab()), (1, 0));
        assert_eq!(a.screen().tab_bar, None);
        assert_eq!(a.screen().body.height, 24);
        assert_eq!(
            effects,
            vec![Effect::ResizePty(PaneId::FIRST, sized(24, 80))]
        );
    }

    // Spec: Last shell exits.
    #[test]
    fn the_last_pane_of_the_last_tab_quits() {
        let mut a = tabs(1);
        assert_eq!(a.update(exited(PaneId::FIRST)), vec![Effect::Quit]);
    }

    // Spec: Tab switch in RESIZE; Tab switch exits COPY.
    #[test]
    fn a_tab_change_exits_resize_and_copy() {
        let mut a = tabs(2);
        press(&mut a, "wr");
        assert_eq!(a.input(), InputMode::Resize);
        a.update(exited(id2()));
        assert_eq!(a.input(), InputMode::Terminal);
        let mut b = tabs(2);
        press(&mut b, "[");
        assert!(matches!(b.input(), InputMode::Copy(_)));
        b.update(exited(id2()));
        assert_eq!(b.input(), InputMode::Terminal);
    }

    // Spec: Failed new tab.
    #[test]
    fn a_failed_new_tab_is_removed_and_the_previous_tab_stays() {
        let mut a = tabs(1);
        press(&mut a, "tn");
        let effects = a.update(AppEvent::SpawnFailed(id2(), "boom".into()));
        assert_eq!((a.tab_count(), a.active_tab()), (1, 0));
        assert_eq!(a.focused(), PaneId::FIRST);
        assert_eq!(a.notice(), Some("spawn failed: boom"));
        assert_eq!(
            effects,
            vec![Effect::ResizePty(PaneId::FIRST, sized(24, 80))]
        );
        let mut b = tabs(3);
        b.update(AppEvent::SpawnFailed(pid(3), "x".into()));
        assert_eq!((b.tab_count(), b.active_tab()), (2, 1));
    }

    // Spec: Body math.
    #[test]
    fn the_body_starts_below_the_tab_bar_when_there_are_several_tabs() {
        let mut a = tabs(1);
        assert_eq!(a.screen().tab_bar, None);
        assert_eq!(
            a.screen().body,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 24
            }
        );
        press(&mut a, "tn");
        let bar = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 1,
        };
        assert_eq!(a.screen().tab_bar, Some(bar));
        assert_eq!(
            a.screen().body,
            Rect {
                x: 0,
                y: 1,
                width: 80,
                height: 23
            }
        );
    }

    // Spec: Resize touches panes of every tab.
    #[test]
    fn a_host_resize_resizes_the_panes_of_every_tab() {
        let mut a = tabs(2);
        let effects = a.update(AppEvent::Resize {
            cols: 100,
            rows: 30,
        });
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(28, 100)),
                Effect::ResizePty(id2(), sized(28, 100)),
            ]
        );
    }

    // Spec: Last pane of one of two tabs prompts close pane.
    #[test]
    fn the_last_pane_of_one_of_two_tabs_asks_to_close_the_pane() {
        let mut a = tabs(2);
        press(&mut a, "wq");
        assert_eq!(a.input(), InputMode::Confirm(Confirm::ClosePane));
        let effects = a.update(key('y'));
        assert_eq!((a.tab_count(), a.active_tab()), (1, 0));
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 80)),
                Effect::ClosePane(id2())
            ]
        );
    }
}
