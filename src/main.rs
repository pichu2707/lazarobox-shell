use std::ffi::{OsStr, OsString};
use std::io::Write;

use lazarobox_shell::core::ai::jail::SUBCOMMAND;
use lazarobox_shell::core::{cli, nesting};

/// A sync `main`: the jail helper (`__jail-exec`) must run before any thread
/// exists, so the tokio runtime is built only after that dispatch.
fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_deref() == Some(OsStr::new(SUBCOMMAND)) {
        let rest: Vec<OsString> = args.collect();
        std::process::exit(lazarobox_shell::runtime::jail::exec_main(&rest));
    }
    // The MCP shim of a chat engine: a plain relay, before the nesting guard
    // (it runs inside a lazarobox pane) and before anything touches the TTY.
    if first.as_deref() == Some(OsStr::new(lazarobox_shell::runtime::mcp::shim::SUBCOMMAND)) {
        let rest: Vec<OsString> = args.collect();
        std::process::exit(lazarobox_shell::runtime::mcp::shim::main(&rest));
    }
    // The watcher lazarobox starts for itself (see `runtime::watch`).
    if first.as_deref() == Some(OsStr::new(lazarobox_shell::runtime::watch::SUBCOMMAND)) {
        let rest: Vec<OsString> = args.collect();
        std::process::exit(lazarobox_shell::runtime::watch::main(&rest));
    }
    if first.as_deref() == Some(OsStr::new(lazarobox_shell::core::desktop::SUBCOMMAND)) {
        let rest: Vec<OsString> = args.collect();
        std::process::exit(lazarobox_shell::runtime::desktop::main(&rest));
    }
    // Before the runtime enters raw mode or the alternate screen.
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let action = cli::classify(&argv);
    match action {
        cli::Action::Version => {
            return out(&format!("lazarobox-shell {}", env!("CARGO_PKG_VERSION")));
        }
        cli::Action::Help => {
            return out(cli::USAGE);
        }
        _ => {}
    }
    if let Some(message) = cli::rejection(&action) {
        eprintln!("{message}");
        std::process::exit(2);
    }
    let marker = std::env::var_os(nesting::ENV_VAR).map(|v| v.to_string_lossy().into_owned());
    let exempt = !nesting::guarded_start(&argv);
    if let Err(message) = nesting::nesting_check(marker.as_deref(), exempt) {
        eprintln!("{message}");
        std::process::exit(1);
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    // The quit work is done inside `run`; the guard keeps a wedged blocking
    // thread (a hung PATH lookup) from holding the exit, panics included.
    let guard =
        lazarobox_shell::runtime::QuitGuard::new(rt, lazarobox_shell::runtime::RUNTIME_QUIT_BOUND);
    guard.block_on(lazarobox_shell::runtime::run(
        lazarobox_shell::core::start::parse_args(&argv),
    ))
}

/// Prints a line to stdout; a closed pipe (`--help | head -0`) is not an error.
fn out(text: &str) -> std::io::Result<()> {
    match writeln!(std::io::stdout().lock(), "{text}") {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}
