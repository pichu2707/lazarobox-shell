//! Throwaway spike for the `terminal-pane` change (tasks 1.2 / 1.3).
//!
//! Runs a program (default `$SHELL`) inside a PTY, feeds its output into a
//! `vt100` parser, answers terminal queries (DA1, DSR 5n/6n), applies DECSCUSR
//! cursor shapes and renders the screen into ratatui with a hand-written cell
//! loop. It exists so a human can judge nvim / LazyVim / htop fidelity.
//!
//! ```text
//! cargo run --example spike_vt100            # $SHELL
//! cargo run --example spike_vt100 -- nvim    # or any program (+ args)
//! ```
//!
//! Quit with Ctrl+Q (spike only). The bottom row is a status line showing the
//! last raw `KeyEvent` (verify Ctrl+Space = `Char(' ')` + CONTROL) and the last
//! DECSCUSR value the child requested.
//!
//! The vt100 API findings are encoded as tests in this file.

use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{execute, terminal as ct_terminal};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};
use vt100::{Callbacks, Parser, Screen};

const SCROLLBACK: usize = 1_000;
const TICK: Duration = Duration::from_millis(16);
const DRAIN_BUDGET: usize = 256;
const LOG_CAP: usize = 64;

// ---------------------------------------------------------------------------
// Responder: vt100 callbacks answering terminal queries and capturing DECSCUSR
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct CsiEvent {
    i1: Option<u8>,
    i2: Option<u8>,
    params: Vec<Vec<u16>>,
    c: char,
}

#[derive(Default)]
struct Responder {
    /// Bytes to write back to the child (query replies).
    replies: Vec<u8>,
    /// Last DECSCUSR Ps requested by the child.
    decscusr: Option<u16>,
    /// Every CSI that reached `unhandled_csi` (capped; for the tests/status).
    csi_log: Vec<CsiEvent>,
    /// Every OSC that reached `unhandled_osc` (capped).
    osc_log: Vec<Vec<Vec<u8>>>,
}

impl Responder {
    fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }
}

fn first_param(params: &[&[u16]]) -> u16 {
    params.first().and_then(|p| p.first()).copied().unwrap_or(0)
}

fn push_capped<T>(log: &mut Vec<T>, item: T) {
    if log.len() >= LOG_CAP {
        log.remove(0);
    }
    log.push(item);
}

impl Callbacks for Responder {
    fn unhandled_csi(
        &mut self,
        screen: &mut Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        push_capped(
            &mut self.csi_log,
            CsiEvent {
                i1,
                i2,
                params: params.iter().map(|p| p.to_vec()).collect(),
                c,
            },
        );
        match (i1, i2, c) {
            // DA1: only the plain request (Ps 0 / absent). `\e[>c` (DA2) has a
            // `>` intermediate and is intentionally not answered.
            (None, None, 'c') if first_param(params) == 0 => {
                self.replies.extend_from_slice(b"\x1b[?62;c");
            }
            (None, None, 'n') => match first_param(params) {
                5 => self.replies.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let (row, col) = screen.cursor_position();
                    let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                    self.replies.extend_from_slice(reply.as_bytes());
                }
                _ => {}
            },
            // DECSCUSR: CSI Ps SP q
            (Some(b' '), None, 'q') => self.decscusr = Some(first_param(params)),
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _screen: &mut Screen, params: &[&[u8]]) {
        push_capped(
            &mut self.osc_log,
            params.iter().map(|p| p.to_vec()).collect(),
        );
    }
}

/// DECSCUSR Ps -> crossterm cursor style. `None` for unknown Ps (ignored).
fn cursor_style(ps: u16) -> Option<SetCursorStyle> {
    Some(match ps {
        0 => SetCursorStyle::DefaultUserShape,
        1 => SetCursorStyle::BlinkingBlock,
        2 => SetCursorStyle::SteadyBlock,
        3 => SetCursorStyle::BlinkingUnderScore,
        4 => SetCursorStyle::SteadyUnderScore,
        5 => SetCursorStyle::BlinkingBar,
        6 => SetCursorStyle::SteadyBar,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Minimal key encoder
// ---------------------------------------------------------------------------

fn encode_key(key: KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let arrow = |letter: u8| {
        let intro: &[u8] = if application_cursor {
            b"\x1bO"
        } else {
            b"\x1b["
        };
        let mut out = intro.to_vec();
        out.push(letter);
        out
    };
    let bytes = match key.code {
        KeyCode::Char(c) if ctrl => match c {
            ' ' | '@' | '2' => vec![0x00],
            'a'..='z' => vec![(c as u8) - b'a' + 1],
            'A'..='Z' => vec![(c as u8) - b'A' + 1],
            '['..='_' => vec![(c as u8) - b'@'],
            _ => return None,
        },
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Null => vec![0x00],
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Up => arrow(b'A'),
        KeyCode::Down => arrow(b'B'),
        KeyCode::Right => arrow(b'C'),
        KeyCode::Left => arrow(b'D'),
        KeyCode::Home => arrow(b'H'),
        KeyCode::End => arrow(b'F'),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        _ => return None,
    };
    if alt {
        let mut out = vec![0x1b];
        out.extend(bytes);
        Some(out)
    } else {
        Some(bytes)
    }
}

// ---------------------------------------------------------------------------
// PTY session (detached reader thread, kill on drop)
// ---------------------------------------------------------------------------

enum Msg {
    Out(Vec<u8>),
    Exited,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Msg>,
}

impl Session {
    fn spawn(program: &str, args: &[String], rows: u16, cols: u16) -> io::Result<Self> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd).map_err(io::Error::other)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Msg::Exited);
                        break;
                    }
                    Ok(n) => {
                        if tx.send(Msg::Out(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            master: pair.master,
            child,
            writer,
            rx,
        })
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // std's unix `Child::kill` is SIGKILL, so `wait` cannot hang. Dropping
        // the master afterwards hangs up any grandchildren (nvim under a shell).
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Terminal lifecycle
// ---------------------------------------------------------------------------

fn reset_cursor_style() {
    let _ = execute!(io::stdout(), SetCursorStyle::DefaultUserShape);
}

/// Restores raw mode / alt screen / cursor shape on drop (incl. unwinding).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        reset_cursor_style();
        ratatui::restore();
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn map_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::new()
        .fg(map_color(cell.fgcolor()))
        .bg(map_color(cell.bgcolor()));
    for (on, modifier) in [
        (cell.bold(), Modifier::BOLD),
        (cell.dim(), Modifier::DIM),
        (cell.italic(), Modifier::ITALIC),
        (cell.underline(), Modifier::UNDERLINED),
        (cell.inverse(), Modifier::REVERSED),
    ] {
        if on {
            style = style.add_modifier(modifier);
        }
    }
    style
}

fn draw(frame: &mut Frame, parser: &Parser<Responder>, last_key: &str) {
    let area = frame.area();
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let max_rows = rows.min(area.height.saturating_sub(1));
    let max_cols = cols.min(area.width);
    let buf = frame.buffer_mut();
    for row in 0..max_rows {
        for col in 0..max_cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let out = &mut buf[(col, row)];
            let text = cell.contents();
            out.set_symbol(if text.is_empty() { " " } else { text });
            out.set_style(cell_style(cell));
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (row, col) = screen.cursor_position();
        if row < max_rows && col < max_cols {
            frame.set_cursor_position((col, row));
        }
    }
    let status = format!(
        " Ctrl+Q quit | key: {last_key} | DECSCUSR: {:?} | alt: {} | {cols}x{rows}",
        parser.callbacks().decscusr,
        screen.alternate_screen(),
    );
    let bar = Rect::new(0, area.height.saturating_sub(1), area.width, 1);
    frame.render_widget(
        Paragraph::new(status).style(Style::new().fg(Color::Black).bg(Color::Yellow)),
        bar,
    );
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn pane_size(rows: u16, cols: u16) -> (u16, u16) {
    (rows.saturating_sub(1).max(1), cols.max(1))
}

fn run(terminal: &mut DefaultTerminal, session: &mut Session) -> io::Result<()> {
    let (cols, rows) = ct_terminal::size()?;
    let (pane_rows, pane_cols) = pane_size(rows, cols);
    let mut parser =
        Parser::new_with_callbacks(pane_rows, pane_cols, SCROLLBACK, Responder::default());
    let mut last_key = String::from("(none yet)");
    let mut applied_ps: Option<u16> = None;
    let mut dirty = true;

    loop {
        for _ in 0..DRAIN_BUDGET {
            match session.rx.try_recv() {
                Ok(Msg::Out(bytes)) => {
                    parser.process(&bytes);
                    let replies = parser.callbacks_mut().take_replies();
                    if !replies.is_empty() {
                        session.write(&replies)?;
                    }
                    dirty = true;
                }
                Ok(Msg::Exited) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        let wanted = parser.callbacks().decscusr;
        if wanted != applied_ps {
            if let Some(style) = wanted.and_then(cursor_style) {
                execute!(io::stdout(), style)?;
            }
            applied_ps = wanted;
        }

        if dirty {
            terminal.draw(|frame| draw(frame, &parser, &last_key))?;
            dirty = false;
        }

        if event::poll(TICK)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    last_key = format!("{key:?}");
                    dirty = true;
                    let quit = key.code == KeyCode::Char('q')
                        && key.modifiers.contains(KeyModifiers::CONTROL);
                    if quit {
                        return Ok(());
                    }
                    if let Some(bytes) = encode_key(key, parser.screen().application_cursor()) {
                        session.write(&bytes)?;
                    }
                }
                Event::Resize(cols, rows) => {
                    let (pane_rows, pane_cols) = pane_size(rows, cols);
                    parser.screen_mut().set_size(pane_rows, pane_cols);
                    session.resize(pane_rows, pane_cols)?;
                    dirty = true;
                }
                _ => {}
            }
        }
    }
}

fn main() -> io::Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (program, args) = match argv.split_first() {
        Some((program, args)) => (program.clone(), args.to_vec()),
        None => (
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            Vec::new(),
        ),
    };

    let (cols, rows) = ct_terminal::size()?;
    let (pane_rows, pane_cols) = pane_size(rows, cols);
    // Declared first so it is dropped last: the terminal is restored before the
    // child is killed, on both normal exit and unwinding.
    let mut session = Session::spawn(&program, &args, pane_rows, pane_cols)?;

    let mut terminal = ratatui::init();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        reset_cursor_style();
        previous_hook(info);
    }));
    let _guard = TerminalGuard;

    run(&mut terminal, &mut session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt100::Parser;

    fn parser(rows: u16, cols: u16, scrollback: usize) -> Parser<Responder> {
        Parser::new_with_callbacks(rows, cols, scrollback, Responder::default())
    }

    /// Feed N numbered lines so that `n - rows + 1` rows scroll into history.
    fn feed_lines(p: &mut Parser<Responder>, n: usize) {
        for i in 0..n {
            p.process(format!("line{i}\r\n").as_bytes());
        }
    }

    #[test]
    fn da1_reaches_unhandled_csi_without_intermediates() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[c");
        assert_eq!(p.callbacks().csi_log.len(), 1);
        let ev = &p.callbacks().csi_log[0];
        assert_eq!((ev.i1, ev.i2, ev.c), (None, None, 'c'));
        assert_eq!(p.callbacks_mut().take_replies(), b"\x1b[?62;c");
    }

    #[test]
    fn dsr_5n_reports_ok() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[5n");
        assert_eq!(p.callbacks_mut().take_replies(), b"\x1b[0n");
    }

    #[test]
    fn dsr_6n_reports_one_based_cursor_position() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[6n");
        assert_eq!(p.callbacks_mut().take_replies(), b"\x1b[1;1R");
        p.process(b"\x1b[5;10H\x1b[6n");
        assert_eq!(p.callbacks_mut().take_replies(), b"\x1b[5;10R");
    }

    #[test]
    fn query_split_across_chunks_replies_once() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[");
        assert!(p.callbacks().replies.is_empty());
        p.process(b"6");
        p.process(b"n");
        assert_eq!(p.callbacks_mut().take_replies(), b"\x1b[1;1R");
    }

    #[test]
    fn decscusr_reaches_unhandled_csi_with_space_intermediate() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[6 q");
        let ev = &p.callbacks().csi_log[0];
        assert_eq!((ev.i1, ev.i2, ev.c), (Some(b' '), None, 'q'));
        assert_eq!(ev.params, vec![vec![6u16]]);
        assert_eq!(p.callbacks().decscusr, Some(6));
        assert!(p.callbacks().replies.is_empty(), "DECSCUSR never replies");
    }

    #[test]
    fn decscusr_variants_and_split_chunks() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[2 q");
        assert_eq!(p.callbacks().decscusr, Some(2));
        p.process(b"\x1b[");
        p.process(b"5 ");
        p.process(b"q");
        assert_eq!(p.callbacks().decscusr, Some(5));
        // Missing Ps means 0 (default shape): vte yields a single 0 param.
        p.process(b"\x1b[ q");
        assert_eq!(p.callbacks().decscusr, Some(0));
        assert_eq!(p.callbacks().csi_log.len(), 3);
    }

    #[test]
    fn other_probes_nvim_sends_reach_callbacks_with_prefix_intermediates() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[>c"); // DA2
        p.process(b"\x1b[>0q"); // XTVERSION
        p.process(b"\x1b[?u"); // kitty keyboard query
        p.process(b"\x1b[?6n"); // DECXCPR
        let log = &p.callbacks().csi_log;
        assert_eq!(log.len(), 4);
        assert_eq!((log[0].i1, log[0].c), (Some(b'>'), 'c'));
        assert_eq!((log[1].i1, log[1].c), (Some(b'>'), 'q'));
        assert_eq!((log[2].i1, log[2].c), (Some(b'?'), 'u'));
        assert_eq!((log[3].i1, log[3].c), (Some(b'?'), 'n'));
        // The spike responder deliberately ignores them: no replies.
        assert!(p.callbacks().replies.is_empty());
    }

    #[test]
    fn osc_color_query_reaches_unhandled_osc() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b]11;?\x07");
        assert_eq!(
            p.callbacks().osc_log,
            vec![vec![b"11".to_vec(), b"?".to_vec()]]
        );
    }

    #[test]
    fn handled_sequences_do_not_reach_unhandled_csi() {
        let mut p = parser(24, 80, 0);
        p.process(b"\x1b[1;31m\x1b[2J\x1b[H\x1b[?1h\x1b[?2004h\x1b[?25l");
        assert!(p.callbacks().csi_log.is_empty());
        assert!(p.screen().application_cursor());
        assert!(p.screen().bracketed_paste());
        assert!(p.screen().hide_cursor());
    }

    #[test]
    fn scrollback_len_via_set_max_then_read() {
        let mut p = parser(3, 10, 100);
        feed_lines(&mut p, 10); // 10 lines on 3 rows => 8 rows in history
        assert_eq!(p.screen().scrollback(), 0);
        p.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(p.screen().scrollback(), 8);
        p.screen_mut().set_scrollback(0);
        assert_eq!(p.screen().scrollback(), 0);
    }

    #[test]
    fn set_scrollback_clamps_and_max_len_caps_history() {
        let mut p = parser(3, 10, 5);
        feed_lines(&mut p, 30);
        p.screen_mut().set_scrollback(3);
        assert_eq!(p.screen().scrollback(), 3);
        p.screen_mut().set_scrollback(1_000);
        assert_eq!(p.screen().scrollback(), 5, "clamped to configured max");
    }

    #[test]
    fn offset_larger_than_rows_does_not_panic_and_shows_history() {
        let mut p = parser(3, 10, 100);
        feed_lines(&mut p, 20);
        p.screen_mut().set_scrollback(9); // > rows (3)
        assert_eq!(p.screen().scrollback(), 9);
        let rows: Vec<String> = p.screen().rows(0, 10).collect();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].starts_with("line"), "got {rows:?}");
        assert!(p.screen().cell(0, 0).is_some());
        assert!(p.screen().cell(2, 0).is_some());
        // And a huge offset also reads safely.
        p.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(p.screen().rows(0, 10).count(), 3);
    }

    #[test]
    fn new_output_while_scrolled_back_anchors_the_view() {
        let mut p = parser(3, 10, 100);
        feed_lines(&mut p, 10);
        p.screen_mut().set_scrollback(2);
        let before: Vec<String> = p.screen().rows(0, 10).collect();
        feed_lines(&mut p, 2); // two more rows scroll into history
        assert_eq!(p.screen().scrollback(), 4, "vt100 bumps the offset itself");
        let after: Vec<String> = p.screen().rows(0, 10).collect();
        assert_eq!(before, after, "viewport content is anchored by vt100");
    }

    #[test]
    fn alternate_screen_has_no_scrollback() {
        let mut p = parser(3, 10, 100);
        feed_lines(&mut p, 10);
        assert!(!p.screen().alternate_screen());
        p.process(b"\x1b[?1049h");
        assert!(p.screen().alternate_screen());
        p.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(p.screen().scrollback(), 0);
        p.process(b"\x1b[?1049l");
        assert!(!p.screen().alternate_screen());
        p.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(p.screen().scrollback(), 8, "main-screen history survives");
    }

    #[test]
    fn resize_while_scrolled_back_does_not_panic() {
        let mut p = parser(5, 20, 100);
        feed_lines(&mut p, 30);
        p.screen_mut().set_scrollback(10);
        p.screen_mut().set_size(2, 8);
        assert_eq!(p.screen().size(), (2, 8));
        p.screen_mut().set_size(1, 1);
        assert_eq!(p.screen().rows(0, 1).count(), 1);
    }

    mod keys {
        use super::super::*;
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        fn k(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
            KeyEvent::new(code, mods)
        }

        #[test]
        fn printable_enter_backspace_esc_tab() {
            let n = KeyModifiers::NONE;
            assert_eq!(
                encode_key(k(KeyCode::Char('a'), n), false),
                Some(b"a".to_vec())
            );
            assert_eq!(
                encode_key(k(KeyCode::Char('é'), n), false),
                Some("é".as_bytes().to_vec())
            );
            assert_eq!(
                encode_key(k(KeyCode::Enter, n), false),
                Some(b"\r".to_vec())
            );
            assert_eq!(
                encode_key(k(KeyCode::Backspace, n), false),
                Some(vec![0x7f])
            );
            assert_eq!(encode_key(k(KeyCode::Esc, n), false), Some(vec![0x1b]));
            assert_eq!(encode_key(k(KeyCode::Tab, n), false), Some(b"\t".to_vec()));
        }

        #[test]
        fn ctrl_letters_and_ctrl_space() {
            let c = KeyModifiers::CONTROL;
            assert_eq!(encode_key(k(KeyCode::Char('c'), c), false), Some(vec![3]));
            assert_eq!(encode_key(k(KeyCode::Char('a'), c), false), Some(vec![1]));
            assert_eq!(encode_key(k(KeyCode::Char(' '), c), false), Some(vec![0]));
        }

        #[test]
        fn alt_prefixes_escape() {
            let a = KeyModifiers::ALT;
            assert_eq!(
                encode_key(k(KeyCode::Char('x'), a), false),
                Some(b"\x1bx".to_vec())
            );
        }

        #[test]
        fn arrows_honor_application_cursor() {
            let n = KeyModifiers::NONE;
            assert_eq!(
                encode_key(k(KeyCode::Up, n), false),
                Some(b"\x1b[A".to_vec())
            );
            assert_eq!(
                encode_key(k(KeyCode::Up, n), true),
                Some(b"\x1bOA".to_vec())
            );
            assert_eq!(
                encode_key(k(KeyCode::Left, n), false),
                Some(b"\x1b[D".to_vec())
            );
            assert_eq!(
                encode_key(k(KeyCode::Left, n), true),
                Some(b"\x1bOD".to_vec())
            );
        }

        #[test]
        fn unmapped_key_is_none() {
            assert_eq!(
                encode_key(k(KeyCode::CapsLock, KeyModifiers::NONE), false),
                None
            );
        }
    }

    mod cursor {
        use super::super::*;
        use ratatui::crossterm::cursor::SetCursorStyle;

        #[test]
        fn decscusr_ps_maps_to_crossterm_styles() {
            assert_eq!(cursor_style(0), Some(SetCursorStyle::DefaultUserShape));
            assert_eq!(cursor_style(1), Some(SetCursorStyle::BlinkingBlock));
            assert_eq!(cursor_style(2), Some(SetCursorStyle::SteadyBlock));
            assert_eq!(cursor_style(3), Some(SetCursorStyle::BlinkingUnderScore));
            assert_eq!(cursor_style(4), Some(SetCursorStyle::SteadyUnderScore));
            assert_eq!(cursor_style(5), Some(SetCursorStyle::BlinkingBar));
            assert_eq!(cursor_style(6), Some(SetCursorStyle::SteadyBar));
            assert_eq!(cursor_style(7), None);
        }
    }
}
