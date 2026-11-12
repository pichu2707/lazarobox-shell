use std::cell::RefCell;

use super::*;

fn plan() -> Plan {
    Plan {
        dir: "/h/.config/mcp-gsc".to_owned(),
        file: "/h/.config/lazarobox/google-oauth-client.json".to_owned(),
    }
}

/// A fake `uv`: what `tool list` and `tool dir` print, and the calls made.
struct Fake {
    list: Result<String, Fail>,
    dir: Result<String, Fail>,
    calls: RefCell<Vec<Vec<String>>>,
}

impl Fake {
    fn ok() -> Self {
        Self {
            list: Ok("mcp-search-console v0.4.2\n- mcp-gsc\n".to_owned()),
            dir: Ok("/h/.local/share/uv/tools\n".to_owned()),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, Fail> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|a| (*a).to_owned()).collect());
        match args {
            ["tool", "list"] => self.list.clone(),
            ["tool", "dir"] => self.dir.clone(),
            other => panic!("unexpected uv call {other:?}"),
        }
    }
}

fn checked(fake: &Fake, client: bool) -> Result<String, String> {
    check(&plan(), &|args: &[&str]| fake.run(args), |_| client)
}

// Spec mcp-connect, gsc link prompt shows the exact line.
#[test]
fn a_ready_machine_gives_the_exact_line() {
    let fake = Fake::ok();
    assert_eq!(
        checked(&fake, true).unwrap(),
        "env -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH GSC_CONFIG_DIR='/h/.config/mcp-gsc' GSC_OAUTH_CLIENT_SECRETS_FILE='/h/.config/lazarobox/google-oauth-client.json' '/h/.local/share/uv/tools/mcp-search-console/bin/python' -I -c 'import gsc_server as g; g.get_gsc_service_oauth(); print(\"Search Console linked\")'"
    );
    // Only direct argv of `uv tool list` and `uv tool dir`.
    assert_eq!(
        *fake.calls.borrow(),
        [vec!["tool", "list"], vec!["tool", "dir"]]
    );
}

// Spec: gsc without the client file.
#[test]
fn no_client_file_points_at_o_and_runs_nothing() {
    let fake = Fake::ok();
    assert_eq!(checked(&fake, false), Err(NO_CLIENT.to_owned()));
    assert_eq!(NO_CLIENT, "add your Desktop OAuth client first (o)");
    assert!(fake.calls.borrow().is_empty());
}

// Spec: gsc without uv or the pinned version.
#[test]
fn no_uv_another_version_or_none_are_notices_and_no_line() {
    let none = Fake {
        list: Err(Fail::NotFound),
        ..Fake::ok()
    };
    assert_eq!(
        checked(&none, true),
        Err("uv not found: install uv first".to_owned())
    );
    let other = Fake {
        list: Ok("mcp-search-console v0.4.3\n".to_owned()),
        ..Fake::ok()
    };
    assert_eq!(
        checked(&other, true),
        Err(
            "mcp-search-console 0.4.3; run: uv tool install --force mcp-search-console==0.4.2"
                .to_owned()
        )
    );
    let missing = Fake {
        list: Ok("ruff v0.1.0\n".to_owned()),
        ..Fake::ok()
    };
    assert_eq!(
        checked(&missing, true),
        Err("mcp-search-console not installed; run: uv tool install --force mcp-search-console==0.4.2"
            .to_owned())
    );
    // The tool dir is not asked when the version is wrong.
    assert_eq!(other.calls.borrow().len(), 1);
}

#[test]
fn a_tool_dir_that_is_not_one_absolute_folder_or_a_failing_uv_gives_a_notice() {
    for dir in ["", "relative\n", "/a\n/b\n"] {
        let fake = Fake {
            dir: Ok(dir.to_owned()),
            ..Fake::ok()
        };
        assert_eq!(
            checked(&fake, true),
            Err(gsc::NO_TOOL_DIR.to_owned()),
            "{dir:?}"
        );
    }
    let slow = Fake {
        dir: Err(Fail::Other("uv did not answer in 5 s")),
        ..Fake::ok()
    };
    assert_eq!(
        checked(&slow, true),
        Err("uv did not answer in 5 s".to_owned())
    );
    let list_fails = Fake {
        list: Err(Fail::Other("uv failed")),
        ..Fake::ok()
    };
    assert_eq!(checked(&list_fails, true), Err("uv failed".to_owned()));
}

#[test]
fn a_path_the_line_cannot_carry_is_a_notice() {
    let fake = Fake::ok();
    let bad = Plan {
        dir: "/h/a\nb".to_owned(),
        ..plan()
    };
    let run = |args: &[&str]| fake.run(args);
    assert_eq!(check(&bad, &run, |_| true), Err(gsc::BAD_PATH.to_owned()));
}

// The real runner: direct argv, a limit, and a child that is stopped.
#[test]
fn the_real_runner_gives_stdout_not_found_a_failure_and_a_timeout() {
    let sh = OsStr::new("/bin/sh");
    let out = run_with(sh, &["-c", "echo hi"], Duration::from_secs(5));
    assert_eq!(out, Ok("hi\n".to_owned()));
    let missing = run_with(
        OsStr::new("/nonexistent/lazarobox-uv"),
        &[],
        Duration::from_secs(5),
    );
    assert_eq!(missing, Err(Fail::NotFound));
    let failing = run_with(sh, &["-c", "echo no >&2; exit 3"], Duration::from_secs(5));
    assert_eq!(failing, Err(Fail::Other("uv failed")));
    let started = std::time::Instant::now();
    let slow = run_with(sh, &["-c", "sleep 30"], Duration::from_millis(300));
    assert_eq!(slow, Err(Fail::Other("uv did not answer in 5 s")));
    assert!(started.elapsed() < Duration::from_secs(5));
}
