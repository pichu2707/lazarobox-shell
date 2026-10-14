use std::ffi::{OsStr, OsString};

use lazarobox_shell::core::ai::jail::SUBCOMMAND;

/// A sync `main`: the jail helper (`__jail-exec`) must run before any thread
/// exists, so the tokio runtime is built only after that dispatch.
fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(OsStr::new(SUBCOMMAND)) {
        let rest: Vec<OsString> = args.collect();
        std::process::exit(lazarobox_shell::runtime::jail::exec_main(&rest));
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(lazarobox_shell::runtime::run())
}
