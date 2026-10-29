//! Nesting guard, modelled on tmux's `$TMUX`: every pane child gets
//! `LAZAROBOX=<pid of the owning instance>`, and a new instance refuses to
//! start when it sees that variable unless `--force` is given.

/// Set in every shell pane's environment; its value is the pid of the
/// instance that spawned it (useful to find the owner, like `$TMUX`'s pid).
pub const ENV_VAR: &str = "LAZAROBOX";

/// The flag that skips the guard.
pub const FORCE_FLAG: &str = "--force";

/// Decides whether this instance may start. `env_value` is `$LAZAROBOX`; an
/// empty value counts as unset. The error is a one-line message for stderr.
pub fn nesting_check(env_value: Option<&str>, force: bool) -> Result<(), String> {
    match env_value {
        Some(value) if !value.is_empty() && !force => Err(format!(
            "lazarobox-shell is already running in this terminal; use {FORCE_FLAG} to nest anyway"
        )),
        _ => Ok(()),
    }
}

/// Whether the nesting check applies to this start (program name excluded).
/// It does only for the plain TUI start: the leading arguments are all flags
/// and none is `--force`. Any subcommand (first non-flag argument) is exempt,
/// and `--force` counts only as a top-level flag before it. The value after
/// `--dir` is not a subcommand.
pub fn guarded_start(args: &[String]) -> bool {
    let mut it = args.iter().peekable();
    while let Some(arg) = it.next() {
        if arg == FORCE_FLAG || !arg.starts_with('-') {
            return false;
        }
        if arg == crate::core::start::DIR_FLAG {
            it.next_if(|v| !v.starts_with('-'));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_start_when_the_variable_is_absent() {
        assert_eq!(nesting_check(None, false), Ok(()));
    }

    #[test]
    fn allows_start_when_the_variable_is_empty() {
        assert_eq!(nesting_check(Some(""), false), Ok(()));
    }

    #[test]
    fn refuses_when_the_variable_is_set() {
        let msg = nesting_check(Some("1234"), false).unwrap_err();
        assert!(msg.contains("already running"));
        assert!(msg.contains("--force"));
        assert!(!msg.contains('\n'));
    }

    #[test]
    fn force_skips_the_guard() {
        assert_eq!(nesting_check(Some("1234"), true), Ok(()));
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn plain_start_is_guarded() {
        assert!(guarded_start(&args(&[])));
        assert!(guarded_start(&args(&["--version"])));
        assert!(guarded_start(&args(&["--forced"])));
    }

    #[test]
    fn force_alone_skips_the_guard() {
        assert!(!guarded_start(&args(&["--force"])));
        assert!(!guarded_start(&args(&["--version", "--force"])));
    }

    #[test]
    fn subcommands_are_exempt() {
        assert!(!guarded_start(&args(&["install-desktop"])));
        assert!(!guarded_start(&args(&["__jail-exec"])));
    }

    #[test]
    fn force_after_a_subcommand_is_not_an_override() {
        assert!(!guarded_start(&args(&["install-desktop", "--force"])));
        assert!(!guarded_start(&args(&["--version", "x"])));
    }

    #[test]
    fn the_value_of_dir_does_not_exempt_the_start() {
        assert!(guarded_start(&args(&["--dir", "/a"])));
        assert!(guarded_start(&args(&["--dir=/a"])));
        assert!(guarded_start(&args(&["--dir", "/a", "--version"])));
        assert!(!guarded_start(&args(&["--dir", "/a", "--force"])));
        assert!(!guarded_start(&args(&["--dir", "/a", "install-desktop"])));
    }
}
