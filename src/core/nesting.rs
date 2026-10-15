//! Nesting guard, modelled on tmux's `$TMUX`: every pane child gets
//! `LAZAROBOX=<pid of the owning instance>`, and a new instance refuses to
//! start when it sees that variable unless `--force` is given.

use std::ffi::OsString;

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

/// Whether `--force` appears among the arguments (program name excluded).
pub fn has_force(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter().any(|arg| arg == FORCE_FLAG)
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

    #[test]
    fn force_flag_is_found_among_the_arguments() {
        assert!(has_force(["--force"].map(OsString::from)));
        assert!(has_force(["x", "--force"].map(OsString::from)));
        assert!(!has_force(["--forced"].map(OsString::from)));
        assert!(!has_force(Vec::<OsString>::new()));
    }
}
