//! Classifies the command line of the plain start (program name excluded),
//! after the subcommands have been dispatched by `main`.

/// What the leading arguments ask for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// Start the TUI.
    Run,
    Version,
    Help,
    /// An unknown flag, verbatim.
    UnknownOption(String),
    /// An unknown first non-flag argument, verbatim.
    UnknownCommand(String),
}

/// The text of `--help`.
pub const USAGE: &str = "\
usage: lazarobox-shell [--force] [--dir <folder>]
       lazarobox-shell install-desktop [--terminal <cmd>] [--dir <folder>] [--uninstall]
       lazarobox-shell --version | --help

  --force        start even inside a lazarobox pane
  --dir <folder> start in this folder
  -V, --version  print the version
  -h, --help     print this help";

/// The one-line stderr message of a rejected command line.
pub fn rejection(action: &Action) -> Option<String> {
    match action {
        Action::UnknownOption(x) => Some(format!(
            "lazarobox-shell: unknown option '{x}' (try --help)"
        )),
        Action::UnknownCommand(x) => Some(format!(
            "lazarobox-shell: unknown command '{x}' (try --help)"
        )),
        _ => None,
    }
}

/// The first decisive argument wins; `--dir` swallows its value.
pub fn classify(argv: &[String]) -> Action {
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--version" | "-V" => return Action::Version,
            "--help" | "-h" => return Action::Help,
            "--force" => {}
            "--dir" => {
                it.next();
            }
            a if a.starts_with("--dir=") => {}
            a if a.starts_with('-') => return Action::UnknownOption(a.to_owned()),
            a => return Action::UnknownCommand(a.to_owned()),
        }
    }
    Action::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(list: &[&str]) -> Action {
        classify(&list.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn known_flags_run_the_tui() {
        assert_eq!(c(&[]), Action::Run);
        assert_eq!(c(&["--force"]), Action::Run);
        assert_eq!(c(&["--dir", "/a", "--force"]), Action::Run);
        assert_eq!(c(&["--dir=/a"]), Action::Run);
    }

    #[test]
    fn version_and_help_in_both_spellings() {
        assert_eq!(c(&["--version"]), Action::Version);
        assert_eq!(c(&["-V"]), Action::Version);
        assert_eq!(c(&["--help"]), Action::Help);
        assert_eq!(c(&["-h"]), Action::Help);
        assert_eq!(c(&["--force", "--help"]), Action::Help);
    }

    #[test]
    fn the_dir_value_is_not_classified() {
        assert_eq!(c(&["--dir", "--version"]), Action::Run);
        assert_eq!(c(&["--dir", "x", "-h"]), Action::Help);
    }

    #[test]
    fn unknown_input_is_rejected_with_one_line() {
        let opt = c(&["--forse"]);
        assert_eq!(opt, Action::UnknownOption("--forse".into()));
        assert_eq!(
            rejection(&opt).unwrap(),
            "lazarobox-shell: unknown option '--forse' (try --help)"
        );
        let cmd = c(&["bogus"]);
        assert_eq!(cmd, Action::UnknownCommand("bogus".into()));
        assert!(rejection(&cmd).unwrap().contains("unknown command 'bogus'"));
        assert_eq!(rejection(&Action::Run), None);
    }
}
