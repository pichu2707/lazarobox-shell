use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::{
    ai::{
        Detection,
        chat::{
            ChatState, Follow, Phase,
            engine::{ChatRequest, StreamEvent},
            prompts::Resolved,
            store::{StoreCmd, StoreReply},
        },
        config::{AiConfig, Profile, Task},
        jail::JailSupport,
        memory::{
            offer::ServerOffer,
            port::{MemCmd, MemReply, Reason},
        },
    },
    config::{BarPosition, BarPositions, Config, ConfigEdit},
    copy::{
        CopyCommand, CopyMotion, CopyState, Geometry, Highlight, Point, SelectionKind, TooLarge,
        join, osc52,
    },
    keys::{encode_key, encode_paste},
    layout::{
        Axis, Direction, Node, PaneId, PaneIds, Rect, Removal, SplitError, Tiling, neighbour,
        pane_at, tile,
    },
    menu::{MenuCommand, MenuState},
    pane::{CursorKind, CursorShape, Pane, PaneSize},
    prefix::{self, Binding, Group, PREFIX_KEY, PREFIX_TREE, PrefixAction, Step},
    pty::{PtyEvent, SpawnSpec},
};

mod chat;
mod context;
mod notes;
mod offer;

/// Rows of history kept by the emulator.
const SCROLLBACK_LINES: usize = 10_000;

/// How long after leaving COPY a Press of the same key is taken as the key
/// still being held (OS auto-repeat arrives as plain Press events when the
/// terminal reports no event types). Trade-off: a deliberate retype of the
/// exit key within this window is dropped.
const HELD_EXIT_WINDOW: Duration = Duration::from_millis(500);

/// Shown when `Prefix [` is pressed on a chat pane.
const CHAT_COPY_REFUSED: &str =
    "copy mode is not available in chat panes; Ctrl+Y copies the last answer";

/// Shown when `Prefix n` (save a note) is pressed on a pane that is not a chat.
const NOTE_CHAT_ONLY: &str = "Save as note is for chat panes; open one with Ctrl+Space c";
/// Notice of the memory-line key on a pane that is not a chat.
const MEMORY_CHAT_ONLY: &str = "The memory line is for chat panes; open one with Ctrl+Space c";

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
    /// The config menu is open; `App.menu` holds its state (invariant:
    /// `menu.is_some() == (input == Menu)`).
    Menu,
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
            Self::Menu => "MENU",
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
    /// The runtime wrote the menu draft to `config.toml`.
    ConfigSaved,
    /// The save was refused or failed; carries the one-line footer text.
    ConfigSaveFailed(String),
    /// One engine detection run finished. An empty `Detection` means it failed.
    EnginesDetected(Detection),
    /// An event of a chat turn: the pane, the turn id it was started with, and
    /// what the engine task reported.
    Chat(PaneId, u64, StreamEvent),
    /// What the memory worker answered. It may arrive after its pane is gone.
    Memory(MemReply),
    /// The store thread answered a command of a pane.
    Store(StoreReply),
    /// The runtime prepared the system prompt of a chat pane (see
    /// `Effect::PreparePrompt`): the resolved prompt with the workspace
    /// block, and the notices of the override files it skipped.
    PromptReady(PaneId, Resolved),
    /// One step of the thinking-indicator animation. The runtime sends it
    /// only while a chat stream runs.
    AnimTick,
    /// Time passed while a chat waits for memory: the waits older than
    /// `MEMORY_WAIT` are given up. The runtime sends it only while
    /// `memory_waiting` holds.
    MemoryTick,
}

/// How the menu closed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    /// The draft stays as the live config.
    Saved,
    /// The config returns to the values captured at open.
    Reverted,
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
    /// Write the menu-owned keys to `config.toml`. The runtime answers with
    /// exactly one `ConfigSaved` or `ConfigSaveFailed`.
    SaveConfig(Vec<ConfigEdit>),
    /// Hand a finished OSC 52 sequence to the host terminal.
    SetClipboard(Vec<u8>),
    /// Probe the AI engines; the runtime answers with one `EnginesDetected`.
    DetectEngines,
    /// Start one chat turn for a pane. Events of the turn come back tagged
    /// with `turn`.
    ChatSend {
        pane: PaneId,
        turn: u64,
        request: ChatRequest,
    },
    /// Stop a pane's turn without blocking the loop: the socket is closed or
    /// the CLI's process group is killed.
    ChatCancel(PaneId),
    /// Ask the memory worker for something. Returns at once: the answer comes
    /// back as one `AppEvent::Memory`, or never when memory is not active.
    Memory(MemCmd),
    /// A command for the store thread. It never waits for the database.
    Store(StoreCmd),
    /// Resolve the system prompt of a conversation just started in `pane`:
    /// the override files of `profile` for the chat task, then the built-in,
    /// plus the workspace block of `origin` when `context` is set. Reads the
    /// disk off the UI thread; the runtime answers with one `PromptReady`.
    PreparePrompt {
        pane: PaneId,
        profile: Profile,
        origin: Option<PathBuf>,
        context: bool,
    },
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
    /// The tab bar is the outermost row of its edge and the statusline sits
    /// between it and the body. Rows beyond a tiny terminal are clipped by
    /// the UI; the body keeps at least 1x1.
    fn new(cols: u16, rows: u16, bar: bool, bars: BarPositions) -> Self {
        let size = pane_size(cols, rows);
        let bar_rows = u16::from(bar);
        let bar_top = bar && bars.tabbar == BarPosition::Top;
        let bar_bottom = bar && bars.tabbar == BarPosition::Bottom;
        let status_top = bars.statusline == BarPosition::Top;
        let top_rows = u16::from(bar_top) + u16::from(status_top);
        let from_bottom = |n: u16| rows.saturating_sub(n);
        let status_gap = u16::from(bar_bottom) + 1;
        Self {
            tab_bar: bar.then_some(Rect {
                x: 0,
                y: if bar_top { 0 } else { from_bottom(1) },
                width: size.cols,
                height: 1,
            }),
            body: Rect {
                x: 0,
                y: top_rows,
                width: size.cols,
                height: size.rows.saturating_sub(bar_rows).max(1),
            },
            status: Rect {
                x: 0,
                y: if status_top {
                    u16::from(bar_top)
                } else {
                    from_bottom(status_gap)
                },
                width: size.cols,
                height: 1,
            },
        }
    }
}

/// What a pane shows: a shell behind a PTY.
struct PtyPane {
    emu: Pane,
    /// The `/proc` poll result; the OSC 7 cwd, when seen, lives in `emu`.
    proc_cwd: Option<PathBuf>,
}

/// What a pane holds: a shell behind a PTY, or an AI chat that owns its
/// state and has no PTY.
enum PaneContent {
    Pty(Box<PtyPane>),
    Chat(Box<ChatState>),
}

/// What the UI sees of a pane.
pub enum PaneView<'a> {
    Pty(&'a Pane),
    Chat(&'a ChatState),
}

/// The kind of pane a split or a new tab creates.
enum NewPane {
    Shell,
    Chat(Box<ChatState>),
}

/// A pane's content and the size it was last laid out at (for a PTY pane, the
/// size its PTY was last told).
struct PaneState {
    content: PaneContent,
    size: PaneSize,
}

impl PaneState {
    fn new(size: PaneSize, host: Option<&str>) -> Self {
        Self {
            content: PaneContent::Pty(Box::new(PtyPane {
                emu: Pane::new(size, SCROLLBACK_LINES).with_host(host),
                proc_cwd: None,
            })),
            size,
        }
    }

    fn chat(size: PaneSize, state: ChatState) -> Self {
        Self {
            content: PaneContent::Chat(Box::new(state)),
            size,
        }
    }

    fn pty(&self) -> Option<&PtyPane> {
        match &self.content {
            PaneContent::Pty(pty) => Some(pty),
            PaneContent::Chat(_) => None,
        }
    }

    fn pty_mut(&mut self) -> Option<&mut PtyPane> {
        match &mut self.content {
            PaneContent::Pty(pty) => Some(pty),
            PaneContent::Chat(_) => None,
        }
    }

    fn chat_state(&self) -> Option<&ChatState> {
        match &self.content {
            PaneContent::Chat(chat) => Some(chat),
            PaneContent::Pty(_) => None,
        }
    }

    fn chat_mut(&mut self) -> Option<&mut ChatState> {
        match &mut self.content {
            PaneContent::Chat(chat) => Some(chat),
            PaneContent::Pty(_) => None,
        }
    }

    /// A shell's: OSC 7 wins once seen, and the poll fills in for shells that
    /// never report. A chat's is the cwd it was opened from.
    fn effective_cwd(&self) -> Option<&Path> {
        match &self.content {
            PaneContent::Pty(pty) => pty.emu.cwd().or(pty.proc_cwd.as_deref()),
            PaneContent::Chat(chat) => chat.origin_cwd(),
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
    /// The live config; while the menu is open it doubles as the draft.
    config: Config,
    menu: Option<MenuState>,
    /// Where the first pane starts, and the fallback for panes with no known cwd.
    launch_cwd: Option<PathBuf>,
    home: Option<PathBuf>,
    /// This machine's hostname: OSC 7 reports naming it count as local.
    hostname: Option<String>,
    /// Whether a directory exists. Injected so the core does no IO; the runtime
    /// passes the real check.
    dir_exists: fn(&Path) -> bool,
    shell: Option<String>,
    /// The key whose Press left COPY: its Repeat events are dropped in
    /// TERMINAL until its Release or any other Press, and so are its Presses
    /// arriving within `HELD_EXIT_WINDOW` of the previous such event.
    held_exit: Option<HeldExit>,
    /// The same debounce for the one-shot keys of a chat (cancel, yank,
    /// reasoning toggle): a terminal without key event types delivers a held
    /// key as Presses.
    held_chat_key: Option<HeldExit>,
    /// Monotonic clock, injected so `update` stays deterministic.
    clock: fn() -> Instant,
    /// The last finished detection; `None` until the first run ends.
    engines: Option<Detection>,
    /// A detection is in flight: further requests are dropped.
    detecting: bool,
    /// The saved AI choice; the menu edits a draft of it.
    ai: AiConfig,
    /// Whether this system can jail a CLI. Nothing probes it yet.
    jail: JailSupport,
    /// Chat panes whose send waits for the first detection result, with the
    /// text parked at Enter.
    waiting_sends: Vec<(PaneId, String)>,
    /// The frame of the thinking-indicator animation.
    anim_frame: usize,
    /// The one offer to start `engram serve` of this run, as the worker made
    /// it.
    server_offer: ServerOffer,
    /// Whether the offer is on screen (see `offer.rs`).
    offer_shown: bool,
    /// The chat the offer was shown in: the outcome of the start is told there.
    offer_pane: Option<PaneId>,
    /// Why no server can be offered (no usable `engram` on the PATH), as the
    /// worker said once: it names the cause of every `engram is not running`.
    no_server: Option<Reason>,
}

/// The key that left COPY and when it was last seen.
#[derive(Debug, Clone, Copy)]
struct HeldExit {
    code: KeyCode,
    modifiers: KeyModifiers,
    last: Instant,
}

impl App {
    /// An app for a terminal of `cols` x `rows`, with one pane in the body.
    pub fn new(cols: u16, rows: u16) -> Self {
        let config = Config::default();
        let screen = ScreenLayout::new(cols, rows, false, config.bars);
        let mut ids = PaneIds::default();
        let first = ids.alloc();
        Self {
            panes: HashMap::from([(first, PaneState::new(body_size(screen.body), None))]),
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
            config,
            menu: None,
            launch_cwd: None,
            home: None,
            hostname: None,
            dir_exists: |_| true,
            shell: None,
            held_exit: None,
            held_chat_key: None,
            clock: Instant::now,
            engines: None,
            detecting: false,
            ai: AiConfig::default(),
            jail: JailSupport::Unavailable("no CLI jail yet"),
            waiting_sends: Vec::new(),
            anim_frame: 0,
            server_offer: ServerOffer::new(),
            offer_shown: false,
            offer_pane: None,
            no_server: None,
        }
    }

    /// Starts with the AI choice saved in `config.toml`.
    pub fn with_ai(mut self, ai: AiConfig) -> Self {
        self.ai = ai;
        self
    }

    /// The saved AI choice.
    pub fn ai(&self) -> &AiConfig {
        &self.ai
    }

    /// The last finished AI engine detection, if any.
    pub fn engines(&self) -> Option<&Detection> {
        self.engines.as_ref()
    }

    /// Asks the runtime to detect the AI engines, unless a run is already
    /// in flight (single flight: that run is at most a couple of seconds old).
    pub fn request_detection(&mut self) -> Vec<Effect> {
        if self.detecting {
            return Vec::new();
        }
        self.detecting = true;
        vec![Effect::DetectEngines]
    }

    /// Sets the facts the statusline shows. They are inputs, not lookups, so
    /// `update` stays free of IO.
    pub fn with_env(mut self, shell: Option<&str>, home: Option<PathBuf>, cwd: PathBuf) -> Self {
        self.shell = shell.map(str::to_owned);
        self.home = home;
        if let Some(pty) = self.focused_state_mut().pty_mut() {
            pty.proc_cwd = Some(cwd.clone());
        }
        self.launch_cwd = Some(cwd);
        self
    }

    /// Injects the directory-existence check used when a new pane or tab picks
    /// the cwd it starts in.
    pub fn with_dir_check(mut self, dir_exists: fn(&Path) -> bool) -> Self {
        self.dir_exists = dir_exists;
        self
    }

    /// Injects the monotonic clock used to debounce a held COPY exit key.
    pub fn with_clock(mut self, clock: fn() -> Instant) -> Self {
        self.clock = clock;
        self
    }

    /// Names this machine so OSC 7 reports for it are accepted.
    pub fn with_hostname(mut self, host: Option<&str>) -> Self {
        self.hostname = host.map(str::to_owned);
        for pty in self.panes.values_mut().filter_map(PaneState::pty_mut) {
            pty.emu.set_host(host);
        }
        self
    }

    /// Places the statusline and the tab bar as the config says.
    pub fn with_config(mut self, config: &Config) -> Self {
        self.config = *config;
        self.refresh_screen();
        // Nothing runs yet, so there is no PTY to tell about the new size.
        self.relayout();
        self
    }

    /// Shows `notice` in the statusline path segment until the next key.
    pub fn with_notice(mut self, notice: Option<String>) -> Self {
        self.notice = notice;
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

    /// What the right-hand statusline segment shows: the chat engine over a
    /// focused chat pane, the shell name otherwise.
    pub fn status_right(&self) -> String {
        if !self.focused_is_chat() {
            return self.shell_name().to_owned();
        }
        let (assignment, _) = self.ai.resolve(Task::Chat);
        match (assignment.engine, assignment.model) {
            (None, _) => "no engine".to_owned(),
            (Some(engine), Some(model)) if !model.is_empty() => format!("{engine} › {model}"),
            (Some(engine), _) => engine,
        }
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

    /// The open menu, if any.
    pub fn menu(&self) -> Option<&MenuState> {
        self.menu.as_ref()
    }

    /// The live config (the draft while the menu is open).
    pub fn config(&self) -> &Config {
        &self.config
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
        let total = self.focused_pty()?.emu.scrollback_len();
        (total > 0).then_some((state.offset, total))
    }

    /// COPY cursor cell `(row, col)` inside the focused pane's viewport;
    /// `None` outside COPY and when the cursor is scrolled off screen.
    pub fn copy_cursor(&self) -> Option<(u16, u16)> {
        let InputMode::Copy(state) = &self.input else {
            return None;
        };
        let geo = self.copy_geometry();
        let top = (geo.first_line + geo.history).checked_sub(state.offset)?;
        let row = state.cursor.line.checked_sub(top)?;
        (row < usize::from(geo.rows)).then_some((row as u16, state.cursor.col))
    }

    /// The cells to paint as selected in the focused pane; `None` outside COPY
    /// and without a selection.
    pub fn copy_highlight(&self) -> Option<Highlight> {
        let InputMode::Copy(state) = &self.input else {
            return None;
        };
        let geo = self.copy_geometry();
        Some(Highlight {
            span: state.span(geo.cols)?,
            top_line: (geo.first_line + geo.history).saturating_sub(state.offset),
        })
    }

    /// The kind of the active selection, for the statusline marker.
    pub fn copy_selection_kind(&self) -> Option<SelectionKind> {
        let InputMode::Copy(state) = &self.input else {
            return None;
        };
        state.selection.map(|s| s.kind)
    }

    /// What the focused pane offers to copy mode.
    fn copy_geometry(&self) -> Geometry {
        let pane = self.copy_pane();
        let size = self.focused_size();
        Geometry {
            first_line: pane.first_line(),
            history: pane.scrollback_len(),
            rows: size.rows,
            cols: size.cols,
        }
    }

    /// Fresh COPY state with the cursor on the focused pane's live cursor.
    fn enter_copy(&self) -> CopyState {
        let pane = self.copy_pane();
        CopyState::enter(
            self.copy_geometry(),
            pane.live_cursor(),
            pane.line_generation(),
        )
    }

    /// How many tabs are open.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Index of the active tab.
    pub fn active_tab(&self) -> usize {
        self.active
    }

    /// One label per tab: `N cwd-basename`, or just `N` without a cwd.
    pub fn tab_labels(&self) -> Vec<String> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let number = index + 1;
                match self.panes[&tab.focus].effective_cwd() {
                    Some(cwd) => {
                        let name = cwd.file_name().map_or_else(
                            || "/".to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        format!("{number} {name}")
                    }
                    None => number.to_string(),
                }
            })
            .collect()
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
        self.panes
            .get(&id)
            .and_then(PaneState::pty)
            .map(|pty| &pty.emu)
    }

    /// The focused pane's PTY side, `None` when it is not a shell pane.
    fn focused_pty(&self) -> Option<&PtyPane> {
        self.focused_state().pty()
    }

    /// The emulator COPY works on. COPY is only entered on a shell pane and
    /// ends on every focus change, so the focus is always a PTY pane here.
    fn copy_pane(&self) -> &Pane {
        &self.focused_pty().expect("COPY runs on a shell pane").emu
    }

    fn copy_pane_mut(&mut self) -> &mut Pane {
        &mut self
            .focused_state_mut()
            .pty_mut()
            .expect("COPY runs on a shell pane")
            .emu
    }

    #[cfg(test)]
    fn focused_pane(&self) -> &Pane {
        self.copy_pane()
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

    /// The cwd a new pane or tab starts in: the focused pane's effective cwd,
    /// then the launch cwd, then `$HOME`, taking the first that still exists.
    /// `None` when none does (the spawn then falls back to `/`).
    fn inherited_cwd(&self) -> Option<PathBuf> {
        let focused = self.focused_state().effective_cwd();
        let home = self.home.as_deref().filter(|home| home.is_absolute());
        [focused, self.launch_cwd.as_deref(), home]
            .into_iter()
            .flatten()
            .find(|dir| (self.dir_exists)(dir))
            .map(Path::to_path_buf)
    }

    fn spawn_spec(&self, cwd: Option<PathBuf>, size: PaneSize) -> SpawnSpec {
        let cwd = cwd.unwrap_or_else(|| "/".into());
        SpawnSpec::new(self.shell.as_deref(), cwd, size)
    }

    /// The child's cwd for the statusline, with `$HOME` shown as `~`. Empty
    /// until the first cwd is known.
    pub fn cwd_label(&self) -> String {
        let Some(cwd) = self.focused_state().effective_cwd() else {
            return String::new();
        };
        let home = self.home.as_deref().filter(|home| home.parent().is_some());
        match home.and_then(|home| cwd.strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.display()),
            None => cwd.display().to_string(),
        }
    }

    /// Test-only: opens a chat pane to the right of the focus.
    #[cfg(test)]
    pub(crate) fn open_chat(&mut self, origin: Option<PathBuf>) -> Vec<Effect> {
        self.split_with(Axis::X, NewPane::Chat(Box::new(ChatState::new(origin))))
    }

    /// Test-only: sends `text` from chat pane `id` to a ready ollama engine.
    #[cfg(test)]
    pub(crate) fn chat_send(&mut self, id: PaneId, text: &str) -> Vec<Effect> {
        use crate::core::ai::chat::{SendOutcome, engine::Availability};
        let ready = Availability::Ready {
            engine: "ollama".into(),
            model: Some("llama3:8b".into()),
        };
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match chat.send(text, &ready, "") {
            SendOutcome::Sent { turn, request } => {
                self.dirty = true;
                vec![Effect::ChatSend {
                    pane: id,
                    turn,
                    request,
                }]
            }
            SendOutcome::Refused => Vec::new(),
        }
    }

    /// Test-only: types `text` into the composer of chat pane `id`.
    #[cfg(test)]
    pub(crate) fn chat_type(&mut self, id: PaneId, text: &str) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.composer_mut().paste(text);
            self.dirty = true;
        }
    }

    /// Test-only: cancels the turn of chat pane `id`.
    #[cfg(test)]
    pub(crate) fn chat_cancel(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match chat.cancel() {
            Follow::CancelEngine => {
                self.dirty = true;
                vec![Effect::ChatCancel(id)]
            }
            Follow::Nothing => Vec::new(),
        }
    }

    /// Test-only: opens a chat pane as a new tab.
    #[cfg(test)]
    fn open_chat_tab(&mut self, origin: Option<PathBuf>) -> Vec<Effect> {
        self.new_tab_with(NewPane::Chat(Box::new(ChatState::new(origin))))
    }

    /// What the UI draws for pane `id`, if it exists.
    pub fn pane_view(&self, id: PaneId) -> Option<PaneView<'_>> {
        self.panes.get(&id).map(|state| match &state.content {
            PaneContent::Pty(pty) => PaneView::Pty(&pty.emu),
            PaneContent::Chat(chat) => PaneView::Chat(chat),
        })
    }

    /// COPY works on an emulator, so it must never outlive the focus on a
    /// shell pane: `copy_pane` relies on it.
    fn debug_assert_copy_on_shell(&self) {
        debug_assert!(
            !matches!(self.input, InputMode::Copy(_)) || self.focused_pty().is_some(),
            "COPY is only active while a shell pane has the focus"
        );
    }

    /// Whether `id` names a pane with a PTY behind it.
    fn is_shell(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .is_some_and(|state| state.pty().is_some())
    }

    pub fn update(&mut self, event: AppEvent) -> Vec<Effect> {
        let effects = match event {
            AppEvent::Key(key) => self.on_key(key),
            AppEvent::Paste(text) => self.on_paste(&text),
            AppEvent::Resize { cols, rows } => self.on_resize(cols, rows),
            // Events for panes that no longer exist are dropped.
            // A chat pane has no PTY, so no PTY event is meant for it either.
            AppEvent::Pty(id, event) if self.is_shell(id) => self.on_pty(id, event),
            AppEvent::Cwd(id, cwd) if self.is_shell(id) => self.on_cwd(id, cwd),
            AppEvent::Pty(..) | AppEvent::Cwd(..) => Vec::new(),
            AppEvent::SpawnFailed(id, error) if self.is_shell(id) => {
                let effects = self.remove_pane(id);
                self.notice = Some(format!("spawn failed: {error}"));
                effects
            }
            AppEvent::SpawnFailed(..) => Vec::new(),
            // Results of a save are stale when the menu is gone.
            AppEvent::ConfigSaved if self.menu.is_some() => {
                self.dirty = true;
                self.close_menu(Outcome::Saved)
            }
            AppEvent::ConfigSaveFailed(message) => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.set_error(message);
                    self.dirty = true;
                }
                Vec::new()
            }
            AppEvent::ConfigSaved => Vec::new(),
            // Even an empty (failed) result ends the run, so the triggers re-arm.
            AppEvent::EnginesDetected(detection) => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.set_engines(Some(detection.clone()));
                }
                self.engines = Some(detection);
                self.detecting = false;
                self.dirty = true;
                self.run_waiting_sends()
            }
            AppEvent::Chat(id, turn, event) => self.on_chat(id, turn, event),
            AppEvent::Memory(reply) => self.on_memory_reply(&reply),
            AppEvent::Store(reply) => self.on_store_reply(&reply),
            AppEvent::PromptReady(id, resolved) => self.on_prompt_ready(id, resolved),
            AppEvent::AnimTick => self.on_anim_tick(),
            AppEvent::MemoryTick => self.on_memory_tick(),
        };
        debug_assert_eq!(
            self.menu.is_some(),
            self.input == InputMode::Menu,
            "the menu state and the MENU mode go together"
        );
        self.debug_assert_copy_on_shell();
        effects
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
            // A chat has no child to ask: its composer is an editor, so a bar.
            _ if self.focused_state().chat_state().is_some() => CursorShape {
                kind: CursorKind::Bar,
                blinking: true,
            },
            _ => self
                .focused_pty()
                .map(|pty| pty.emu.cursor_shape())
                .unwrap_or_default(),
        }
    }

    /// Release is always ignored. Repeat acts like Press where holding a key
    /// is meaningful (TERMINAL typing, COPY scrolling) and is dropped where a
    /// held key could trigger a one-shot decision (PREFIX, GROUP, CONFIRM).
    fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let repeat = match key.kind {
            KeyEventKind::Press => false,
            KeyEventKind::Repeat => true,
            KeyEventKind::Release => {
                if self.held_exit.is_some_and(|h| h.code == key.code) {
                    self.held_exit = None;
                }
                if self.held_chat_key.is_some_and(|h| h.code == key.code) {
                    self.held_chat_key = None;
                }
                return Vec::new();
            }
        };
        if repeat {
            // A held key that left COPY must not reach the shell as typed text.
            if self.input == InputMode::Terminal
                && self.held_exit.is_some_and(|h| h.code == key.code)
            {
                return Vec::new();
            }
        } else if let Some(held) = self.held_exit.take() {
            // Terminals that report no event types deliver auto-repeat as
            // Presses: the same key within the window is still the hold.
            let now = (self.clock)();
            let same = held.code == key.code && held.modifiers == key.modifiers;
            // `held_exit` is only armed on entering TERMINAL and every Press
            // takes it, so the mode is always TERMINAL here.
            debug_assert_eq!(self.input, InputMode::Terminal);
            if same && now.saturating_duration_since(held.last) <= HELD_EXIT_WINDOW {
                self.held_exit = Some(HeldExit { last: now, ..held });
                return Vec::new();
            }
        }
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
            InputMode::Terminal => self.on_terminal_key(key, repeat),
            InputMode::Prefix => self.on_pending_key(PREFIX_TREE, &key),
            InputMode::Group(group) => self.on_pending_key(group.bindings, &key),
            InputMode::Copy(state) => self.on_copy_key(state, &key, repeat),
            InputMode::Resize => self.on_resize_key(&key),
            InputMode::Confirm(confirm) => self.on_confirm_key(confirm, &key),
            InputMode::Menu => self.on_menu_key(&key, repeat),
        }
    }

    /// Opens the menu: captures the live config as the original.
    /// The forced tab bar may resize the panes.
    fn open_menu(&mut self) -> Vec<Effect> {
        self.menu =
            Some(MenuState::open(&self.config).with_ai(self.ai.clone(), self.engines.clone()));
        self.input = InputMode::Menu;
        let mut effects = self.apply_config();
        effects.extend(self.request_detection());
        self.sync_checking();
        effects
    }

    /// Mirrors the run in flight into the menu's `refreshing…` cue. A result
    /// clears it through `set_engines`.
    fn sync_checking(&mut self) {
        if let Some(menu) = self.menu.as_mut() {
            menu.set_checking(self.detecting);
        }
    }

    /// The only way out of MENU. `Reverted` restores the config captured at
    /// open; `Saved` keeps the draft.
    fn close_menu(&mut self, outcome: Outcome) -> Vec<Effect> {
        if let Some(menu) = self.menu.take() {
            match outcome {
                Outcome::Reverted => self.config = menu.original(),
                Outcome::Saved => self.ai = menu.ai_draft(),
            }
            if !self.ai.memory.active() {
                self.memory_turned_off();
            } else {
                self.sync_memory_lines();
            }
        }
        self.input = InputMode::Terminal;
        // The forced tab bar goes away with the menu, whatever the outcome.
        self.apply_config()
    }

    /// Lays the screen out again for the live config, telling only the panes
    /// whose size changed.
    fn apply_config(&mut self) -> Vec<Effect> {
        self.refresh_screen();
        self.relayout()
    }

    fn on_menu_key(&mut self, key: &KeyEvent, repeat: bool) -> Vec<Effect> {
        let Some(menu) = self.menu.as_mut() else {
            debug_assert!(false, "MENU mode without menu state");
            return Vec::new();
        };
        menu.set_term(self.term.0, self.term.1);
        match menu.on_key(key, repeat, &mut self.config) {
            MenuCommand::Changed => self.apply_config(),
            MenuCommand::Cancel => self.close_menu(Outcome::Reverted),
            MenuCommand::Save => self.request_save(),
            MenuCommand::Refresh => {
                let effects = self.request_detection();
                self.sync_checking();
                effects
            }
            MenuCommand::None => Vec::new(),
        }
    }

    /// An unchanged draft has nothing to save and closes at once. A changed
    /// one asks the runtime to write it; the menu stays open until the
    /// answer (`ConfigSaved` or `ConfigSaveFailed`) arrives.
    fn request_save(&mut self) -> Vec<Effect> {
        let edits = self
            .menu
            .as_ref()
            .map(|menu| menu.edits(&self.config))
            .unwrap_or_default();
        if edits.is_empty() {
            self.close_menu(Outcome::Saved)
        } else {
            vec![Effect::SaveConfig(edits)]
        }
    }

    fn on_terminal_key(&mut self, key: KeyEvent, repeat: bool) -> Vec<Effect> {
        if PREFIX_KEY.matches(&key) {
            self.input = InputMode::Prefix;
            return Vec::new();
        }
        if self.focused_is_chat() {
            return self.on_chat_key(key, repeat);
        }
        let Some(pty) = self.focused_pty() else {
            return Vec::new();
        };
        encode_key(key, pty.emu.modes())
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
            // A chat has no scrollback to select from: COPY is for shell panes.
            Step::Run(PrefixAction::EnterCopy) if self.focused_pty().is_none() => {
                self.notice = Some(CHAT_COPY_REFUSED.to_string());
                InputMode::Terminal
            }
            Step::Run(PrefixAction::EnterCopy) => InputMode::Copy(self.enter_copy()),
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
                | PrefixAction::Focus(_)
                | PrefixAction::OpenMenu
                | PrefixAction::OpenChat
                | PrefixAction::SaveNote
                | PrefixAction::ToggleMemory,
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
            // The literal is a byte for a child; a chat has none, so it swallows it.
            Step::Run(PrefixAction::SendPrefixLiteral) if self.focused_pty().is_some() => {
                vec![Effect::WritePty(self.tab().focus, vec![PREFIX_LITERAL])]
            }
            Step::Run(PrefixAction::OpenMenu) => self.open_menu(),
            Step::Run(PrefixAction::OpenChat) => self.open_chat_pane(),
            Step::Run(PrefixAction::SaveNote) if self.focused_is_chat() => {
                let id = self.tab().focus;
                self.chat_note_key(id)
            }
            Step::Run(PrefixAction::SaveNote) => {
                self.notice = Some(NOTE_CHAT_ONLY.to_string());
                Vec::new()
            }
            Step::Run(PrefixAction::ToggleMemory) if self.focused_is_chat() => {
                self.chat_toggle_memory()
            }
            Step::Run(PrefixAction::ToggleMemory) => {
                self.notice = Some(MEMORY_CHAT_ONLY.to_string());
                Vec::new()
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
        let left = self.tabs[self.active].focus;
        if let Some(pty) = self.panes.get_mut(&left).and_then(PaneState::pty_mut) {
            pty.emu.set_scrollback(0);
        }
        if matches!(self.input, InputMode::Copy(_) | InputMode::Resize) {
            self.input = InputMode::Terminal;
        }
        self.active = index;
        self.debug_assert_copy_on_shell();
    }

    /// Appends a tab with one pane, activates it and spawns its shell in the
    /// focused pane's cwd. The tab bar may appear, which resizes every tab.
    fn new_tab(&mut self) -> Vec<Effect> {
        self.new_tab_with(NewPane::Shell)
    }

    /// `new_tab` for any kind of pane; only a shell is spawned.
    fn new_tab_with(&mut self, kind: NewPane) -> Vec<Effect> {
        let cwd = self.inherited_cwd();
        let id = self.ids.alloc();
        self.tabs.push(Tab {
            tree: Node::Leaf(id),
            focus: id,
            zoom: false,
        });
        self.activate(self.tabs.len() - 1);
        self.refresh_screen();
        let size = body_size(self.screen.body);
        let state = self.new_state(kind, size);
        let spawn = matches!(state.content, PaneContent::Pty(_));
        self.panes.insert(id, state);
        let mut effects = self.relayout();
        if spawn {
            effects.push(Effect::SpawnPane(id, self.spawn_spec(cwd, size)));
        }
        effects
    }

    /// Drops tab `index` and its panes. The tab that takes its index becomes
    /// active (the last one if it was last); closing a tab before the active
    /// one keeps the active tab. The bar may disappear, which resizes every tab.
    fn drop_tab(&mut self, index: usize) -> Vec<Effect> {
        let leaves = self.tabs.remove(index).tree.leaves();
        let mut effects = self.end_effects(&leaves);
        for id in leaves {
            self.panes.remove(&id);
            self.drop_parked_send(id);
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
        effects.extend(self.relayout());
        effects
    }

    /// Recomputes the screen rects: the tab bar exists while there is more
    /// than one tab, and while the menu is open so the preview shows the
    /// real geometry (CM-8).
    fn refresh_screen(&mut self) {
        self.screen = ScreenLayout::new(
            self.term.0,
            self.term.1,
            self.tabs.len() > 1 || self.menu.is_some(),
            self.config.bars,
        );
    }

    /// Splits the focused pane and focuses the new one. A split the layout
    /// refuses (too small) changes nothing.
    fn split(&mut self, axis: Axis) -> Vec<Effect> {
        self.split_with(axis, NewPane::Shell)
    }

    /// `split` for any kind of pane; only a shell is spawned.
    fn split_with(&mut self, axis: Axis, kind: NewPane) -> Vec<Effect> {
        self.tab_mut().zoom = false;
        let target = self.tab().focus;
        let new = self.ids.alloc();
        let body = self.screen.body;
        match self.tab_mut().tree.split(body, target, new, axis) {
            Ok(()) => {}
            Err(SplitError::TooSmall | SplitError::NotFound) => return self.relayout(),
        }
        let cwd = self.inherited_cwd();
        let size = self.layout_sizes()[&new];
        let state = self.new_state(kind, size);
        let spawn = matches!(state.content, PaneContent::Pty(_));
        self.panes.insert(new, state);
        self.set_focus(new);
        let mut effects = self.relayout();
        if spawn {
            effects.push(Effect::SpawnPane(new, self.spawn_spec(cwd, size)));
        }
        effects
    }

    fn new_state(&self, kind: NewPane, size: PaneSize) -> PaneState {
        match kind {
            NewPane::Shell => PaneState::new(size, self.hostname.as_deref()),
            NewPane::Chat(chat) => PaneState::chat(size, *chat),
        }
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
        if let Some(pty) = self.panes.get_mut(&left).and_then(PaneState::pty_mut) {
            pty.emu.set_scrollback(0);
        }
        if t == self.active && matches!(self.input, InputMode::Copy(_) | InputMode::Resize) {
            self.input = InputMode::Terminal;
        }
        self.tabs[t].focus = next;
        self.debug_assert_copy_on_shell();
    }

    /// Applies one event of a chat turn. Events for a pane that is gone, is a
    /// shell, or belongs to another turn change nothing and draw nothing.
    fn on_chat(&mut self, id: PaneId, turn: u64, event: StreamEvent) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let live = turn == chat.turn() && chat.phase() != Phase::Idle;
        let follow = chat.apply(turn, event);
        self.dirty |= live;
        match follow {
            // The state ended the turn itself (answer cap): stop the engine.
            Follow::CancelEngine => vec![Effect::ChatCancel(id)],
            Follow::Nothing => Vec::new(),
        }
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
            Removal::WasLast if self.tabs.len() == 1 => return self.quit_effects(),
            Removal::WasLast => self.drop_tab(t),
            Removal::Removed => {
                let mut ends = self.end_effects(&[id]);
                self.panes.remove(&id);
                self.drop_parked_send(id);
                self.tabs[t].zoom = false;
                if id == self.tabs[t].focus {
                    let tiling = tile(&self.tabs[t].tree, body);
                    let next =
                        pane_at(&tiling, old.x, old.y).unwrap_or(self.tabs[t].tree.leaves()[0]);
                    self.set_focus_in(t, next);
                }
                ends.extend(self.relayout());
                ends
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
        let mut rewrapped = Vec::new();
        for tab in &self.tabs {
            for (id, rect) in tab.tiling(self.screen.body).panes {
                let size = body_size(rect);
                let Some(state) = self.panes.get_mut(&id) else {
                    continue;
                };
                if state.size != size {
                    let old = state.size;
                    state.size = size;
                    if let Some(pty) = state.pty_mut() {
                        pty.emu.resize(size);
                        effects.push(Effect::ResizePty(id, size));
                    }
                    if state.chat_state().is_some() {
                        rewrapped.push((id, old.cols));
                    }
                }
            }
        }
        for (id, old_cols) in rewrapped {
            // A scrolled view stays on the same content, at the height that
            // is really drawn (composer and guidance taken off).
            let rows = self.chat_viewport(id);
            let cols = self.panes.get(&id).map_or(0, |state| state.size.cols);
            if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                chat.rewrap(usize::from(old_cols), usize::from(cols), rows);
            }
        }
        self.sync_copy();
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
            Confirm::Quit => self.quit_effects(),
            Confirm::CloseTab => self.close_tab(),
            Confirm::ClosePane => {
                let id = self.tab().focus;
                let stop = self.stop_effect(id);
                let mut effects = self.remove_pane(id);
                effects.push(stop);
                effects
            }
        }
    }

    /// Closes the active tab with all its panes; the only tab quits instead.
    fn close_tab(&mut self) -> Vec<Effect> {
        if self.tabs.len() == 1 {
            return self.quit_effects();
        }
        let stops: Vec<_> = (self.tab().tree.leaves().into_iter())
            .map(|id| self.stop_effect(id))
            .collect();
        let mut effects = self.drop_tab(self.active);
        effects.extend(stops);
        effects
    }

    /// What ends the work behind pane `id` when it closes: a shell's child is
    /// terminated, a chat's turn is cancelled. A chat has no child, so it
    /// never gets `ClosePane`.
    fn stop_effect(&self, id: PaneId) -> Effect {
        if self.is_shell(id) {
            Effect::ClosePane(id)
        } else {
            Effect::ChatCancel(id)
        }
    }

    fn on_copy_key(&mut self, mut state: CopyState, key: &KeyEvent, repeat: bool) -> Vec<Effect> {
        match state.on_key(key, repeat) {
            CopyCommand::Move(motion) => {
                let geo = self.copy_geometry();
                let pane = self.copy_pane_mut();
                let line_end = match motion {
                    CopyMotion::LineEnd => pane.line_end(state.cursor.line),
                    _ => 0,
                };
                state.apply_motion(motion, geo, line_end);
                pane.set_scrollback(state.offset);
                self.input = InputMode::Copy(state);
            }
            CopyCommand::Exit => {
                self.copy_pane_mut().set_scrollback(0);
                self.input = InputMode::Terminal;
                self.hold_exit(key);
            }
            CopyCommand::Yank => {
                let effects = self.yank(state);
                if self.input == InputMode::Terminal {
                    self.hold_exit(key);
                }
                return effects;
            }
            CopyCommand::Ignore => self.input = InputMode::Copy(state),
        }
        Vec::new()
    }

    fn hold_exit(&mut self, key: &KeyEvent) {
        self.held_exit = Some(HeldExit {
            code: key.code,
            modifiers: key.modifiers,
            last: (self.clock)(),
        });
    }

    /// `y`: reads the selection, hands it to the host as OSC 52 and leaves
    /// COPY. A selection over the cap, or one with no text, sends nothing and keeps
    /// COPY open.
    fn yank(&mut self, state: CopyState) -> Vec<Effect> {
        let cols = self.copy_geometry().cols;
        let Some(span) = state.span(cols) else {
            self.input = InputMode::Copy(state);
            return Vec::new();
        };
        let pane = self.copy_pane_mut();
        let text = join(&pane.read_lines(span));
        if text.is_empty() {
            // An empty OSC 52 would clobber the clipboard.
            self.notice = Some("Nothing to copy".to_string());
            self.input = InputMode::Copy(state);
            return Vec::new();
        }
        match osc52(&text) {
            Ok(sequence) => {
                pane.set_scrollback(0);
                let count = text.chars().count();
                self.notice = Some(match count {
                    1 => "Copied 1 character".to_string(),
                    n => format!("Copied {n} characters"),
                });
                self.input = InputMode::Terminal;
                vec![Effect::SetClipboard(sequence)]
            }
            Err(TooLarge) => {
                self.notice = Some("Selection too large to copy".to_string());
                self.input = InputMode::Copy(state);
                Vec::new()
            }
        }
    }

    fn on_paste(&mut self, text: &str) -> Vec<Effect> {
        // A paste is not part of a held key: the debounce is over.
        self.held_exit = None;
        if self.input == InputMode::Terminal && self.focused_is_chat() {
            self.chat_paste(text);
            return Vec::new();
        }
        match self.input {
            InputMode::Terminal => self
                .focused_pty()
                .map(|pty| {
                    vec![Effect::WritePty(
                        self.tab().focus,
                        encode_paste(text, pty.emu.modes()),
                    )]
                })
                .unwrap_or_default(),
            InputMode::Menu => {
                // Only the custom field takes a paste; nothing reaches a PTY.
                if let Some(menu) = self.menu.as_mut() {
                    menu.on_paste(text);
                    self.dirty = true;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn on_resize(&mut self, cols: u16, rows: u16) -> Vec<Effect> {
        self.term = (cols, rows);
        self.refresh_screen();
        let effects = self.relayout();
        self.dirty = true;
        effects
    }

    /// Revalidates COPY against its pane after output or a relayout. The pane
    /// is the source of truth for the offset (the emulator pins the viewport
    /// and clamps it at the history cap). A new line generation (screen flip,
    /// resize) makes line numbers incomparable, so COPY starts over as when
    /// entered: no selection, cursor on the live cursor, viewport at the
    /// bottom. The cursor and the anchor are clamped into the retained lines;
    /// an evicted anchor becomes `(first_line, 0)`, shrinking the selection.
    /// The anchor needs no upper clamp: `first_line + history` never shrinks
    /// and the row count only changes with a generation bump.
    fn sync_copy(&mut self) {
        let InputMode::Copy(mut state) = self.input else {
            return;
        };
        let geo = self.copy_geometry();
        if state.generation != self.copy_pane().line_generation() {
            // Line numbers are incomparable now: start over like entering COPY.
            state = self.enter_copy();
            self.copy_pane_mut().set_scrollback(0);
        }
        let pane = self.copy_pane();
        state.offset = pane.scrollback_offset();
        let last_line = geo.first_line + geo.history + usize::from(geo.rows.max(1)) - 1;
        let last_col = geo.cols.saturating_sub(1);
        state.cursor.line = state.cursor.line.clamp(geo.first_line, last_line);
        state.cursor.col = state.cursor.col.min(last_col);
        if let Some(selection) = &mut state.selection {
            let anchor = &mut selection.anchor;
            if anchor.line < geo.first_line {
                *anchor = Point {
                    line: geo.first_line,
                    col: 0,
                };
            }
            anchor.col = anchor.col.min(last_col);
        }
        self.input = InputMode::Copy(state);
    }

    fn on_cwd(&mut self, id: PaneId, cwd: PathBuf) -> Vec<Effect> {
        if let Some(state) = self.panes.get_mut(&id) {
            let before = state.effective_cwd().map(Path::to_path_buf);
            if let Some(pty) = state.pty_mut() {
                pty.proc_cwd = Some(cwd);
            }
            if state.effective_cwd() != before.as_deref() {
                self.mark_cwd_dirty(id);
            }
        }
        Vec::new()
    }

    /// Redraws when a pane's effective cwd changed and it is on screen: the
    /// statusline shows the focused pane's, and the tab bar every tab's.
    fn mark_cwd_dirty(&mut self, id: PaneId) {
        let labelled = self.tabs.len() > 1 && self.tabs.iter().any(|tab| tab.focus == id);
        self.dirty |= labelled || id == self.tab().focus;
    }

    fn on_pty(&mut self, id: PaneId, event: PtyEvent) -> Vec<Effect> {
        match event {
            PtyEvent::Output(bytes) => {
                // Panes of hidden tabs are not on screen: feed them, don't redraw.
                self.dirty |= self.tab().tree.leaves().contains(&id);
                let mut cwd_changed = false;
                let reply = self
                    .panes
                    .get_mut(&id)
                    .map(|state| {
                        let before = state.effective_cwd().map(Path::to_path_buf);
                        let reply = state
                            .pty_mut()
                            .map(|pty| pty.emu.feed(&bytes))
                            .unwrap_or_default();
                        cwd_changed = state.effective_cwd() != before.as_deref();
                        reply
                    })
                    .unwrap_or_default();
                if cwd_changed {
                    self.mark_cwd_dirty(id);
                }
                if id == self.tab().focus {
                    self.sync_copy();
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
        copy::{CopyState, Point, SelectionKind},
        menu::Row,
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

    /// Presses `c` `n` times. From the bottom row of a 24-row pane, scrolling
    /// the viewport by `n` rows takes `23 + n` presses of `k`.
    fn press_n(a: &mut App, c: char, n: usize) {
        for _ in 0..n {
            a.update(key(c));
        }
    }

    fn ctrl_d() -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL))
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

    // Memory replies come from a worker the app never waits on, so a reply
    // can be stale: the pane it was for may be gone by then.
    #[test]
    fn a_memory_reply_for_a_pane_that_is_gone_is_dropped() {
        use crate::core::ai::memory::{
            port::{MemReply, Reason},
            reply::ProjectError,
        };
        let mut a = app();
        a.take_dirty();
        let gone = PaneId::for_test(99);
        let replies = [
            MemReply::Project {
                pane: gone,
                result: Err(ProjectError::NotGit),
            },
            MemReply::Context {
                pane: gone,
                block: None,
            },
            MemReply::Unavailable {
                pane: Some(gone),
                reason: Reason::Busy,
            },
        ];
        for reply in replies {
            assert_eq!(a.update(AppEvent::Memory(reply)), vec![]);
            assert!(!a.take_dirty(), "a stale reply changes nothing");
        }
    }

    #[test]
    fn a_global_memory_reply_asks_for_nothing() {
        use crate::core::ai::memory::{health::Health, port::MemReply};
        let mut a = app();
        let reply = MemReply::Health {
            state: Health::Down,
            offer: true,
        };
        assert_eq!(a.update(AppEvent::Memory(reply)), vec![]);
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
        let mut a = app();
        with_history(&mut a, 40);
        a.update(ctrl_space());
        a.update(key('['));
        for _ in 0..23 {
            a.update(repeat('k'));
        }
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
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
        let mut a = app();
        with_history(&mut a, 60);
        a.update(ctrl_space());
        a.update(key('['));
        let history = a.focused_pane().scrollback_len();
        assert!(history > 30);
        press_n(&mut a, 'k', 24);
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

    fn copy_state(a: &App) -> CopyState {
        match a.input() {
            InputMode::Copy(state) => state,
            other => panic!("not in COPY: {other:?}"),
        }
    }

    fn copy_offset(a: &App) -> usize {
        copy_state(a).offset
    }

    fn feed_focused(a: &mut App, bytes: &[u8]) {
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(bytes.to_vec()),
        ));
    }

    // copy-mode-scrollback: Cursor starts at the live cursor.
    #[test]
    fn entering_copy_puts_the_cursor_on_the_childs_cursor() {
        let mut a = app();
        feed_focused(&mut a, b"\r\n\r\n\r\n\r\n\r\nabc");
        a.update(ctrl_space());
        a.update(key('['));
        let state = copy_state(&a);
        assert_eq!(state.cursor, Point { line: 5, col: 3 });
        assert_eq!(state.offset, 0);
        assert_eq!(state.selection, None);
        assert_eq!(a.copy_cursor(), Some((5, 3)));
    }

    #[test]
    fn entering_copy_with_a_hidden_cursor_uses_the_bottom_row() {
        let mut a = app();
        feed_focused(&mut a, b"abc\x1b[?25l");
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(copy_state(&a).cursor, Point { line: 23, col: 0 });
        assert_eq!(a.copy_cursor(), Some((23, 0)));
    }

    fn copy_after(a: &mut App, bytes: &[u8]) {
        feed_focused(a, bytes);
        a.update(ctrl_space());
        a.update(key('['));
    }

    fn esc_kinded(kind: KeyEventKind) -> AppEvent {
        kinded(KeyCode::Esc, KeyModifiers::NONE, kind)
    }

    // copy-mode-scrollback: Horizontal motion, End of a blank row (app part).
    #[test]
    fn horizontal_keys_move_the_copy_cursor_and_dollar_uses_the_line_end() {
        let mut a = app();
        copy_after(&mut a, b"echo hi");
        assert_eq!(copy_state(&a).cursor, Point { line: 0, col: 7 });
        let mut cols = Vec::new();
        for c in ['0', '$', 'l', 'h'] {
            a.update(key(c));
            cols.push(copy_state(&a).cursor.col);
        }
        assert_eq!(cols, [0, 6, 7, 6]);
    }

    #[test]
    fn dollar_on_a_blank_row_goes_to_column_zero() {
        let mut a = app();
        copy_after(&mut a, b"echo hi\r\n");
        a.update(key('l'));
        assert_eq!(copy_state(&a).cursor, Point { line: 1, col: 1 });
        a.update(key('$'));
        assert_eq!(copy_state(&a).cursor, Point { line: 1, col: 0 });
    }

    // copy-mode-scrollback: Line motion, Cursor moves inside the viewport.
    #[test]
    fn vertical_keys_move_the_cursor_and_scroll_only_at_the_edge() {
        let mut a = app();
        let mut lines = Vec::new();
        for i in 0..40 {
            lines.extend_from_slice(format!("line{i}\r\n").as_bytes());
        }
        copy_after(&mut a, &lines);
        assert_eq!(a.focused_pane().scrollback_len(), 17);
        assert_eq!(copy_state(&a).cursor, Point { line: 40, col: 0 });
        a.update(key('k'));
        assert_eq!(copy_state(&a).cursor.line, 39);
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
        for _ in 0..22 {
            a.update(key('k'));
        }
        assert_eq!(copy_state(&a).cursor.line, 17);
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
        a.update(key('k'));
        assert_eq!(copy_state(&a).cursor.line, 16);
        assert_eq!(a.focused_pane().scrollback_offset(), 1);
        assert_eq!(copy_offset(&a), 1);
        a.update(key('j'));
        assert_eq!(copy_state(&a).cursor.line, 17);
        assert_eq!(a.focused_pane().scrollback_offset(), 1);
    }

    // copy-selection: Esc clears then exits, `q` drops the selection.
    #[test]
    fn esc_clears_the_selection_then_leaves_copy_and_q_drops_it() {
        let mut a = app();
        copy_after(&mut a, b"");
        a.update(key('v'));
        assert_eq!(
            copy_state(&a).selection.map(|s| s.anchor),
            Some(Point::default())
        );
        assert_eq!(a.update(esc()), vec![]);
        assert_eq!(copy_state(&a).selection, None);
        assert_eq!(a.update(esc()), vec![]);
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
        a.update(ctrl_space());
        a.update(key('['));
        a.update(key('V'));
        assert_eq!(a.update(key('q')), vec![]);
        assert_eq!(a.input(), InputMode::Terminal);
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(copy_state(&a).selection, None);
    }

    // modal-input: Repeat of selection keys / motions in COPY.
    #[test]
    fn repeat_of_one_shot_keys_changes_nothing_in_copy() {
        let mut a = app();
        copy_after(&mut a, b"abc");
        a.update(key('v'));
        let before = a.input();
        let held = [
            repeat('v'),
            repeat('V'),
            repeat('y'),
            repeat('q'),
            repeat('i'),
            esc_kinded(KeyEventKind::Repeat),
        ];
        for ev in held {
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), before, "{ev:?}");
        }
    }

    #[test]
    fn repeat_of_j_moves_the_copy_cursor_one_row() {
        let mut a = app();
        copy_after(&mut a, b"");
        assert_eq!(a.update(repeat('j')), vec![]);
        assert_eq!(copy_state(&a).cursor, Point { line: 1, col: 0 });
    }

    fn selection_of(a: &App) -> Option<(Point, SelectionKind)> {
        copy_state(a).selection.map(|s| (s.anchor, s.kind))
    }

    fn osc(text: &str) -> Vec<u8> {
        crate::core::copy::osc52(text).expect("fits")
    }

    /// 1000 rows: 999 of 74 `x` and a last one of `last` `x` (no newline).
    fn big_selection(a: &mut App, last: usize) {
        let mut data = String::new();
        for _ in 0..999 {
            data.push_str(&"x".repeat(74));
            data.push_str("\r\n");
        }
        data.push_str(&"x".repeat(last));
        feed_focused(a, data.as_bytes());
        a.update(ctrl_space());
        a.update(key('['));
        a.update(key('V'));
        a.update(key('g'));
        a.update(key('g'));
    }

    // copy-selection: Yank emits OSC 52, Character selection within a row.
    #[test]
    fn yank_emits_the_clipboard_sequence_and_leaves_copy() {
        let mut a = app();
        copy_after(&mut a, b"hello");
        a.update(key('0'));
        a.update(key('v'));
        press_n(&mut a, 'l', 4);
        assert_eq!(
            a.update(key('y')),
            vec![Effect::SetClipboard(b"\x1b]52;c;aGVsbG8=\x07".to_vec())]
        );
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
        assert_eq!(a.copy_cursor(), None);
    }

    // copy-selection: Empty extraction.
    #[test]
    fn yanking_only_blank_rows_sends_nothing_and_stays_in_copy() {
        let mut a = App::new(20, 5);
        copy_after(&mut a, b"abc");
        press_n(&mut a, 'j', 2);
        a.update(key('V'));
        assert_eq!(a.update(key('y')), vec![]);
        assert!(is_copy(&a));
        assert!(selection_of(&a).is_some());
        assert_eq!(a.notice(), Some("Nothing to copy"));
    }

    // copy-selection: Line yank drops trailing blank rows.
    #[test]
    fn a_line_yank_never_ends_in_a_newline() {
        let mut a = app();
        copy_after(&mut a, b"foo\r\n\r\nbar\r\n\r\n");
        a.update(key('V'));
        press_n(&mut a, 'k', 4);
        assert_eq!(
            a.update(key('y')),
            vec![Effect::SetClipboard(osc52("foo\n\nbar").expect("fits"))]
        );
        assert_eq!(a.notice(), Some("Copied 8 characters"));
    }

    // copy-selection: Multi-row line selection.
    #[test]
    fn a_line_selection_yanks_the_rows_joined_by_newline() {
        let mut a = app();
        copy_after(&mut a, b"foo\r\nbar");
        a.update(key('k'));
        a.update(key('V'));
        a.update(key('j'));
        assert_eq!(
            a.update(key('y')),
            vec![Effect::SetClipboard(osc("foo\nbar"))]
        );
        assert_eq!(a.notice(), Some("Copied 7 characters"));
    }

    // copy-selection: Wide character once.
    #[test]
    fn a_wide_character_is_yanked_once() {
        let mut a = app();
        copy_after(&mut a, "a漢b".as_bytes());
        a.update(key('0'));
        a.update(key('v'));
        a.update(key('$'));
        assert_eq!(a.update(key('y')), vec![Effect::SetClipboard(osc("a漢b"))]);
        assert_eq!(a.notice(), Some("Copied 3 characters"));
    }

    // copy-selection: Selection spans scrollback (app part: offset restored).
    #[test]
    fn yanking_from_scrollback_restores_the_bottom_viewport() {
        let mut a = app();
        with_history(&mut a, 40);
        a.update(ctrl_space());
        a.update(key('['));
        a.update(key('g'));
        a.update(key('g'));
        a.update(key('v'));
        press_n(&mut a, 'l', 4);
        assert_eq!(a.update(key('y')), vec![Effect::SetClipboard(osc("line0"))]);
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
    }

    // copy-selection: Yank without selection.
    #[test]
    fn yank_without_a_selection_changes_nothing() {
        let mut a = app();
        copy_after(&mut a, b"abc");
        assert_eq!(a.update(key('y')), vec![]);
        assert!(is_copy(&a));
        assert_eq!(a.notice(), None);
    }

    // copy-selection: At the cap.
    #[test]
    fn a_selection_of_exactly_the_cap_is_sent() {
        let mut a = app();
        big_selection(&mut a, 75);
        let text = format!(
            "{}{}",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n"
                .repeat(999),
            "x".repeat(75)
        );
        assert_eq!(text.len(), 75_000);
        assert_eq!(a.update(key('y')), vec![Effect::SetClipboard(osc(&text))]);
        assert_eq!(a.input(), InputMode::Terminal);
    }

    // copy-selection: Over the cap.
    #[test]
    fn an_oversized_selection_sends_nothing_and_stays_in_copy() {
        let mut a = app();
        big_selection(&mut a, 76);
        let before = selection_of(&a);
        assert!(before.is_some());
        assert_eq!(a.update(key('y')), vec![]);
        assert!(is_copy(&a));
        assert_eq!(selection_of(&a), before);
        assert_eq!(a.notice(), Some("Selection too large to copy"));
    }

    // statusline: Copied notice, Singular notice, Notice cleared by next key.
    #[test]
    fn the_copied_notice_counts_characters_and_clears_on_the_next_key() {
        let mut a = app();
        copy_after(&mut a, b"x");
        a.update(key('0'));
        a.update(key('v'));
        a.update(key('y'));
        assert_eq!(a.notice(), Some("Copied 1 character"));
        assert_eq!(a.status_path(40), "Copied 1 character");
        assert_eq!(a.input(), InputMode::Terminal);
        a.update(key('z'));
        assert_eq!(a.notice(), None);
    }

    // statusline: a newer notice replaces the copy notice.
    #[test]
    fn a_newer_notice_replaces_the_copied_notice() {
        let mut a = app();
        copy_after(&mut a, b"x");
        a.update(key('0'));
        a.update(key('v'));
        a.update(key('y'));
        a.update(AppEvent::SpawnFailed(PaneId::FIRST, "boom".into()));
        assert_eq!(a.notice(), Some("spawn failed: boom"));
    }

    // copy-selection: Output keeps the selection on its content
    // (characterization: stable line numbers need no adjustment).
    #[test]
    fn output_keeps_the_selection_on_its_content() {
        let mut a = app();
        with_history(&mut a, 40);
        a.update(ctrl_space());
        a.update(key('['));
        a.update(key('v'));
        let selected = selection_of(&a);
        assert_eq!(
            selected,
            Some((Point { line: 40, col: 0 }, SelectionKind::Char))
        );
        with_history(&mut a, 5);
        assert_eq!(selection_of(&a), selected);
        assert_eq!(copy_state(&a).cursor, Point { line: 40, col: 0 });
    }

    // copy-selection: Eviction of the anchor.
    #[test]
    fn eviction_clamps_the_anchor_and_keeps_the_selection() {
        let mut a = app();
        with_history(&mut a, SCROLLBACK_LINES + 100);
        a.update(ctrl_space());
        a.update(key('['));
        a.update(key('g'));
        a.update(key('g'));
        press_n(&mut a, 'l', 3);
        a.update(key('v'));
        let before = a.focused_pane().first_line();
        assert_eq!(before, 77);
        assert_eq!(
            selection_of(&a).map(|s| s.0),
            Some(Point { line: 77, col: 3 })
        );
        with_history(&mut a, 50);
        let first = a.focused_pane().first_line();
        assert_eq!(first, 127);
        assert_eq!(
            selection_of(&a),
            Some((Point { line: 127, col: 0 }, SelectionKind::Char))
        );
        assert_eq!(copy_state(&a).cursor, Point { line: 127, col: 3 });
        assert_eq!(a.copy_cursor(), Some((0, 3)));
        assert!(is_copy(&a));
    }

    fn release(c: char) -> AppEvent {
        kinded(KeyCode::Char(c), KeyModifiers::NONE, KeyEventKind::Release)
    }

    // modal-input: Held exit key does not leak into TERMINAL.
    #[test]
    fn a_held_q_or_i_does_not_leak_into_terminal_after_leaving_copy() {
        for c in ['q', 'i'] {
            let mut a = in_copy();
            a.update(key(c));
            assert_eq!(a.input(), InputMode::Terminal, "{c}");
            assert_eq!(a.update(repeat(c)), vec![], "{c}");
            assert_eq!(a.update(repeat(c)), vec![], "{c}");
        }
    }

    #[test]
    fn a_held_esc_does_not_leak_into_terminal_after_leaving_copy() {
        let mut a = in_copy();
        a.update(esc());
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.update(esc_kinded(KeyEventKind::Repeat)), vec![]);
    }

    #[test]
    fn a_held_y_does_not_leak_into_terminal_after_a_yank() {
        let mut a = app();
        copy_after(&mut a, b"abc");
        a.update(key('h'));
        a.update(key('v'));
        assert_eq!(a.update(key('y')).len(), 1);
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.update(repeat('y')), vec![]);
        // The drop must not eat the yank notice.
        assert_eq!(a.notice(), Some("Copied 1 character"));
    }

    #[test]
    fn another_key_after_leaving_copy_is_passed_through() {
        let mut a = in_copy();
        a.update(key('q'));
        assert_eq!(
            a.update(repeat('x')),
            vec![Effect::WritePty(PaneId::FIRST, b"x".to_vec())]
        );
    }

    #[test]
    fn a_fresh_press_after_the_release_is_passed_through() {
        let mut a = in_copy();
        a.update(key('q'));
        assert_eq!(a.update(release('q')), vec![]);
        assert_eq!(
            a.update(repeat('q')),
            vec![Effect::WritePty(PaneId::FIRST, b"q".to_vec())]
        );
        let mut a = in_copy();
        a.update(key('q'));
        a.update(key('x'));
        assert_eq!(
            a.update(repeat('q')),
            vec![Effect::WritePty(PaneId::FIRST, b"q".to_vec())]
        );
    }

    thread_local! {
        static NOW_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        static BASE: Instant = Instant::now();
    }

    fn mock_now() -> Instant {
        BASE.with(|b| *b + Duration::from_millis(NOW_MS.with(|n| n.get())))
    }

    /// Sends `ev` as if it arrived `ms` after the test's t0.
    fn at(a: &mut App, ms: u64, ev: AppEvent) -> Vec<Effect> {
        NOW_MS.with(|n| n.set(ms));
        a.update(ev)
    }

    fn timed(a: App) -> App {
        a.with_clock(mock_now)
    }

    fn q_pty() -> Vec<Effect> {
        vec![Effect::WritePty(PaneId::FIRST, b"q".to_vec())]
    }

    // modal-input: Held exit key within the window (Presses only).
    #[test]
    fn a_held_exit_key_sent_as_presses_is_dropped_for_the_whole_hold() {
        for c in ['q', 'i'] {
            let mut a = timed(app());
            at(&mut a, 0, ctrl_space());
            at(&mut a, 0, key('['));
            at(&mut a, 0, key(c));
            assert_eq!(a.input(), InputMode::Terminal, "{c}");
            for ms in [400, 433, 466] {
                assert_eq!(at(&mut a, ms, key(c)), vec![], "{c} at {ms}");
            }
        }
        let mut a = timed(app());
        at(&mut a, 0, ctrl_space());
        at(&mut a, 0, key('['));
        at(&mut a, 0, esc());
        for ms in [400, 433, 466] {
            assert_eq!(at(&mut a, ms, esc()), vec![], "esc at {ms}");
        }
    }

    #[test]
    fn a_held_yank_key_sent_as_presses_is_dropped_and_keeps_the_notice() {
        let mut a = timed(app());
        copy_after(&mut a, b"abc");
        at(&mut a, 0, key('h'));
        at(&mut a, 0, key('v'));
        assert_eq!(at(&mut a, 0, key('y')).len(), 1);
        for ms in [400, 433, 466] {
            assert_eq!(at(&mut a, ms, key('y')), vec![], "y at {ms}");
        }
        assert_eq!(a.notice(), Some("Copied 1 character"));
    }

    #[test]
    fn a_long_hold_keeps_being_dropped_past_the_window_from_the_exit() {
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        for ms in [400, 800, 1200, 1600] {
            assert_eq!(at(&mut a, ms, key('q')), vec![], "q at {ms}");
        }
    }

    #[test]
    fn a_deliberate_press_after_the_window_passes_through() {
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        assert_eq!(at(&mut a, 600, key('q')), q_pty());
        // The debounce is over: the next one is typed text too.
        assert_eq!(at(&mut a, 650, key('q')), q_pty());
    }

    #[test]
    fn another_key_ends_the_debounce() {
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        assert_eq!(
            at(&mut a, 100, key('x')),
            vec![Effect::WritePty(PaneId::FIRST, b"x".to_vec())]
        );
        assert_eq!(at(&mut a, 150, key('q')), q_pty());
    }

    #[test]
    fn a_release_of_the_exit_key_ends_the_debounce() {
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        at(&mut a, 50, release('q'));
        assert_eq!(at(&mut a, 100, key('q')), q_pty());
    }

    #[test]
    fn the_window_boundary_is_inclusive_at_500_ms() {
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        assert_eq!(at(&mut a, 500, key('q')), vec![]);
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        assert_eq!(at(&mut a, 501, key('q')), q_pty());
    }

    #[test]
    fn each_dropped_press_refreshes_the_window() {
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        assert_eq!(at(&mut a, 499, key('q')), vec![]);
        assert_eq!(at(&mut a, 998, key('q')), vec![]);
        assert_eq!(at(&mut a, 1499, key('q')), q_pty());
    }

    #[test]
    fn a_yank_that_stays_in_copy_does_not_arm_the_debounce() {
        // Over the cap.
        let mut a = timed(app());
        big_selection(&mut a, 76);
        assert_eq!(at(&mut a, 0, key('y')), vec![]);
        assert!(is_copy(&a));
        assert!(a.held_exit.is_none());
        // Nothing to copy.
        let mut a = timed(App::new(20, 5));
        copy_after(&mut a, b"abc");
        press_n(&mut a, 'j', 2);
        a.update(key('V'));
        assert_eq!(at(&mut a, 0, key('y')), vec![]);
        assert!(is_copy(&a));
        assert!(a.held_exit.is_none());
        // Leaving COPY by another key, a `y` right after is typed text.
        at(&mut a, 10, key('q'));
        assert_eq!(
            at(&mut a, 100, key('y')),
            vec![Effect::WritePty(PaneId::FIRST, b"y".to_vec())]
        );
    }

    #[test]
    fn a_paste_ends_the_debounce() {
        let mut a = timed(in_copy());
        at(&mut a, 0, key('q'));
        at(&mut a, 100, AppEvent::Paste("p".into()));
        assert_eq!(at(&mut a, 200, key('q')), q_pty());
    }

    #[test]
    fn the_debounce_needs_the_identical_key_and_modifiers() {
        let shift_q = AppEvent::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::SHIFT));
        let ctrl_q = AppEvent::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
        for ev in [shift_q, ctrl_q] {
            let mut a = timed(in_copy());
            at(&mut a, 0, key('q'));
            assert_eq!(at(&mut a, 100, ev.clone()).len(), 1, "{ev:?}");
        }
    }

    // copy-mode-scrollback: a horizontal motion also brings an off-screen
    // cursor back into view.
    #[test]
    fn a_horizontal_motion_brings_an_off_screen_cursor_back() {
        let mut a = app();
        copy_after(&mut a, b"abc");
        feed_focused(&mut a, "\r\n".repeat(40).as_bytes());
        assert_eq!(a.copy_cursor(), None);
        a.update(key('l'));
        assert_eq!(a.copy_cursor(), Some((0, 4)));
    }

    // copy-mode-scrollback: a screen switch resets the cursor like entering COPY.
    #[test]
    fn a_screen_round_trip_puts_the_cursor_back_on_the_live_cursor() {
        let mut a = app();
        with_history(&mut a, 30);
        feed_focused(&mut a, b"abc");
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(a.copy_cursor(), Some((23, 3)));
        press_n(&mut a, 'k', 5);
        feed_focused(&mut a, b"\x1b[?1049h");
        feed_focused(&mut a, b"\x1b[?1049l");
        assert!(is_copy(&a));
        assert_eq!(a.copy_cursor(), Some((23, 3)));
    }

    // copy-selection: Screen switch drops it.
    #[test]
    fn a_screen_switch_drops_the_selection_and_keeps_copy() {
        let mut a = app();
        copy_after(&mut a, b"abc");
        a.update(key('v'));
        feed_focused(&mut a, b"\x1b[?1049h");
        assert!(is_copy(&a));
        assert_eq!(selection_of(&a), None);
        assert_eq!(
            copy_state(&a).generation,
            a.focused_pane().line_generation()
        );
        // The new generation is adopted and the cursor sits on the alternate
        // screen's live cursor: a fresh selection survives output.
        assert_eq!(copy_state(&a).cursor, Point { line: 0, col: 0 });
        a.update(key('v'));
        feed_focused(&mut a, b"x");
        assert_eq!(
            selection_of(&a),
            Some((Point { line: 0, col: 0 }, SelectionKind::Char))
        );
    }

    // copy-selection: Resize drops it.
    #[test]
    fn a_host_resize_drops_the_selection_and_clamps_the_cursor() {
        let mut a = app();
        copy_after(&mut a, &[b'x'; 70]);
        a.update(key('v'));
        assert_eq!(copy_state(&a).cursor, Point { line: 0, col: 70 });
        a.update(AppEvent::Resize { cols: 40, rows: 25 });
        assert!(is_copy(&a));
        assert_eq!(selection_of(&a), None);
        assert_eq!(copy_state(&a).cursor, Point { line: 0, col: 39 });
    }

    #[test]
    fn a_resize_that_shortens_the_pane_clamps_the_cursor_line() {
        let mut a = app();
        copy_after(&mut a, "\r\n".repeat(23).as_bytes());
        assert_eq!(copy_state(&a).cursor, Point { line: 23, col: 0 });
        a.update(AppEvent::Resize { cols: 80, rows: 10 });
        let pane = a.focused_pane();
        let last = pane.first_line() + pane.scrollback_len() + 8;
        assert_eq!(copy_state(&a).cursor, Point { line: last, col: 0 });
        assert_eq!(copy_state(&a).offset, pane.scrollback_offset());
    }

    #[test]
    fn a_sibling_exit_that_resizes_the_focused_pane_drops_the_selection() {
        let mut a = split_right();
        a.update(ctrl_space());
        a.update(key('['));
        a.update(key('v'));
        assert!(selection_of(&a).is_some());
        let effects = a.update(exited(PaneId::FIRST));
        assert_eq!(effects, vec![Effect::ResizePty(id2(), sized(24, 80))]);
        assert!(is_copy(&a));
        assert_eq!(selection_of(&a), None);
    }

    // copy-selection: Focus change drops it.
    #[test]
    fn leaving_copy_by_a_focus_change_forgets_the_selection() {
        let mut a = scrolled_split();
        a.update(key('v'));
        a.update(exited(id2()));
        assert_eq!(a.input(), InputMode::Terminal);
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(selection_of(&a), None);
    }

    // copy-mode-scrollback: Block follows the COPY cursor (app part).
    #[test]
    fn the_copy_cursor_is_none_outside_copy_and_when_scrolled_off_screen() {
        let mut a = app();
        assert_eq!(a.copy_cursor(), None, "TERMINAL");
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(a.copy_cursor(), Some((0, 0)));
        feed_focused(&mut a, "\r\n".repeat(30).as_bytes());
        assert_eq!(
            a.copy_cursor(),
            None,
            "the cursor row scrolled into history"
        );
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
        let mut a = app();
        with_history(&mut a, 40);
        a.update(ctrl_space());
        a.update(key('['));
        press_n(&mut a, 'k', 23 + 10);
        assert_eq!(copy_offset(&a), 10);
        with_history(&mut a, 5);
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        let anchored = a.focused_pane().scrollback_offset();
        assert!(anchored > 10, "the emulator pins the viewport");
        a.update(ctrl_d());
        assert_eq!(a.focused_pane().scrollback_offset(), anchored - 12);
        assert_eq!(copy_offset(&a), anchored - 12);
        press_n(&mut a, 'k', 13);
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
    // comes from the emulator pinning the viewport plus `sync_copy`.
    #[test]
    fn output_during_copy_keeps_the_same_content_visible() {
        let mut a = app();
        with_history(&mut a, 60);
        a.update(ctrl_space());
        a.update(key('['));
        press_n(&mut a, 'k', 23 + 10);
        let before = visible_text(&a);
        assert_eq!(before.first().map(String::as_str), Some("line27"));
        with_history(&mut a, 5);
        assert_eq!(visible_text(&a), before);
        assert!(is_copy(&a));
        a.update(ctrl_d());
        assert_eq!(visible_text(&a)[0], "line39");
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
        press_n(&mut a, 'k', 23 + 10);
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
    fn a_focused_chat_shows_a_blinking_bar_and_the_shell_shape_comes_back() {
        let mut a = app();
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[2 q".to_vec()),
        ));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.open_chat(None);
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, true));
        a.update(ctrl_space());
        a.update(key('h'));
        assert_eq!(a.focused(), PaneId::FIRST);
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
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

    fn osc7(id: PaneId, uri: &str) -> AppEvent {
        AppEvent::Pty(
            id,
            PtyEvent::Output(format!("\x1b]7;{uri}\x07").into_bytes()),
        )
    }

    // Spec (terminal-session): OSC 7 wins over the poll.
    #[test]
    fn osc7_cwd_is_shown_and_survives_a_later_poll() {
        let mut a = launched(80, 25);
        a.update(osc7(PaneId::FIRST, "file:///tmp"));
        assert_eq!(a.cwd_label(), "/tmp");
        a.update(cwd_event("/var"));
        assert_eq!(a.cwd_label(), "/tmp");
    }

    // Spec (terminal-session): Fallback for plain shell.
    #[test]
    fn the_poll_fills_in_for_a_shell_that_never_reports() {
        let mut a = launched(80, 25);
        a.update(cwd_event("/var"));
        assert_eq!(a.cwd_label(), "/var");
    }

    #[test]
    fn a_rejected_osc7_leaves_the_poll_cwd_visible() {
        let mut a = launched(80, 25);
        a.update(cwd_event("/var"));
        a.update(osc7(PaneId::FIRST, "file://other.example/etc"));
        assert_eq!(a.cwd_label(), "/var");
    }

    // A hidden tab's pane output does not redraw, but its tab label may change.
    #[test]
    fn osc7_in_a_hidden_tab_redraws_only_when_its_label_changes() {
        let mut a = launched(80, 25);
        press(&mut a, "tn");
        a.take_dirty();
        a.update(osc7(PaneId::FIRST, "file:///srv"));
        assert!(a.take_dirty());
        a.update(osc7(PaneId::FIRST, "file:///srv"));
        assert!(!a.take_dirty());
    }

    #[test]
    fn a_hostname_makes_matching_osc7_reports_valid() {
        let mut a = launched(80, 25).with_hostname(Some("myhost"));
        a.update(osc7(PaneId::FIRST, "file://myhost/srv"));
        assert_eq!(a.cwd_label(), "/srv");
        let mut b = launched(80, 25);
        b.update(osc7(PaneId::FIRST, "file://myhost/srv"));
        assert_eq!(b.cwd_label(), LAUNCH);
    }

    // Spec (statusline): OSC 7 cwd shown; (tabs): label follows it.
    #[test]
    fn the_tab_label_follows_the_osc7_cwd() {
        let mut a = launched(80, 25);
        press(&mut a, "tn");
        a.update(osc7(PaneId::FIRST, "file:///srv/www"));
        assert_eq!(a.tab_labels(), vec!["1 www".to_string(), "2".to_string()]);
    }

    // Spec (terminal-session / tabs): new pane and tab inherit the OSC 7 cwd.
    #[test]
    fn a_split_inherits_the_osc7_cwd() {
        let mut a = launched(80, 25);
        a.update(cwd_event("/var"));
        a.update(osc7(PaneId::FIRST, "file:///srv"));
        let effects = press(&mut a, "wv");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec("/srv", sized(24, 40))))
        );
    }

    #[test]
    fn a_new_tab_inherits_the_osc7_cwd() {
        let mut a = launched(80, 25);
        a.update(osc7(PaneId::FIRST, "file:///srv"));
        let effects = press(&mut a, "tn");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec("/srv", sized(23, 80))))
        );
    }

    // Spec (terminal-session / tabs): a vanished inherited cwd falls back.
    fn exists(path: &Path) -> bool {
        path != Path::new("/gone") && path != Path::new("/launch-gone")
    }

    fn with_vanished_cwd() -> App {
        let mut a = App::new(80, 25)
            .with_env(Some("/bin/zsh"), Some("/home/ana".into()), LAUNCH.into())
            .with_dir_check(exists);
        a.update(osc7(PaneId::FIRST, "file:///gone"));
        a
    }

    #[test]
    fn a_split_from_a_vanished_cwd_uses_the_launch_cwd() {
        let mut a = with_vanished_cwd();
        let effects = press(&mut a, "wv");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec(LAUNCH, sized(24, 40))))
        );
    }

    #[test]
    fn a_new_tab_from_a_vanished_cwd_uses_the_launch_cwd() {
        let mut a = with_vanished_cwd();
        let effects = press(&mut a, "tn");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec(LAUNCH, sized(23, 80))))
        );
    }

    #[test]
    fn a_vanished_launch_cwd_falls_back_to_home() {
        let mut a = App::new(80, 25)
            .with_env(
                Some("/bin/zsh"),
                Some("/home/ana".into()),
                "/launch-gone".into(),
            )
            .with_dir_check(exists);
        a.update(osc7(PaneId::FIRST, "file:///gone"));
        let effects = press(&mut a, "tn");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec("/home/ana", sized(23, 80))))
        );
    }

    #[test]
    fn a_relative_home_is_never_a_spawn_fallback() {
        let mut a = App::new(80, 25)
            .with_env(
                Some("/bin/zsh"),
                Some("home/ana".into()),
                "/launch-gone".into(),
            )
            .with_dir_check(exists);
        a.update(osc7(PaneId::FIRST, "file:///gone"));
        let effects = press(&mut a, "tn");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec("/", sized(23, 80))))
        );
    }

    #[test]
    fn with_nothing_left_the_spawn_falls_back_to_the_root() {
        let mut a = App::new(80, 25)
            .with_env(Some("/bin/zsh"), None, "/launch-gone".into())
            .with_dir_check(exists);
        a.update(osc7(PaneId::FIRST, "file:///gone"));
        let effects = press(&mut a, "wv");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec("/", sized(24, 40))))
        );
    }

    #[test]
    fn an_existing_cwd_is_inherited_unchanged_with_a_dir_check() {
        let mut a = launched(80, 25).with_dir_check(exists);
        a.update(osc7(PaneId::FIRST, "file:///srv"));
        let effects = press(&mut a, "wv");
        assert_eq!(
            effects.last(),
            Some(&Effect::SpawnPane(id2(), spec("/srv", sized(24, 40))))
        );
    }

    #[test]
    fn panes_created_after_with_hostname_accept_the_hostname_too() {
        let mut a = launched(80, 25).with_hostname(Some("myhost"));
        press(&mut a, "wv");
        a.update(osc7(id2(), "file://myhost/srv"));
        assert_eq!(a.cwd_label(), "/srv");
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
        press_n(&mut a, 'k', 24);
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

    const ROOT_HINT: &str =
        "w window · t tab · g go · b buffer · [ copy · m menu · c chat · n note · q quit";
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

    fn output(n: u32) -> AppEvent {
        AppEvent::Pty(pid(n), PtyEvent::Output(b"zz".to_vec()))
    }

    // Spec: Close tab confirmed; only live panes get a ClosePane.
    #[test]
    fn closing_a_tab_with_an_exited_pane_closes_only_the_live_panes() {
        let mut a = tabs(2);
        press(&mut a, "wv");
        a.update(exited(pid(2)));
        press(&mut a, "tc");
        let effects = a.update(key('y'));
        let closed: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::ClosePane(_)))
            .collect();
        assert_eq!(closed, vec![&Effect::ClosePane(id3())]);
    }

    // A prompt is cancelled by a removal, in any tab.
    #[test]
    fn a_pending_confirm_is_cancelled_when_a_pane_of_an_inactive_tab_exits() {
        for keys in ["tc", "wq"] {
            let mut a = tabs(3);
            press(&mut a, keys);
            assert!(matches!(a.input(), InputMode::Confirm(_)), "{keys}");
            a.update(exited(pid(1)));
            assert_eq!(a.input(), InputMode::Terminal, "{keys}");
            let count = a.tab_count();
            let effects = a.update(key('y'));
            assert_eq!(a.tab_count(), count, "{keys}");
            assert!(
                !effects.iter().any(|e| matches!(e, Effect::ClosePane(_))),
                "{keys}: {effects:?}"
            );
        }
    }

    // Hidden tabs are not on screen: their output must not redraw.
    #[test]
    fn output_of_a_pane_in_a_hidden_tab_does_not_mark_dirty() {
        let mut a = tabs(2);
        a.take_dirty();
        a.update(output(1));
        assert!(!a.take_dirty());
        a.update(output(2));
        assert!(a.take_dirty());
    }

    #[test]
    fn a_hidden_pane_still_receives_its_output() {
        let mut a = tabs(2);
        a.update(output(1));
        press(&mut a, "b1");
        assert_eq!(a.focused_pane().cell(0, 0).unwrap().text, "z");
    }

    // The tab label shows the focused pane's cwd, so its change redraws.
    #[test]
    fn a_cwd_change_in_a_hidden_tab_marks_dirty_while_the_bar_is_shown() {
        let mut a = tabs(2);
        a.take_dirty();
        a.update(AppEvent::Cwd(pid(1), "/tmp".into()));
        assert!(a.take_dirty());
        a.update(AppEvent::Cwd(pid(1), "/tmp".into()));
        assert!(!a.take_dirty());
    }

    // --- S8b: labels ---

    // Spec: Tab label.
    #[test]
    fn tab_labels_are_the_number_and_the_cwd_basename() {
        let mut a = tabs(2);
        a.update(AppEvent::Cwd(pid(1), "/home/u/proj".into()));
        a.update(AppEvent::Cwd(pid(2), "/tmp".into()));
        assert_eq!(a.tab_labels(), ["1 proj", "2 tmp"]);
        a.update(AppEvent::Cwd(pid(2), "/".into()));
        assert_eq!(a.tab_labels(), ["1 proj", "2 /"]);
    }

    // Spec: Label without cwd.
    #[test]
    fn a_tab_label_without_a_cwd_is_its_number() {
        let a = tabs(2);
        assert_eq!(a.tab_labels(), ["1 launch", "2"]);
    }

    #[test]
    fn a_tab_label_follows_the_focused_pane_of_that_tab() {
        let mut a = tabs(2);
        a.update(AppEvent::Cwd(pid(2), "/a".into()));
        press(&mut a, "wv");
        a.update(AppEvent::Cwd(pid(3), "/b".into()));
        assert_eq!(a.tab_labels(), ["1 launch", "2 b"]);
        press(&mut a, "h");
        assert_eq!(a.tab_labels(), ["1 launch", "2 a"]);
    }

    // Spec: Bar positions (screen geometry).
    fn configured(
        cols: u16,
        rows: u16,
        statusline: BarPosition,
        tabbar: BarPosition,
        n: usize,
    ) -> App {
        let config = Config {
            bars: BarPositions { statusline, tabbar },
            ..Config::default()
        };
        let mut a = launched(cols, rows).with_config(&config);
        for _ in 1..n {
            press(&mut a, "tn");
        }
        a
    }

    fn row_rect(y: u16) -> Rect {
        Rect {
            x: 0,
            y,
            width: 80,
            height: 1,
        }
    }

    fn body_rect(y: u16, height: u16) -> Rect {
        Rect {
            x: 0,
            y,
            width: 80,
            height,
        }
    }

    fn expect(a: &App, bar: Option<u16>, body: (u16, u16), status: u16) {
        let screen = a.screen();
        assert_eq!(screen.tab_bar, bar.map(row_rect), "tab bar");
        assert_eq!(screen.body, body_rect(body.0, body.1), "body");
        assert_eq!(screen.status, row_rect(status), "status");
    }

    #[test]
    fn one_tab_with_the_statusline_on_top_moves_the_body_down() {
        let a = configured(80, 25, BarPosition::Top, BarPosition::Top, 1);
        expect(&a, None, (1, 24), 0);
        let a = configured(80, 25, BarPosition::Bottom, BarPosition::Bottom, 1);
        expect(&a, None, (0, 24), 24);
    }

    #[test]
    fn several_tabs_in_the_four_position_combinations() {
        use BarPosition::{Bottom, Top};
        // Default: the bar on top, the statusline at the bottom.
        expect(&configured(80, 25, Bottom, Top, 2), Some(0), (1, 23), 24);
        // Both on top: the bar is outermost (row 0), the statusline next to the body.
        expect(&configured(80, 25, Top, Top, 2), Some(0), (2, 23), 1);
        // Both at the bottom: body, statusline, bar on the last row.
        expect(
            &configured(80, 25, Bottom, Bottom, 2),
            Some(24),
            (0, 23),
            23,
        );
        // One each, the other way round.
        expect(&configured(80, 25, Top, Bottom, 2), Some(24), (1, 23), 0);
    }

    #[test]
    fn closing_back_to_one_tab_drops_the_bar_in_any_position() {
        let mut a = configured(80, 25, BarPosition::Bottom, BarPosition::Bottom, 2);
        press(&mut a, "tc");
        a.update(key('y'));
        expect(&a, None, (0, 24), 24);
    }

    #[test]
    fn a_resize_keeps_the_configured_positions() {
        let mut a = configured(80, 25, BarPosition::Top, BarPosition::Bottom, 2);
        a.update(AppEvent::Resize {
            cols: 100,
            rows: 40,
        });
        let screen = a.screen();
        assert_eq!(screen.status.y, 0);
        assert_eq!(screen.tab_bar.map(|r| r.y), Some(39));
        assert_eq!((screen.body.y, screen.body.height), (1, 38));
    }

    #[test]
    fn positions_change_the_ptys_body_and_emit_no_resize_for_a_same_height_move() {
        // Statusline on top keeps the body height: the PTY size is unchanged.
        let a = configured(80, 25, BarPosition::Top, BarPosition::Top, 1);
        assert_eq!(a.focused_size(), sized(24, 80));
        // A bar on top and the statusline at the bottom shrinks the body by one.
        let b = configured(80, 25, BarPosition::Bottom, BarPosition::Top, 2);
        assert_eq!(b.focused_size(), sized(23, 80));
    }

    #[test]
    fn the_initial_spawn_uses_the_configured_body_size() {
        let a = configured(80, 25, BarPosition::Top, BarPosition::Top, 1);
        let (_, spec) = a.initial_spawn();
        assert_eq!(spec.size, sized(24, 80));
    }

    #[test]
    fn a_new_tab_under_a_configured_layout_resizes_every_pane_once() {
        let mut a = configured(80, 25, BarPosition::Bottom, BarPosition::Bottom, 1);
        let effects = press(&mut a, "tn");
        let resizes: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::ResizePty(..)))
            .collect();
        assert_eq!(resizes, [&Effect::ResizePty(PaneId::FIRST, sized(23, 80))]);
    }

    #[test]
    fn tiny_terminals_never_panic_in_any_combination() {
        use BarPosition::{Bottom, Top};
        for (s, t) in [(Top, Top), (Top, Bottom), (Bottom, Top), (Bottom, Bottom)] {
            for n in [1, 2] {
                for (cols, rows) in [(0, 0), (1, 1), (1, 2), (3, 3), (5, 4)] {
                    let a = configured(cols, rows, s, t, n);
                    let screen = a.screen();
                    assert!(screen.body.width >= 1 && screen.body.height >= 1);
                    assert!(a.focused_size().rows >= 1 && a.focused_size().cols >= 1);
                }
            }
        }
    }

    // Spec: Startup notice.
    #[test]
    fn a_startup_notice_shows_until_the_next_key() {
        let mut a = launched(80, 25).with_notice(Some("config: bad".into()));
        assert_eq!(a.notice(), Some("config: bad"));
        assert_eq!(a.status_path(80), "config: bad");
        a.update(key('x'));
        assert_eq!(a.notice(), None);
        let b = launched(80, 25).with_notice(None);
        assert_eq!(b.notice(), None);
    }

    // Spec: config-menu Open and close; modal-input MENU mode.
    fn open_menu(a: &mut App) -> Vec<Effect> {
        press(a, "m")
    }

    // Spec: modal-input `m` is in the table; Not from other modes.
    #[test]
    fn prefix_m_opens_the_menu_from_the_real_table() {
        let mut a = launched(80, 25);
        a.update(ctrl_space());
        assert_eq!(a.input(), InputMode::Prefix);
        a.update(key('m'));
        assert_eq!(a.input(), InputMode::Menu);
        assert!(a.menu().is_some());
    }

    #[test]
    fn m_does_not_open_the_menu_from_other_modes() {
        let mut modes = Vec::new();
        for keys in ["[", "wr", "q", "tx"] {
            let mut a = launched(80, 25);
            press(&mut a, "wv");
            press(&mut a, keys);
            modes.push(a);
        }
        let mut group = launched(80, 25);
        press(&mut group, "w");
        modes.push(group);
        for mut a in modes {
            let before = a.input();
            a.update(key('m'));
            assert_ne!(a.input(), InputMode::Menu, "from {before:?}");
            assert!(a.menu().is_none(), "from {before:?}");
        }
    }

    fn menu_app() -> App {
        let mut a = launched(80, 25);
        open_menu(&mut a);
        a
    }

    fn code(c: KeyCode) -> AppEvent {
        AppEvent::Key(KeyEvent::new(c, KeyModifiers::NONE))
    }

    fn kind(c: KeyCode, kind: KeyEventKind) -> AppEvent {
        let mut ev = KeyEvent::new(c, KeyModifiers::NONE);
        ev.kind = kind;
        AppEvent::Key(ev)
    }

    fn config_of(statusline: BarPosition, tabbar: BarPosition) -> Config {
        Config {
            bars: BarPositions { statusline, tabbar },
            ..Config::default()
        }
    }

    #[test]
    fn opening_the_menu_enters_menu_without_writing() {
        let mut a = launched(80, 25);
        // The forced bar shrinks the body by one row: exactly one resize.
        let effects = open_menu(&mut a);
        assert_eq!(
            effects,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(23, 80)),
                Effect::DetectEngines
            ]
        );
        assert_eq!(a.input(), InputMode::Menu);
        assert!(a.menu().is_some());
    }

    #[test]
    fn esc_closes_the_menu_back_to_terminal() {
        let mut a = menu_app();
        let effects = a.update(esc());
        assert_eq!(
            effects,
            vec![Effect::ResizePty(PaneId::FIRST, sized(24, 80))]
        );
        assert_eq!(a.input(), InputMode::Terminal);
        assert!(a.menu().is_none());
    }

    #[test]
    fn reopening_starts_on_the_first_row_with_the_reverted_values() {
        let mut a = menu_app();
        a.update(key('j'));
        a.update(key('h'));
        a.update(esc());
        open_menu(&mut a);
        let rows = a.menu().unwrap().rows(&a.config);
        assert!(matches!(
            &rows[1],
            Row::Item {
                selected: true,
                value: Some(v),
                ..
            } if v == "bottom"
        ));
        assert!(matches!(
            &rows[2],
            Row::Item {
                selected: false,
                value: Some(v),
                ..
            } if v == "top"
        ));
    }

    // Spec: config-menu Live preview and revert.
    fn body_y(a: &App) -> u16 {
        a.screen().body.y
    }

    #[test]
    fn cycling_the_statusline_moves_the_screen_live() {
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        open_menu(&mut a);
        let panes_before = a.tiling().panes.len();
        // Same body height both ways, so no pane needs a new PTY size.
        assert_eq!(a.update(key('l')), vec![]);
        assert_eq!(a.screen().status.y, 1, "below the forced top tab bar");
        assert_eq!(body_y(&a), 2);
        assert!(a.tiling().panes.iter().all(|(_, rect)| rect.y == 2));
        assert_eq!(a.tiling().panes.len(), panes_before);
    }

    #[test]
    fn cycling_the_tab_bar_moves_it_live() {
        let mut a = launched(80, 25);
        press(&mut a, "tn");
        open_menu(&mut a);
        assert_eq!(a.screen().tab_bar.map(|r| r.y), Some(0));
        a.update(key('j'));
        assert_eq!(a.update(key('l')), vec![]);
        assert_eq!(a.screen().tab_bar.map(|r| r.y), Some(24));
    }

    #[test]
    fn a_preview_emits_resize_only_for_panes_whose_size_changed() {
        // Every position change keeps the body height, so nothing resizes:
        // the diff in `relayout` is what keeps it that way.
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        open_menu(&mut a);
        for k in ['l', 'l', 'h'] {
            let effects = a.update(key(k));
            assert!(
                !effects.iter().any(|e| matches!(e, Effect::ResizePty(..))),
                "{effects:?}"
            );
        }
    }

    #[test]
    fn esc_restores_the_original_positions_and_relayouts() {
        let mut a = launched(80, 25);
        press(&mut a, "tn");
        let before = a.screen();
        open_menu(&mut a);
        a.update(key('l'));
        a.update(key('j'));
        a.update(key('l'));
        assert_ne!(a.screen(), before);
        a.update(esc());
        assert_eq!(a.screen(), before);
        assert_eq!(a.config, Config::default());
    }

    #[test]
    fn an_external_edit_is_not_reloaded_because_the_app_never_reads_the_file() {
        // Characterization: `App` is pure, so Esc can only restore what it
        // captured at open, whatever the file says meanwhile.
        let start = config_of(BarPosition::Top, BarPosition::Bottom);
        let mut a = launched(80, 25).with_config(&start);
        open_menu(&mut a);
        a.update(key('l'));
        a.update(esc());
        assert_eq!(a.config, start);
    }

    // Spec: modal-input MENU mode; config-menu Input is swallowed.
    // Characterization: the swallow rules held from the moment the MENU
    // branch delegated to `MenuState`, so these pass at once.
    #[test]
    fn unlisted_keys_and_release_emit_nothing_and_stay_in_menu() {
        let ctrl = AppEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL));
        let events = [
            key('x'),
            code(KeyCode::Tab),
            ctrl,
            kind(KeyCode::Char('l'), KeyEventKind::Release),
        ];
        for ev in events {
            let mut a = menu_app();
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Menu, "{ev:?}");
            assert_eq!(a.config, Config::default(), "{ev:?}");
        }
    }

    #[test]
    fn paste_in_menu_writes_nothing() {
        let mut a = menu_app();
        assert_eq!(a.update(AppEvent::Paste("hi".into())), vec![]);
        assert_eq!(a.input(), InputMode::Menu);
    }

    // `repeat` comes from `key.kind` in `App::on_key` only.
    #[test]
    fn a_held_s_or_esc_does_nothing_but_a_held_j_moves() {
        for held in [KeyCode::Char('s'), KeyCode::Esc] {
            let mut a = menu_app();
            a.update(key('l'));
            assert_eq!(a.update(kind(held, KeyEventKind::Repeat)), vec![]);
            assert_eq!(a.input(), InputMode::Menu, "{held:?}");
            assert_eq!(a.config.bars.statusline, BarPosition::Top, "{held:?}");
        }
        let mut a = menu_app();
        a.update(kind(KeyCode::Char('j'), KeyEventKind::Repeat));
        a.update(key('l'));
        assert_eq!(
            a.config.bars.tabbar,
            BarPosition::Bottom,
            "j moved to row 2"
        );
    }

    // Spec: s with an unchanged draft closes without saving.
    #[test]
    fn s_with_an_unchanged_draft_closes_the_menu() {
        let mut a = menu_app();
        let effects = a.update(key('s'));
        assert_eq!(
            effects,
            vec![Effect::ResizePty(PaneId::FIRST, sized(24, 80))]
        );
        assert_eq!(a.input(), InputMode::Terminal);
        assert!(a.menu().is_none());
    }

    fn statusline_top_edit() -> ConfigEdit {
        ConfigEdit {
            table: "statusline",
            key: "position",
            value: "top".into(),
        }
    }

    // Spec: config-persistence Save on s.
    #[test]
    fn s_with_a_changed_draft_requests_a_save_and_keeps_the_menu_open() {
        let mut a = menu_app();
        a.update(key('l'));
        let effects = a.update(key('s'));
        assert_eq!(
            effects,
            vec![Effect::SaveConfig(vec![statusline_top_edit()])]
        );
        assert_eq!(a.input(), InputMode::Menu);
        assert!(a.menu().is_some());
        assert_eq!(a.config.bars.statusline, BarPosition::Top, "preview kept");
        assert_eq!(a.screen().tab_bar.map(|r| r.y), Some(0), "forced bar kept");
    }

    #[test]
    fn a_double_s_requests_two_identical_saves() {
        let mut a = menu_app();
        a.update(key('l'));
        let first = a.update(key('s'));
        let second = a.update(key('s'));
        assert_eq!(first, vec![Effect::SaveConfig(vec![statusline_top_edit()])]);
        assert_eq!(second, first);
    }

    // Spec: config-persistence Save closes.
    #[test]
    fn config_saved_closes_the_menu_and_keeps_the_draft() {
        let mut a = menu_app();
        a.update(key('l'));
        a.update(key('s'));
        let effects = a.update(AppEvent::ConfigSaved);
        assert_eq!(
            effects,
            vec![Effect::ResizePty(PaneId::FIRST, sized(24, 80))]
        );
        assert_eq!(a.input(), InputMode::Terminal);
        assert!(a.menu().is_none());
        assert_eq!(a.config.bars.statusline, BarPosition::Top, "draft kept");
        assert_eq!(a.screen().tab_bar, None, "the one-tab bar rule is back");
    }

    // Spec: config-persistence Write fails.
    #[test]
    fn config_save_failed_keeps_the_menu_the_draft_and_shows_the_error() {
        let mut a = menu_app();
        a.update(key('l'));
        a.update(key('s'));
        a.take_dirty();
        let effects = a.update(AppEvent::ConfigSaveFailed("save failed: disk full".into()));
        assert_eq!(effects, Vec::new());
        assert_eq!(a.input(), InputMode::Menu);
        assert_eq!(a.config.bars.statusline, BarPosition::Top, "preview kept");
        assert_eq!(a.screen().tab_bar.map(|r| r.y), Some(0), "forced bar kept");
        assert_eq!(
            a.menu().and_then(MenuState::error),
            Some("save failed: disk full")
        );
        assert!(a.take_dirty(), "the footer needs a redraw");
    }

    // Spec: config-persistence Retry and cancel after failure.
    #[test]
    fn the_next_key_clears_the_error_and_a_retry_can_succeed() {
        let mut a = menu_app();
        a.update(key('l'));
        a.update(key('s'));
        a.update(AppEvent::ConfigSaveFailed("save failed: x".into()));
        let retry = a.update(key('s'));
        assert_eq!(a.menu().and_then(MenuState::error), None, "cleared");
        assert_eq!(retry, vec![Effect::SaveConfig(vec![statusline_top_edit()])]);
        a.update(AppEvent::ConfigSaved);
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(a.config.bars.statusline, BarPosition::Top);
    }

    #[test]
    fn esc_after_a_failed_save_reverts_to_the_original() {
        let mut a = menu_app();
        a.update(key('l'));
        a.update(key('s'));
        a.update(AppEvent::ConfigSaveFailed("save failed: x".into()));
        a.update(esc());
        assert_eq!(a.input(), InputMode::Terminal);
        assert!(a.menu().is_none());
        assert_eq!(a.config.bars.statusline, BarPosition::Bottom, "reverted");
    }

    // Spec: config-menu Events while open; save results with no menu are stale.
    #[test]
    fn save_results_with_no_menu_open_are_ignored() {
        for event in [
            AppEvent::ConfigSaved,
            AppEvent::ConfigSaveFailed("late".into()),
        ] {
            let mut a = launched(80, 25);
            a.take_dirty();
            let before = a.config;
            assert_eq!(a.update(event), Vec::new());
            assert_eq!(a.input(), InputMode::Terminal);
            assert!(a.menu().is_none());
            assert_eq!(a.config, before);
            assert!(!a.take_dirty());
        }
    }

    // Spec: tabs Bar forced with one tab; config-menu Esc reverts the one-tab bar.
    fn resizes(effects: &[Effect]) -> Vec<(PaneId, u16)> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::ResizePty(id, size) => Some((*id, size.rows)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_forces_a_tab_bar_with_one_tab() {
        let mut a = launched(80, 25);
        assert_eq!(a.screen().tab_bar, None);
        assert_eq!(a.screen().body.height, 24);
        let effects = open_menu(&mut a);
        assert_eq!(a.screen().tab_bar.map(|r| r.y), Some(0));
        assert_eq!(a.screen().body.height, 23);
        assert_eq!(resizes(&effects), vec![(PaneId::FIRST, 23)]);
    }

    #[test]
    fn esc_and_s_hide_the_forced_bar_and_resize_back() {
        for close in [esc(), key('s')] {
            let mut a = menu_app();
            let effects = a.update(close);
            assert_eq!(a.screen().tab_bar, None);
            assert_eq!(a.screen().body.height, 24);
            assert_eq!(resizes(&effects), vec![(PaneId::FIRST, 24)]);
        }
    }

    #[test]
    fn with_two_panes_the_menu_resizes_each_pane_once_on_open_and_on_close() {
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        let id2 = id2();
        let opened = open_menu(&mut a);
        assert_eq!(
            opened,
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(23, 39)),
                Effect::ResizePty(id2, sized(23, 40)),
                Effect::DetectEngines,
            ]
        );
        assert_eq!(
            a.update(esc()),
            vec![
                Effect::ResizePty(PaneId::FIRST, sized(24, 39)),
                Effect::ResizePty(id2, sized(24, 40)),
            ]
        );
    }

    #[test]
    fn with_several_tabs_the_menu_never_resizes_for_the_bar() {
        let mut a = launched(80, 25);
        press(&mut a, "tn");
        let bar = a.screen().tab_bar;
        assert_eq!(open_menu(&mut a), vec![Effect::DetectEngines]);
        assert_eq!(a.screen().tab_bar, bar);
        assert!(a.update(esc()).is_empty());
        assert_eq!(a.screen().tab_bar, bar);
    }

    #[test]
    fn a_preview_with_several_tabs_moves_the_bar_without_a_resize() {
        let mut a = launched(80, 25);
        press(&mut a, "tn");
        open_menu(&mut a);
        a.update(key('j'));
        assert!(a.update(key('l')).is_empty());
        assert_eq!(a.screen().tab_bar.map(|r| r.y), Some(24));
        assert!(a.update(esc()).is_empty());
    }

    #[test]
    fn a_preview_with_one_tab_keeps_the_forced_body_height() {
        let mut a = menu_app();
        for k in ['l', 'j', 'l', 'h'] {
            let effects = a.update(key(k));
            assert!(resizes(&effects).is_empty(), "{effects:?}");
            assert_eq!(a.screen().body.height, 23);
        }
    }

    #[test]
    fn a_host_resize_between_open_and_esc_keeps_the_forced_bar_then_reverts() {
        let mut a = menu_app();
        let effects = a.update(AppEvent::Resize {
            cols: 100,
            rows: 30,
        });
        assert_eq!(resizes(&effects), vec![(PaneId::FIRST, 28)]);
        assert_eq!(a.screen().body.height, 28);
        assert_eq!(a.input(), InputMode::Menu);
        let effects = a.update(esc());
        assert_eq!(resizes(&effects), vec![(PaneId::FIRST, 29)]);
        assert_eq!(a.screen().tab_bar, None);
        assert_eq!(a.focused_size(), sized(29, 100));
    }

    #[test]
    fn the_forced_bar_never_panics_on_tiny_terminals() {
        for cols in 0..6 {
            for rows in 0..6 {
                let mut a = launched(cols, rows);
                open_menu(&mut a);
                let body = a.screen().body;
                assert!(body.width >= 1 && body.height >= 1, "{cols}x{rows}");
                a.update(esc());
                assert!(a.screen().body.height >= 1, "{cols}x{rows}");
            }
        }
    }

    // Spec: config-menu Events while open; modal-input Focus change keeps MENU.
    fn assert_still_open(a: &App) {
        assert_eq!(a.input(), InputMode::Menu);
        assert!(a.menu().is_some());
    }

    #[test]
    fn pane_output_is_still_parsed_while_the_menu_is_open() {
        let mut a = menu_app();
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"x".to_vec()),
        ));
        assert_eq!(a.focused_pane().cell(0, 0).unwrap().text, "x");
        assert_still_open(&a);
    }

    #[test]
    fn another_pane_exiting_keeps_the_menu_and_the_draft() {
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        open_menu(&mut a);
        a.update(key('l'));
        a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
        assert_still_open(&a);
        assert_eq!(a.config.bars.statusline, BarPosition::Top, "draft kept");
        assert_eq!(a.focused(), id2());
        assert_eq!(a.screen().body.height, 23);
        assert_eq!(a.focused_size().rows, 23);
    }

    #[test]
    fn the_focused_pane_exiting_moves_the_focus_and_keeps_the_menu() {
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        open_menu(&mut a);
        a.update(AppEvent::Pty(id2(), PtyEvent::Exited));
        assert_still_open(&a);
        assert_eq!(a.focused(), PaneId::FIRST);
    }

    #[test]
    fn a_background_tab_pane_exiting_keeps_the_menu_and_the_forced_bar() {
        let mut a = launched(80, 25);
        press(&mut a, "tn");
        open_menu(&mut a);
        // Closing the first tab leaves one tab: the bar stays because MENU is open.
        let effects = a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
        assert_still_open(&a);
        assert_eq!(a.tab_count(), 1);
        assert!(a.screen().tab_bar.is_some());
        assert_eq!(a.screen().body.height, 23);
        assert!(resizes(&effects).is_empty(), "{effects:?}");
        let effects = a.update(esc());
        assert_eq!(a.screen().tab_bar, None);
        assert_eq!(resizes(&effects), vec![(id2(), 24)]);
    }

    #[test]
    fn a_spawn_failure_keeps_the_menu_and_leaves_its_error_alone() {
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        open_menu(&mut a);
        a.update(AppEvent::SpawnFailed(id2(), "boom".into()));
        assert_still_open(&a);
        assert_eq!(a.menu().unwrap().error(), None);
        assert_eq!(a.notice(), Some("spawn failed: boom"));
    }

    #[test]
    fn a_host_resize_keeps_the_menu_with_the_forced_bar_and_preview() {
        let mut a = menu_app();
        a.update(key('l'));
        a.update(AppEvent::Resize { cols: 60, rows: 20 });
        assert_still_open(&a);
        assert_eq!(a.screen().tab_bar.map(|r| r.y), Some(0));
        assert_eq!(a.screen().status.y, 1);
        assert_eq!(a.screen().body.y, 2);
        assert_eq!(a.screen().body.height, 18);
    }

    #[test]
    fn closing_the_last_pane_of_the_last_tab_quits_with_the_menu_open() {
        let mut a = menu_app();
        let effects = a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
        assert_eq!(effects, vec![Effect::Quit]);
        assert_still_open(&a);
    }

    fn detection(status: crate::core::ai::Status) -> Detection {
        use crate::core::ai::CATALOG;
        Detection {
            engines: CATALOG
                .iter()
                .map(|e| crate::core::ai::EngineState {
                    id: e.id,
                    status,
                    models: Vec::new(),
                    version: None,
                })
                .collect(),
        }
    }

    fn texts(a: &App) -> Vec<String> {
        use crate::core::ai::{CATALOG, status_text};
        CATALOG
            .iter()
            .map(|e| status_text(a.engines(), e.id))
            .collect()
    }

    #[test]
    fn detection_is_single_flight() {
        let mut a = app();
        assert_eq!(a.request_detection(), vec![Effect::DetectEngines]);
        assert_eq!(a.request_detection(), Vec::<Effect>::new());
    }

    #[test]
    fn a_result_stores_the_detection_and_rearms_the_trigger() {
        use crate::core::ai::Status;
        let mut a = app();
        a.request_detection();
        let found = detection(Status::NotRunning);
        assert_eq!(
            a.update(AppEvent::EnginesDetected(found.clone())),
            Vec::<Effect>::new()
        );
        assert_eq!(a.engines(), Some(&found));
        assert_eq!(a.request_detection(), vec![Effect::DetectEngines]);
    }

    #[test]
    fn engines_read_detecting_until_the_whole_result_arrives() {
        use crate::core::ai::Status;
        let mut a = app();
        a.request_detection();
        assert_eq!(texts(&a), vec!["detecting…"; 8]);
        a.update(AppEvent::EnginesDetected(detection(Status::NotInstalled)));
        assert_eq!(texts(&a), vec!["not installed"; 8]);
    }

    #[test]
    fn a_failed_run_clears_the_flag_and_reads_unknown() {
        let mut a = app();
        a.request_detection();
        a.update(AppEvent::EnginesDetected(Detection::default()));
        assert_eq!(texts(&a), vec!["unknown"; 8]);
        assert_eq!(a.request_detection(), vec![Effect::DetectEngines]);
    }

    #[test]
    fn typing_is_not_held_back_while_detecting() {
        let mut a = app();
        a.request_detection();
        assert_eq!(
            a.update(key('a')),
            vec![Effect::WritePty(PaneId::FIRST, b"a".to_vec())]
        );
    }

    // AI menu wiring (spec: config-menu AI section; ai-engine-detection triggers).
    fn machine() -> Detection {
        use crate::core::ai::{CATALOG, EngineState, Status};
        let engine = |id: &'static str| match id {
            "ollama" => (Status::Available, vec!["llama3:8b", "qwen2:7b"]),
            "claude" => (Status::Available, vec!["default"]),
            _ => (Status::NotInstalled, vec![]),
        };
        Detection {
            engines: CATALOG
                .iter()
                .map(|e| {
                    let (status, models) = engine(e.id);
                    EngineState {
                        id: e.id,
                        status,
                        models: models.into_iter().map(str::to_owned).collect(),
                        version: None,
                    }
                })
                .collect(),
        }
    }

    fn ai_of(engine: &str, model: &str) -> AiConfig {
        AiConfig::general(Some(engine), Some(model))
    }

    /// Two tabs, so opening the menu never resizes; `ollama`/`qwen2:7b` saved.
    fn ai_app() -> App {
        let mut a = launched(80, 25).with_ai(ai_of("ollama", "qwen2:7b"));
        press(&mut a, "tn");
        a
    }

    // Spec: Three triggers (menu); a trigger while a run is in flight is a no-op.
    #[test]
    fn opening_the_menu_requests_a_detection_unless_one_is_in_flight() {
        let mut a = ai_app();
        assert_eq!(open_menu(&mut a), vec![Effect::DetectEngines]);
        a.update(esc());
        assert_eq!(open_menu(&mut a), Vec::<Effect>::new());
    }

    // Spec: config-menu Refresh key. Only a Press of lowercase `r` on a task
    // row runs a detection, and only when none is in flight.
    #[test]
    fn r_on_a_task_row_runs_one_detection_and_nothing_else_does() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        for _ in 0..4 {
            a.update(key('j'));
        }
        let before = a.menu().cloned();
        let upper = AppEvent::Key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT));
        assert_eq!(a.update(upper), Vec::<Effect>::new(), "uppercase");
        assert_eq!(a.update(key('R')), Vec::<Effect>::new(), "uppercase, bare");
        let held = kind(KeyCode::Char('r'), KeyEventKind::Repeat);
        assert_eq!(a.update(held), Vec::<Effect>::new(), "repeat");
        assert_eq!(a.update(key('r')), vec![Effect::DetectEngines], "press");
        assert_eq!(a.update(key('r')), Vec::<Effect>::new(), "in flight");
        // Only the refreshing cue differs from before the press.
        let mut expected = before.unwrap();
        expected.set_checking(true);
        assert_eq!(a.menu().cloned(), Some(expected));
    }

    // Spec: config-menu Task picker; AI actions never touch the layout or the
    // PTYs, so no key of the picker may produce an effect (no ResizePty).
    #[test]
    fn picker_keys_produce_no_effects() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        for _ in 0..4 {
            a.update(key('j'));
        }
        let enter = || code(KeyCode::Enter);
        let quiet = |a: &mut App, event: AppEvent| {
            assert_eq!(a.update(event.clone()), Vec::<Effect>::new(), "{event:?}");
        };
        // Open, move, choose an engine, move, back out, and choose again.
        quiet(&mut a, enter());
        for event in [key('j'), key('j'), enter(), key('j'), key('k'), esc()] {
            quiet(&mut a, event);
        }
        // Into the custom field of claude and type, delete, leave.
        quiet(&mut a, enter());
        for _ in 0..4 {
            quiet(&mut a, key('j'));
        }
        for event in [enter(), key('m'), key('s'), code(KeyCode::Backspace)] {
            quiet(&mut a, event);
        }
        for event in [esc(), esc(), esc()] {
            quiet(&mut a, event);
        }
        assert_eq!(a.input(), InputMode::Menu);
        assert!(a.menu().is_some());
        // Assign a model: the picker closes and the draft changes.
        for event in [enter(), key('j'), key('j'), enter(), key('j'), enter()] {
            quiet(&mut a, event);
        }
        assert_eq!(a.input(), InputMode::Menu);
        let draft = a.menu().unwrap().ai_draft();
        assert_eq!(
            draft.label(crate::core::ai::config::Task::Code),
            "claude › opus"
        );
        assert_eq!(
            a.ai(),
            &ai_of("ollama", "qwen2:7b"),
            "saved value untouched"
        );
    }

    // Spec: Unchanged AI draft.
    #[test]
    fn an_unchanged_ai_draft_closes_without_a_save() {
        let mut a = launched(80, 25).with_ai(ai_of("ollama", "qwen2:7b"));
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        assert_eq!(
            a.update(key('s')),
            vec![Effect::ResizePty(PaneId::FIRST, sized(24, 80))]
        );
        assert_eq!(a.ai(), &ai_of("ollama", "qwen2:7b"));
    }

    /// Opens the picker of `Set all tasks` on `none` and chooses it: the
    /// general assignment goes from `ollama` to unset.
    fn clear_all(a: &mut App) {
        for _ in 0..2 {
            a.update(key('j'));
        }
        a.update(code(KeyCode::Enter));
        a.update(key('k'));
        a.update(code(KeyCode::Enter));
    }

    // Spec: save before the first detection result.
    #[test]
    fn s_saves_the_ai_draft_before_the_first_detection() {
        let mut a = ai_app();
        open_menu(&mut a);
        clear_all(&mut a);
        let effects = a.update(key('s'));
        let edit = |key, value: &'static str| ConfigEdit {
            table: "ai",
            key,
            value: value.into(),
        };
        assert_eq!(
            effects,
            vec![Effect::SaveConfig(vec![
                edit("engine", ""),
                edit("model", "")
            ])]
        );
        assert_eq!(a.input(), InputMode::Menu);
    }

    // Spec: Esc reverts the AI draft along with the settings.
    #[test]
    fn esc_discards_the_ai_draft_and_the_next_menu_starts_from_the_saved_value() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        clear_all(&mut a);
        assert_eq!(a.menu().unwrap().ai_draft(), AiConfig::default());
        a.update(esc());
        assert!(a.menu().is_none());
        assert_eq!(a.ai(), &ai_of("ollama", "qwen2:7b"));
        open_menu(&mut a);
        assert_eq!(a.menu().unwrap().ai_draft(), ai_of("ollama", "qwen2:7b"));
    }

    // Spec (config-menu, engram-memory): the privacy warning follows the
    // draft flow. `k` from the first row wraps to `Memory`.
    fn memory_warning_open() -> App {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        a.update(key('k'));
        assert!(a.update(key('l')).is_empty(), "turning on asks, no effect");
        assert!(a.menu().unwrap().in_warning());
        a
    }

    fn memory_edits() -> Vec<ConfigEdit> {
        vec![
            ConfigEdit::memory_flag("enabled", true),
            ConfigEdit::memory_flag("acknowledged", true),
        ]
    }

    #[test]
    fn accepting_the_warning_writes_nothing_until_s_and_saves_both_keys() {
        let mut a = memory_warning_open();
        assert!(a.update(key('y')).is_empty(), "y persists nothing");
        assert!(!a.ai().memory.enabled, "the live value is untouched");
        assert_eq!(a.update(key('s')), vec![Effect::SaveConfig(memory_edits())]);
        a.update(AppEvent::ConfigSaved);
        assert!(a.menu().is_none());
        assert!(a.ai().memory.active());
    }

    // Review W2: after a resize into the one-line fallback the facts are
    // hidden, so `y` must not accept.
    #[test]
    fn y_in_the_warning_does_not_accept_after_a_resize_to_a_tiny_terminal() {
        let mut a = memory_warning_open();
        a.update(AppEvent::Resize { cols: 30, rows: 5 });
        assert!(a.update(key('y')).is_empty());
        let menu = a.menu().unwrap();
        assert!(!menu.in_warning());
        assert_eq!(menu.ai_draft().memory, Default::default());
        assert!(menu.edits(a.config()).is_empty());
    }

    #[test]
    fn esc_after_accepting_reverts_the_enabled_and_acknowledged_drafts() {
        let mut a = memory_warning_open();
        a.update(key('y'));
        a.update(esc());
        assert!(a.menu().is_none());
        assert_eq!(a.ai().memory, Default::default());
        open_menu(&mut a);
        assert_eq!(a.menu().unwrap().ai_draft().memory, Default::default());
        a.update(key('k'));
        a.update(key('l'));
        assert!(
            a.menu().unwrap().in_warning(),
            "asked again, not acknowledged"
        );
    }

    #[test]
    fn esc_in_the_warning_closes_only_the_view() {
        let mut a = memory_warning_open();
        assert!(a.update(esc()).is_empty());
        let menu = a.menu().expect("the menu stays open");
        assert!(!menu.in_warning());
        assert!(!menu.ai_draft().memory.enabled);
    }

    /// Moves the menu selection to the `Context` row (before `Memory`, the
    /// last one).
    fn to_context_row(a: &mut App) {
        a.update(key('k'));
        a.update(key('k'));
    }

    /// Moves the menu selection to the `Profile` row (before `Context`).
    fn to_profile_row(a: &mut App) {
        a.update(key('k'));
        a.update(key('k'));
        a.update(key('k'));
    }

    // Spec (config-menu): Profile row. Cycling is silent, `s` saves only the
    // profile key, and the saved value becomes the app's profile.
    #[test]
    fn the_profile_row_saves_only_the_profile_and_updates_the_app() {
        use crate::core::ai::config::Profile;
        let mut a = ai_app();
        open_menu(&mut a);
        to_profile_row(&mut a);
        assert_eq!(a.update(key('l')), Vec::<Effect>::new(), "no ResizePty");
        assert_eq!(a.update(code(KeyCode::Enter)), Vec::<Effect>::new());
        assert_eq!(a.update(key('h')), Vec::<Effect>::new());
        assert_eq!(a.menu().unwrap().ai_draft().profile(), Profile::Analyst);
        assert_eq!(a.ai().profile(), Profile::Programmer, "not saved yet");
        let edit = ConfigEdit {
            table: "ai",
            key: "profile",
            value: "analyst".into(),
        };
        assert_eq!(a.update(key('s')), vec![Effect::SaveConfig(vec![edit])]);
        a.update(AppEvent::ConfigSaved);
        assert!(a.menu().is_none());
        assert_eq!(a.ai().profile(), Profile::Analyst);
        let general = crate::core::ai::config::Task::General;
        assert_eq!(
            a.ai().get(general),
            ai_of("ollama", "qwen2:7b").get(general)
        );
    }

    // Spec (config-menu): Esc restores the profile captured at open.
    #[test]
    fn esc_reverts_the_profile_draft() {
        use crate::core::ai::config::Profile;
        let mut a = ai_app();
        open_menu(&mut a);
        to_profile_row(&mut a);
        a.update(key('l'));
        assert_eq!(a.menu().unwrap().ai_draft().profile(), Profile::Analyst);
        a.update(esc());
        assert!(a.menu().is_none());
        assert_eq!(a.ai().profile(), Profile::Programmer);
        open_menu(&mut a);
        assert_eq!(a.menu().unwrap().ai_draft().profile(), Profile::Programmer);
    }

    // Spec (Context toggle): `s` saves only the context key and the saved
    // value becomes the app's flag.
    #[test]
    fn the_context_row_saves_only_the_flag_and_updates_the_app() {
        let mut a = ai_app();
        assert!(a.ai().context());
        open_menu(&mut a);
        to_context_row(&mut a);
        assert_eq!(a.update(key('l')), Vec::<Effect>::new(), "no ResizePty");
        assert!(!a.menu().unwrap().ai_draft().context());
        assert!(a.ai().context(), "not saved yet");
        let edit = ConfigEdit {
            table: "ai",
            key: "context",
            value: "false".into(),
        };
        assert_eq!(a.update(key('s')), vec![Effect::SaveConfig(vec![edit])]);
        a.update(AppEvent::ConfigSaved);
        assert!(a.menu().is_none());
        assert!(!a.ai().context());
    }

    // Spec (Context toggle): Esc restores the flag captured at open.
    #[test]
    fn esc_reverts_the_context_draft() {
        let mut a = ai_app();
        open_menu(&mut a);
        to_context_row(&mut a);
        a.update(key('l'));
        a.update(esc());
        assert!(a.menu().is_none());
        assert!(a.ai().context());
        open_menu(&mut a);
        assert!(a.menu().unwrap().ai_draft().context());
    }

    // Spec (config-menu): an unchanged profile does not touch the file.
    #[test]
    fn an_untouched_profile_saves_nothing() {
        let mut a = ai_app();
        open_menu(&mut a);
        to_profile_row(&mut a);
        a.update(key('l'));
        a.update(key('l'));
        let effects = a.update(key('s'));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SaveConfig(_))),
            "{effects:?}"
        );
    }

    // Spec: a failed save keeps the picker where it was.
    #[test]
    fn a_failed_save_keeps_the_picker_level_and_shows_the_error() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        clear_all(&mut a);
        for _ in 0..2 {
            a.update(key('j'));
        }
        a.update(code(KeyCode::Enter));
        a.update(key('j'));
        a.update(key('j'));
        a.update(code(KeyCode::Enter));
        let before = a.menu().unwrap().clone();
        a.update(key('s'));
        a.update(AppEvent::ConfigSaveFailed("save failed: disk full".into()));
        let menu = a.menu().unwrap();
        assert_eq!(menu.error(), Some("save failed: disk full"));
        assert_eq!(menu.rows(&a.config), before.rows(&a.config));
        assert_eq!(a.input(), InputMode::Menu);
    }

    fn footer_of(a: &App) -> String {
        use crate::core::menu::Footer;
        match a.menu().unwrap().footer() {
            Footer::Hint(text) => text.to_string(),
            Footer::Error(text) => format!("error: {text}"),
        }
    }

    /// Selection on the first AI task row.
    fn to_task_row(a: &mut App) {
        for _ in 0..4 {
            a.update(key('j'));
        }
    }

    /// The picker of the first task, on the engine two rows down, at level 2.
    fn to_level_two(a: &mut App) {
        to_task_row(a);
        for event in [
            code(KeyCode::Enter),
            key('j'),
            key('j'),
            code(KeyCode::Enter),
        ] {
            a.update(event);
        }
    }

    /// The custom field of that engine's last model row.
    fn to_custom_field(a: &mut App) {
        to_level_two(a);
        for _ in 0..4 {
            a.update(key('j'));
        }
        a.update(code(KeyCode::Enter));
    }

    // Spec: config-menu Paste. A paste lands in the custom field, never in a PTY.
    #[test]
    fn a_paste_reaches_the_custom_field_and_never_the_pty() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        to_custom_field(&mut a);
        assert!(footer_of(&a).starts_with("type a model"));
        assert_eq!(a.update(AppEvent::Paste("opus\nignored".into())), vec![]);
        a.update(code(KeyCode::Enter));
        let draft = a.menu().unwrap().ai_draft();
        assert_eq!(
            draft.label(crate::core::ai::config::Task::Code),
            "claude › opus"
        );
    }

    // Characterization: the picker sanitises and caps a paste; the App only forwards it.
    #[test]
    fn a_paste_with_control_characters_and_a_long_value_is_sanitised_and_capped() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        to_custom_field(&mut a);
        let pasted = format!("a\u{7}b\u{1b}[c d\u{200b}e{}", "z".repeat(200));
        assert_eq!(a.update(AppEvent::Paste(pasted)), vec![]);
        a.update(code(KeyCode::Enter));
        let label = a
            .menu()
            .unwrap()
            .ai_draft()
            .label(crate::core::ai::config::Task::Code);
        let model = label.strip_prefix("claude › ").expect(&label);
        assert!(model.starts_with("ab[c"), "{model}");
        assert!(model.chars().all(|c| !c.is_control() && !c.is_whitespace()));
        assert_eq!(model.chars().count(), 128);
    }

    #[test]
    fn a_paste_is_ignored_outside_the_custom_field() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        let steps: [&[AppEvent]; 4] = [
            &[],
            &[key('j'), key('j'), key('j'), key('j')],
            &[code(KeyCode::Enter)],
            &[key('j'), key('j'), code(KeyCode::Enter)],
        ];
        // Settings row, task row, picker level 1, picker level 2.
        for events in steps {
            for event in events {
                a.update(event.clone());
            }
            let before = a.menu().cloned();
            assert_eq!(a.update(AppEvent::Paste("x".into())), vec![]);
            assert_eq!(a.menu().cloned(), before);
        }
    }

    // Spec: config-menu Checking cue. The cue follows the run in flight.
    #[test]
    fn opening_the_menu_shows_checking_until_the_first_result() {
        let mut a = ai_app();
        open_menu(&mut a);
        to_task_row(&mut a);
        assert!(footer_of(&a).starts_with("checking engines…"));
        a.update(AppEvent::EnginesDetected(machine()));
        assert!(footer_of(&a).starts_with("j/k move"));
    }

    #[test]
    fn opening_the_menu_with_a_result_in_hand_shows_refreshing_until_the_new_one() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        a.update(esc());
        open_menu(&mut a);
        to_task_row(&mut a);
        assert!(
            footer_of(&a).starts_with("refreshing…"),
            "{}",
            footer_of(&a)
        );
        a.update(AppEvent::EnginesDetected(machine()));
        assert!(footer_of(&a).starts_with("j/k move"));
    }

    #[test]
    fn a_run_already_in_flight_at_open_shows_refreshing() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        a.update(esc());
        a.request_detection();
        assert_eq!(open_menu(&mut a), vec![]);
        to_task_row(&mut a);
        assert!(
            footer_of(&a).starts_with("refreshing…"),
            "{}",
            footer_of(&a)
        );
    }

    #[test]
    fn refresh_shows_refreshing_until_the_result_arrives() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        to_task_row(&mut a);
        assert!(footer_of(&a).starts_with("j/k move"));
        assert_eq!(a.update(key('r')), vec![Effect::DetectEngines]);
        assert!(
            footer_of(&a).starts_with("refreshing…"),
            "{}",
            footer_of(&a)
        );
        assert_eq!(a.update(key('r')), vec![]);
        assert!(footer_of(&a).starts_with("refreshing…"));
        a.update(AppEvent::EnginesDetected(machine()));
        assert!(footer_of(&a).starts_with("j/k move"));
    }

    #[test]
    fn a_failed_run_also_clears_the_cue() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        to_task_row(&mut a);
        a.update(key('r'));
        a.update(AppEvent::EnginesDetected(Detection::default()));
        assert!(!footer_of(&a).starts_with("refreshing…"));
    }

    // Spec: config-menu Events while open. A pane exiting never moves the picker.
    #[test]
    fn a_pane_exiting_keeps_the_picker_at_level_two() {
        let mut a = launched(80, 25);
        press(&mut a, "wv");
        press(&mut a, "tn");
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        to_level_two(&mut a);
        let before = a.menu().cloned();
        a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
        a.update(AppEvent::SpawnFailed(pid(9), "boom".into()));
        a.update(AppEvent::Resize {
            cols: 100,
            rows: 30,
        });
        assert_still_open(&a);
        assert_eq!(a.menu().cloned(), before);
        // One Esc goes back to level 1, not out of the picker.
        a.update(esc());
        assert!(footer_of(&a).contains("Enter choose"), "{}", footer_of(&a));
    }

    // Spec: minimal fallback. The popup may not fit, the keys still work.
    #[test]
    fn s_and_esc_work_on_a_terminal_too_small_for_the_popup() {
        let mut a = launched(20, 8);
        open_menu(&mut a);
        a.update(key('l'));
        assert!(!a.update(key('s')).is_empty());
        let mut b = launched(20, 8);
        open_menu(&mut b);
        b.update(key('l'));
        b.update(esc());
        assert_eq!(b.input(), InputMode::Terminal);
        assert_eq!(b.config, Config::default());
    }

    // Review W2: Enter on Settings never saves and never closes.
    #[test]
    fn enter_never_saves_or_closes_the_menu() {
        let mut a = ai_app();
        open_menu(&mut a);
        a.update(AppEvent::EnginesDetected(machine()));
        for _ in 0..6 {
            let effects = a.update(code(KeyCode::Enter));
            assert!(
                !effects.iter().any(|e| matches!(e, Effect::SaveConfig(_))),
                "{effects:?}"
            );
            assert_still_open(&a);
            a.update(key('j'));
        }
    }

    // Review S1: a saved engine that is not detected is never rewritten.
    #[test]
    fn an_undetected_saved_engine_survives_open_refresh_save_and_reload() {
        let saved = ai_of("codex", "default");
        let mut a = launched(80, 25).with_ai(saved.clone());
        press(&mut a, "tn");
        open_menu(&mut a);
        assert_eq!(a.menu().unwrap().ai_draft(), saved, "open");
        a.update(AppEvent::EnginesDetected(machine()));
        assert_eq!(a.menu().unwrap().ai_draft(), saved, "detected");
        to_task_row(&mut a);
        a.update(key('r'));
        a.update(AppEvent::EnginesDetected(machine()));
        assert_eq!(a.menu().unwrap().ai_draft(), saved, "refreshed");
        // Change an unrelated setting: the save carries no [ai] key.
        for _ in 0..4 {
            a.update(key('k'));
        }
        a.update(key('l'));
        let effects = a.update(key('s'));
        let Some(Effect::SaveConfig(edits)) = effects.first() else {
            panic!("{effects:?}");
        };
        assert!(edits.iter().all(|e| e.table != "ai"), "{edits:?}");
        a.update(AppEvent::ConfigSaved);
        assert_eq!(a.ai(), &saved, "saved");
        // Reload: the file still says codex.
        let text = "[ai]\nengine = \"codex\"\nmodel = \"default\"\n";
        let (_, ai, _) = Config::load_with_ai(Ok(text.to_owned()), None);
        assert_eq!(App::new(80, 25).with_ai(ai).ai(), &saved, "reload");
    }

    // Characterization: the file's values reach the App through `with_ai`.
    #[test]
    fn with_ai_carries_the_files_values() {
        let text = "[ai]\nengine = \"codex\"\nmodel = \"default\"\n";
        let (_, ai, notice) = Config::load_with_ai(Ok(text.to_owned()), None);
        assert_eq!(notice, None);
        assert_eq!(
            App::new(80, 25).with_ai(ai).ai(),
            &ai_of("codex", "default")
        );
        assert_eq!(App::new(80, 25).ai(), &AiConfig::default());
    }

    /// The live chat pane (ai-chat C8).
    mod chat_live;

    /// Memory in a chat view (engram-memory M6).
    mod chat_memory;

    /// Save as note (engram-memory M7b).
    mod chat_notes;

    /// Pane-kind seam (ai-chat C1). Characterization of the PTY paths the seam
    /// must leave untouched, then the chat-pane behaviour.
    mod seam {
        use super::*;

        // Characterization: a hidden tab's label follows the cwd poll.
        #[test]
        fn a_hidden_tabs_label_follows_the_poll_cwd() {
            let mut a = launched(80, 25);
            press(&mut a, "tn");
            a.update(cwd_event("/srv/www"));
            assert_eq!(a.tab_labels(), vec!["1 www".to_string(), "2".to_string()]);
        }

        // Characterization: a pane that exits in a hidden tab leaves the rest alone.
        #[test]
        fn a_pane_exiting_in_a_hidden_tab_drops_only_that_pane() {
            let mut a = launched(80, 25);
            press(&mut a, "wv");
            press(&mut a, "tn");
            assert_eq!(
                a.update(exited(id2())),
                vec![Effect::ResizePty(PaneId::FIRST, sized(23, 80))]
            );
            assert!(a.pane(id2()).is_none());
            assert!(a.pane(PaneId::FIRST).is_some());
            assert_eq!(a.tab_count(), 2);
        }

        fn chat_id() -> PaneId {
            id2()
        }

        // 1.1: a chat pane is laid out like any pane but has no PTY.
        #[test]
        fn a_chat_split_spawns_nothing_and_resizes_only_the_shell() {
            let mut a = launched(80, 25);
            let effects = a.open_chat(Some("/srv".into()));
            assert_eq!(
                effects,
                vec![Effect::ResizePty(PaneId::FIRST, sized(24, 39))]
            );
            assert_eq!(a.focused(), chat_id());
            assert_eq!(a.focused_size(), sized(24, 40));
            assert!(a.pane(chat_id()).is_none(), "a chat has no emulator");
        }

        #[test]
        fn a_chat_tab_spawns_nothing() {
            let mut a = launched(80, 25);
            let effects = a.open_chat_tab(None);
            // Only the new tab bar shrinks the first shell; nothing is spawned.
            assert_eq!(
                effects,
                vec![Effect::ResizePty(PaneId::FIRST, sized(23, 80))]
            );
            assert_eq!((a.tab_count(), a.focused()), (2, chat_id()));
        }

        #[test]
        fn a_terminal_resize_keeps_the_chat_size_without_telling_a_pty() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            let effects = a.update(AppEvent::Resize {
                cols: 100,
                rows: 30,
            });
            assert_eq!(
                effects,
                vec![Effect::ResizePty(PaneId::FIRST, sized(29, 48))]
            );
            assert_eq!(a.focused_size(), sized(29, 51));
        }

        #[test]
        fn closing_a_chat_pane_emits_chat_cancel_and_never_close_pane() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            press(&mut a, "wq");
            assert_eq!(a.input(), InputMode::Confirm(Confirm::ClosePane));
            let effects = a.update(key('y'));
            assert_eq!(
                effects,
                vec![
                    Effect::ResizePty(PaneId::FIRST, sized(24, 80)),
                    Effect::ChatCancel(chat_id()),
                ]
            );
            assert_eq!(a.focused(), PaneId::FIRST);
        }

        #[test]
        fn closing_a_tab_cancels_its_chats_and_closes_only_its_shells() {
            let mut a = launched(80, 25);
            a.open_chat_tab(None);
            press(&mut a, "wv");
            let shell = id3();
            press(&mut a, "tc");
            let effects = a.update(key('y'));
            assert!(effects.contains(&Effect::ClosePane(shell)), "{effects:?}");
            assert!(
                effects.contains(&Effect::ChatCancel(chat_id())),
                "{effects:?}"
            );
            assert!(
                !effects.contains(&Effect::ClosePane(chat_id())),
                "{effects:?}"
            );
            assert_eq!(a.tab_count(), 1);
        }

        #[test]
        fn pty_and_cwd_events_for_a_chat_id_are_dropped() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            a.take_dirty();
            for event in [
                AppEvent::Pty(chat_id(), PtyEvent::Output(b"x".to_vec())),
                AppEvent::Pty(chat_id(), PtyEvent::Exited),
                AppEvent::Cwd(chat_id(), "/x".into()),
                AppEvent::SpawnFailed(chat_id(), "boom".into()),
            ] {
                assert_eq!(a.update(event.clone()), vec![], "{event:?}");
            }
            assert!(!a.take_dirty());
            assert_eq!(a.focused(), chat_id(), "the chat pane survives");
            assert_eq!(a.notice(), None);
        }

        #[test]
        fn the_ids_of_chats_and_shells_are_shared_and_never_reused() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            press(&mut a, "wv");
            assert_eq!(a.focused(), id3());
            press(&mut a, "wq");
            a.update(key('y'));
            press(&mut a, "wv");
            assert_eq!(a.focused(), PaneId::for_test(4));
        }

        // 1.2: the prefix still works on a chat, but nothing reaches a PTY.
        #[test]
        fn copy_on_a_chat_is_refused_with_the_notice() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            let effects = press(&mut a, "[");
            assert_eq!(effects, vec![]);
            assert_eq!(a.input(), InputMode::Terminal);
            assert_eq!(
                a.notice(),
                Some("copy mode is not available in chat panes; Ctrl+Y copies the last answer")
            );
            assert_eq!(a.copy_position(), None);
            assert!(a.take_dirty());
        }

        #[test]
        fn the_notice_goes_away_with_the_next_key() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            press(&mut a, "[");
            a.update(key('x'));
            assert_eq!(a.notice(), None);
        }

        #[test]
        fn a_second_prefix_on_a_chat_is_swallowed() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            a.update(ctrl_space());
            assert_eq!(a.input(), InputMode::Prefix);
            assert_eq!(a.update(ctrl_space()), vec![]);
            assert_eq!(a.input(), InputMode::Terminal);
        }

        #[test]
        fn a_second_prefix_on_a_shell_still_sends_the_literal() {
            let mut a = launched(80, 25);
            a.update(ctrl_space());
            assert_eq!(
                a.update(ctrl_space()),
                vec![Effect::WritePty(PaneId::FIRST, vec![0])]
            );
        }

        #[test]
        fn chat_keys_and_pastes_never_write_a_pty() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            let enter = AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            for event in [key('x'), enter, esc(), AppEvent::Paste("hi\nthere".into())] {
                assert_eq!(a.update(event.clone()), vec![], "{event:?}");
            }
            assert_eq!(a.input(), InputMode::Terminal);
        }

        #[test]
        fn repeat_rules_hold_on_a_chat() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            let repeat = |code, modifiers| {
                AppEvent::Key(KeyEvent::new_with_kind(
                    code,
                    modifiers,
                    KeyEventKind::Repeat,
                ))
            };
            assert_eq!(
                a.update(repeat(KeyCode::Char('x'), KeyModifiers::NONE)),
                vec![]
            );
            a.update(ctrl_space());
            // A held key must not trigger a one-shot decision in PREFIX.
            assert_eq!(
                a.update(repeat(KeyCode::Char('['), KeyModifiers::NONE)),
                vec![]
            );
            assert_eq!(a.input(), InputMode::Prefix);
            assert_eq!(a.notice(), None);
        }

        // 1.3: a chat's effective cwd is the cwd it was opened from.
        #[test]
        fn the_tab_label_comes_from_the_chat_origin_cwd() {
            let mut a = launched(80, 25);
            a.open_chat_tab(Some("/srv/www".into()));
            assert_eq!(
                a.tab_labels(),
                vec!["1 launch".to_string(), "2 www".to_string()]
            );
            assert_eq!(a.cwd_label(), "/srv/www");
        }

        #[test]
        fn a_chat_without_an_origin_cwd_has_a_bare_label() {
            let mut a = launched(80, 25);
            a.open_chat_tab(None);
            assert_eq!(
                a.tab_labels(),
                vec!["1 launch".to_string(), "2".to_string()]
            );
            assert_eq!(a.cwd_label(), "");
        }

        #[test]
        fn a_shell_split_from_a_chat_inherits_its_origin_cwd() {
            let mut a = launched(80, 25);
            a.open_chat(Some("/srv".into()));
            let effects = press(&mut a, "wv");
            assert_eq!(
                effects.last(),
                Some(&Effect::SpawnPane(id3(), spec("/srv", sized(24, 20))))
            );
        }

        #[test]
        fn a_new_tab_from_a_chat_inherits_its_origin_cwd() {
            let mut a = launched(80, 25);
            a.open_chat(Some("/srv".into()));
            let effects = press(&mut a, "tn");
            assert_eq!(
                effects.last(),
                Some(&Effect::SpawnPane(id3(), spec("/srv", sized(23, 80))))
            );
        }

        #[test]
        fn a_vanished_origin_cwd_falls_back_to_the_launch_cwd() {
            let mut a = launched(80, 25).with_dir_check(|dir| dir != Path::new("/gone"));
            a.open_chat(Some("/gone".into()));
            let effects = press(&mut a, "tn");
            assert_eq!(
                effects.last(),
                Some(&Effect::SpawnPane(id3(), spec(LAUNCH, sized(23, 80))))
            );
        }

        #[test]
        fn a_chat_can_be_the_last_pane_of_the_last_tab() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            let effects = a.update(exited(PaneId::FIRST));
            assert!(!effects.contains(&Effect::Quit), "{effects:?}");
            assert_eq!((a.tab_count(), a.focused()), (1, chat_id()));
            press(&mut a, "wq");
            assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
            assert_eq!(a.update(key('y')), vec![Effect::Quit]);
        }

        // 1.4: the UI asks for a view, not for an emulator.
        #[test]
        fn pane_view_names_the_kind_of_each_pane() {
            let mut a = launched(80, 25);
            a.open_chat(Some("/srv".into()));
            assert!(matches!(a.pane_view(PaneId::FIRST), Some(PaneView::Pty(_))));
            let Some(PaneView::Chat(chat)) = a.pane_view(chat_id()) else {
                panic!("a chat pane has a chat view");
            };
            assert_eq!(chat.origin_cwd(), Some(Path::new("/srv")));
            assert!(a.pane_view(PaneId::for_test(9)).is_none());
        }

        // A late poll result for a chat id must not give the chat a shell cwd.
        #[test]
        fn a_cwd_event_for_a_chat_leaves_its_cwd_alone() {
            let mut a = launched(80, 25);
            a.open_chat(Some("/srv".into()));
            a.update(AppEvent::Cwd(chat_id(), "/elsewhere".into()));
            assert_eq!(a.cwd_label(), "/srv");
            let Some(PaneView::Chat(chat)) = a.pane_view(chat_id()) else {
                panic!("a chat pane has a chat view");
            };
            assert_eq!(chat.origin_cwd(), Some(Path::new("/srv")));
        }

        // Fuzz: random input over both pane kinds never panics and never
        // sends a PTY effect for a chat. Fixed seed, so it is reproducible.
        #[test]
        fn random_events_never_reach_a_pty_through_a_chat() {
            let mut seed: u64 = 0x5eed_c0de;
            let mut next = move |n: u64| {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (seed >> 33) % n
            };
            let mut a = launched(80, 25);
            let mut chats: Vec<PaneId> = Vec::new();
            for _ in 0..4000 {
                let id = PaneId::for_test(1 + next(14) as u32);
                let keys = "[wvhjklqytncns0123 xyz?\n";
                let ch = keys.chars().nth(next(keys.len() as u64) as usize).unwrap();
                let effects = match next(12) {
                    0..=2 => {
                        let seqs = ["wv", "wh", "wq", "wqy", "tn", "tc", "tcy", "[", "hl", " "];
                        press(&mut a, seqs[next(seqs.len() as u64) as usize])
                    }
                    3..=4 => a.update(key(ch)),
                    5 => a.update(esc()),
                    6 => {
                        let e = a.open_chat(Some("/srv".into()));
                        if !a.is_shell(a.focused()) {
                            chats.push(a.focused());
                        }
                        e
                    }
                    7 => {
                        let e = a.open_chat_tab(None);
                        if !a.is_shell(a.focused()) {
                            chats.push(a.focused());
                        }
                        e
                    }
                    8 => a.update(exited(id)),
                    9 => a.update(AppEvent::Resize {
                        cols: 20 + next(100) as u16,
                        rows: 5 + next(40) as u16,
                    }),
                    10 => match next(4) {
                        0 => a.update(AppEvent::Cwd(id, "/x".into())),
                        1 => a.update(AppEvent::Paste("p".into())),
                        2 => a.update(AppEvent::Pty(id, PtyEvent::Output(b"o\r\n".to_vec()))),
                        _ => a.update(AppEvent::SpawnFailed(id, "boom".into())),
                    },
                    _ => a.update(key('y')),
                };
                for effect in effects {
                    let target = match effect {
                        Effect::WritePty(id, _)
                        | Effect::ResizePty(id, _)
                        | Effect::ClosePane(id)
                        | Effect::SpawnPane(id, _) => id,
                        _ => continue,
                    };
                    assert!(!chats.contains(&target), "{effect:?} targets a chat");
                }
            }
        }
    }

    /// Chat streams (ai-chat C5): `AppEvent::Chat` routing.
    mod chat_stream {
        use super::*;
        use crate::core::ai::chat::{
            ANSWER_CAP, Entry, MsgStatus, Phase,
            engine::{ChatError, Finish},
        };

        fn chat_id() -> PaneId {
            id2()
        }

        fn delta(id: PaneId, turn: u64, text: &str) -> AppEvent {
            AppEvent::Chat(id, turn, StreamEvent::Delta(text.into()))
        }

        fn finish(id: PaneId, turn: u64) -> AppEvent {
            AppEvent::Chat(id, turn, StreamEvent::Done(Finish::Stop))
        }

        fn chat_of(a: &App, id: PaneId) -> &ChatState {
            match a.pane_view(id) {
                Some(PaneView::Chat(chat)) => chat,
                _ => panic!("pane {id:?} is not a chat"),
            }
        }

        /// The text and status of the last message of chat `id`.
        fn last(a: &App, id: PaneId) -> (String, MsgStatus) {
            let msg = chat_of(a, id).messages().last().expect("a message");
            (msg.text.clone(), msg.status)
        }

        fn turn_of(effects: &[Effect]) -> u64 {
            match effects {
                [Effect::ChatSend { turn, .. }] => *turn,
                other => panic!("expected one ChatSend, got {other:?}"),
            }
        }

        /// An app with a chat in pane 2 that has sent "hi" as turn 1.
        fn streaming() -> App {
            let mut a = launched(80, 25);
            a.open_chat(None);
            assert_eq!(turn_of(&a.chat_send(chat_id(), "hi")), 1);
            a
        }

        #[test]
        fn events_are_applied_in_order_and_the_final_one_ends_the_turn() {
            let mut a = streaming();
            a.take_dirty();
            for event in [delta(chat_id(), 1, "Hel"), delta(chat_id(), 1, "lo")] {
                assert_eq!(a.update(event), vec![]);
            }
            assert_eq!(last(&a, chat_id()), ("Hello".into(), MsgStatus::Complete));
            assert_eq!(chat_of(&a, chat_id()).phase(), Phase::Streaming);
            assert!(a.take_dirty(), "the chat is redrawn");
            assert_eq!(a.update(finish(chat_id(), 1)), vec![]);
            assert_eq!(chat_of(&a, chat_id()).phase(), Phase::Idle);
            assert_eq!(last(&a, chat_id()), ("Hello".into(), MsgStatus::Complete));
        }

        fn thinking(id: PaneId, turn: u64, text: &str) -> AppEvent {
            AppEvent::Chat(id, turn, StreamEvent::ThinkingDelta(text.into()))
        }

        #[test]
        fn a_thinking_delta_redraws_the_chat_and_emits_nothing() {
            let mut a = streaming();
            a.take_dirty();
            assert_eq!(a.update(thinking(chat_id(), 1, "Let me")), vec![]);
            assert!(a.take_dirty(), "the reasoning block is redrawn");
            // A stale turn changes and draws nothing.
            assert_eq!(a.update(thinking(chat_id(), 9, "stale")), vec![]);
            assert!(!a.take_dirty());
        }

        #[test]
        fn reasoning_never_reaches_an_effect_or_the_next_request() {
            let mut a = streaming();
            let mut effects = Vec::new();
            effects.extend(a.update(thinking(chat_id(), 1, "secret plan")));
            effects.extend(a.update(delta(chat_id(), 1, "answer")));
            effects.extend(a.update(finish(chat_id(), 1)));
            let next = a.chat_send(chat_id(), "again");
            effects.extend(next.clone());
            assert!(matches!(next.as_slice(), [Effect::ChatSend { .. }]));
            assert!(
                !format!("{effects:?}").contains("secret plan"),
                "no effect carries the reasoning: {effects:?}"
            );
            let Some(Effect::ChatSend { request, .. }) = next.first() else {
                panic!("one ChatSend");
            };
            let sent: Vec<&str> = request
                .messages
                .iter()
                .map(|m| m.content.as_str())
                .collect();
            assert_eq!(sent, ["hi", "answer", "again"]);
        }

        #[test]
        fn an_error_is_terminal_and_keeps_the_partial_answer() {
            let mut a = streaming();
            a.update(delta(chat_id(), 1, "Hel"));
            let error = AppEvent::Chat(
                chat_id(),
                1,
                StreamEvent::Error(ChatError::Failed("boom".into())),
            );
            assert_eq!(a.update(error), vec![]);
            assert_eq!(chat_of(&a, chat_id()).phase(), Phase::Idle);
            assert_eq!(last(&a, chat_id()), ("Hel".into(), MsgStatus::Error));
            assert_eq!(
                chat_of(&a, chat_id()).entries().last(),
                Some(&Entry::System("boom".into()))
            );
            // Nothing after the end of the turn changes the chat.
            a.update(delta(chat_id(), 1, "late"));
            assert_eq!(last(&a, chat_id()), ("Hel".into(), MsgStatus::Error));
        }

        #[test]
        fn cancel_keeps_the_partial_answer_and_drops_the_events_that_follow() {
            let mut a = streaming();
            a.update(delta(chat_id(), 1, "Hel"));
            assert_eq!(
                a.chat_cancel(chat_id()),
                vec![Effect::ChatCancel(chat_id())]
            );
            a.take_dirty();
            for event in [delta(chat_id(), 1, "lo"), finish(chat_id(), 1)] {
                assert_eq!(a.update(event), vec![]);
            }
            assert_eq!(last(&a, chat_id()), ("Hel".into(), MsgStatus::Cancelled));
            assert!(!a.take_dirty(), "a dropped event redraws nothing");
        }

        #[test]
        fn two_chats_stream_independently() {
            let mut a = launched(80, 25);
            a.open_chat(None);
            a.open_chat(None);
            let (one, two) = (id2(), id3());
            assert_eq!(turn_of(&a.chat_send(one, "q1")), 1);
            assert_eq!(turn_of(&a.chat_send(two, "q2")), 1);
            a.update(delta(one, 1, "A"));
            a.update(delta(two, 1, "x"));
            a.update(delta(one, 1, "B"));
            a.update(finish(two, 1));
            assert_eq!(last(&a, one), ("AB".into(), MsgStatus::Complete));
            assert_eq!(chat_of(&a, one).phase(), Phase::Streaming);
            assert_eq!(last(&a, two), ("x".into(), MsgStatus::Complete));
            assert_eq!(chat_of(&a, two).phase(), Phase::Idle);
            a.update(finish(one, 1));
            assert_eq!(chat_of(&a, one).phase(), Phase::Idle);
        }

        #[test]
        fn a_stream_continues_in_a_background_tab() {
            let mut a = launched(80, 25);
            a.open_chat_tab(None);
            let chat = a.focused();
            a.chat_send(chat, "hi");
            press(&mut a, "b1");
            assert_eq!(a.focused(), PaneId::FIRST);
            a.update(delta(chat, 1, "Hel"));
            a.update(delta(chat, 1, "lo"));
            a.update(finish(chat, 1));
            press(&mut a, "b2");
            assert_eq!(a.focused(), chat);
            assert_eq!(last(&a, chat), ("Hello".into(), MsgStatus::Complete));
            assert_eq!(chat_of(&a, chat).phase(), Phase::Idle);
        }

        #[test]
        fn a_stream_continues_during_copy_and_leaves_the_mode_alone() {
            let mut a = streaming();
            press(&mut a, "h");
            assert_eq!(a.focused(), PaneId::FIRST);
            press(&mut a, "[");
            assert!(matches!(a.input(), InputMode::Copy(_)));
            let mode = a.input();
            a.update(delta(chat_id(), 1, "Hel"));
            a.update(finish(chat_id(), 1));
            assert_eq!(a.input(), mode);
            assert_eq!(a.focused(), PaneId::FIRST);
            assert_eq!(last(&a, chat_id()), ("Hel".into(), MsgStatus::Complete));
        }

        #[test]
        fn a_stream_continues_across_zoom_and_focus_changes() {
            let mut a = streaming();
            press(&mut a, "h");
            press(&mut a, "wz");
            a.update(delta(chat_id(), 1, "Hel"));
            press(&mut a, "wz");
            press(&mut a, "l");
            a.update(delta(chat_id(), 1, "lo"));
            press(&mut a, "h");
            a.update(finish(chat_id(), 1));
            assert_eq!(last(&a, chat_id()), ("Hello".into(), MsgStatus::Complete));
            assert_eq!(chat_of(&a, chat_id()).phase(), Phase::Idle);
        }

        #[test]
        fn events_for_a_shell_or_an_unknown_pane_are_dropped() {
            let mut a = streaming();
            a.take_dirty();
            for id in [PaneId::FIRST, PaneId::for_test(99)] {
                assert_eq!(a.update(delta(id, 1, "x")), vec![]);
            }
            assert!(!a.take_dirty());
            assert_eq!(a.notice(), None);
        }

        #[test]
        fn an_event_of_another_turn_is_dropped() {
            let mut a = streaming();
            a.take_dirty();
            assert_eq!(a.update(delta(chat_id(), 7, "x")), vec![]);
            assert_eq!(
                chat_of(&a, chat_id()).phase(),
                Phase::Waiting { slow: false }
            );
            assert!(!a.take_dirty());
        }

        // The runtime stops at 1 MiB first; if the state still hits the cap it
        // asks for exactly one cancel.
        #[test]
        fn a_state_that_hits_the_cap_asks_for_exactly_one_cancel() {
            let mut a = streaming();
            let big = "a".repeat(ANSWER_CAP + 1);
            assert_eq!(
                a.update(delta(chat_id(), 1, &big)),
                vec![Effect::ChatCancel(chat_id())]
            );
            assert_eq!(a.update(delta(chat_id(), 1, "more")), vec![]);
            assert_eq!(last(&a, chat_id()).1, MsgStatus::Truncated);
        }

        // The cap reported by the runtime (Error(Truncated)) needs no cancel.
        #[test]
        fn a_truncated_error_from_the_runtime_asks_for_no_cancel() {
            let mut a = streaming();
            a.update(delta(chat_id(), 1, &"a".repeat(ANSWER_CAP)));
            let truncated = AppEvent::Chat(chat_id(), 1, StreamEvent::Error(ChatError::Truncated));
            assert_eq!(a.update(truncated), vec![]);
            assert_eq!(
                last(&a, chat_id()),
                ("a".repeat(ANSWER_CAP), MsgStatus::Truncated)
            );
        }

        // Task 5.2: closing.
        #[test]
        fn closing_a_streaming_chat_cancels_it_and_drops_its_late_events() {
            let mut a = streaming();
            a.update(delta(chat_id(), 1, "Hel"));
            press(&mut a, "wq");
            let effects = a.update(key('y'));
            assert!(
                effects.contains(&Effect::ChatCancel(chat_id())),
                "{effects:?}"
            );
            assert!(
                !effects.iter().any(|e| matches!(e, Effect::ClosePane(_))),
                "{effects:?}"
            );
            assert!(a.pane_view(chat_id()).is_none(), "the pane is gone");
            a.take_dirty();
            for event in [
                delta(chat_id(), 1, "lo"),
                finish(chat_id(), 1),
                AppEvent::Chat(chat_id(), 1, StreamEvent::Error(ChatError::Interrupted)),
            ] {
                assert_eq!(a.update(event), vec![]);
            }
            assert!(!a.take_dirty());
            assert_eq!(a.focused(), PaneId::FIRST);
        }

        #[test]
        fn closing_a_tab_cancels_every_streaming_chat_in_it() {
            let mut a = launched(80, 25);
            a.open_chat_tab(None);
            let one = a.focused();
            // A split from a chat is a shell; the second chat opens next to it.
            press(&mut a, "wv");
            let shell = a.focused();
            a.open_chat(None);
            let two = a.focused();
            for id in [one, two] {
                assert_eq!(a.chat_send(id, "hi").len(), 1);
            }
            press(&mut a, "tc");
            let effects = a.update(key('y'));
            let chats: Vec<_> = effects
                .iter()
                .filter_map(|e| match e {
                    Effect::ChatCancel(id) => Some(*id),
                    _ => None,
                })
                .collect();
            assert_eq!(chats.len(), 2, "{effects:?}");
            assert!(chats.contains(&one) && chats.contains(&two), "{effects:?}");
            let closed: Vec<_> = effects
                .iter()
                .filter_map(|e| match e {
                    Effect::ClosePane(id) => Some(*id),
                    _ => None,
                })
                .collect();
            assert_eq!(closed, vec![shell], "{effects:?}");
            assert_eq!(a.tab_count(), 1);
            a.take_dirty();
            for id in [one, two] {
                assert_eq!(a.update(delta(id, 1, "late")), vec![], "{id:?}");
            }
            assert!(!a.take_dirty());
        }

        #[test]
        fn quitting_asks_the_runtime_to_stop_and_emits_no_per_chat_effect() {
            let mut a = streaming();
            press(&mut a, "q");
            assert_eq!(a.update(key('y')), vec![Effect::Quit]);
            // The runtime stops every turn on Quit; the state stays readable.
            assert_eq!(
                chat_of(&a, chat_id()).phase(),
                Phase::Waiting { slow: false }
            );
        }

        // Task 4.4 enforcement: a chat never schedules or blocks a detection.
        #[test]
        fn chat_traffic_neither_starts_nor_clears_a_detection() {
            let mut a = streaming();
            assert_eq!(a.request_detection(), vec![Effect::DetectEngines]);
            a.update(delta(chat_id(), 1, "x"));
            a.update(finish(chat_id(), 1));
            assert_eq!(a.request_detection(), vec![], "still in flight");
            a.update(AppEvent::EnginesDetected(Detection::default()));
            assert_eq!(a.request_detection(), vec![Effect::DetectEngines]);
            let send = a.chat_send(chat_id(), "again");
            assert!(
                matches!(send.as_slice(), [Effect::ChatSend { .. }]),
                "{send:?}"
            );
        }
    }
}
