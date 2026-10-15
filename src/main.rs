use std::ffi::{OsStr, OsString};

use lazarobox_shell::core::ai::jail::SUBCOMMAND;
use lazarobox_shell::core::nesting;

/// A sync `main`: the jail helper (`__jail-exec`) must run before any thread
/// exists, so the tokio runtime is built only after that dispatch.
fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(OsStr::new(SUBCOMMAND)) {
        let rest: Vec<OsString> = args.collect();
        std::process::exit(lazarobox_shell::runtime::jail::exec_main(&rest));
    }
    // Before the runtime enters raw mode or the alternate screen.
    let marker = std::env::var_os(nesting::ENV_VAR).map(|v| v.to_string_lossy().into_owned());
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let exempt = !nesting::guarded_start(&argv);
    if let Err(message) = nesting::nesting_check(marker.as_deref(), exempt) {
        eprintln!("{message}");
        std::process::exit(1);
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(lazarobox_shell::runtime::run())
}
