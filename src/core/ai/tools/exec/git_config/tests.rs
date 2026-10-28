use super::*;

fn runs(text: &str) -> bool {
    scan(text).expect(text).runs_program
}

// Ruling 2: every key that makes status, log or diff run a program.
#[test]
fn a_key_that_runs_a_program_is_found() {
    for text in [
        "[core]\n\tfsmonitor = .git/hooks/x",
        "[core]\n  pager = less",
        "[Core]\nFSMonitor",
        "[pager]\n\tlog = sh -c x",
        "[diff]\n\texternal = ./x",
        "[diff \"bin\"]\n\ttextconv = ./x",
        "[diff \"bin\"]\n\tcommand = ./x",
        "[diff.bin]\n\ttextconv = ./x",
        "[filter \"lfs\"]\n\tclean = ./x",
        "[filter \"lfs\"]\n\tprocess = ./x",
        "[filter \"lfs\"]\n\tsmudge = ./x",
        "[core] fsmonitor = x",
        "[user]\n\tname = a\n[core]\n\tpager = less",
    ] {
        assert!(runs(text), "{text:?}");
    }
}

// `git log` verifies signatures with a program the config names.
#[test]
fn a_signature_check_or_a_gpg_program_is_found() {
    for text in [
        "[log]\n\tshowSignature = true",
        "[log]\n\tshowsignature",
        "[log]\n\tshowSignature = yes",
        "[gpg]\n\tprogram = ./x",
        "[gpg \"openpgp\"]\n\tprogram = ./x",
        "[gpg \"x509\"]\n\tprogram = ./x",
        "[gpg \"ssh\"]\n\tprogram = ./x",
    ] {
        assert!(runs(text), "{text:?}");
    }
    for text in [
        "[log]\n\tshowSignature = false",
        "[log]\n\tshowSignature = off",
        "[log]\n\tdecorate = short",
        "[gpg]\n\tformat = ssh",
    ] {
        assert!(!runs(text), "{text:?}");
    }
}

#[test]
fn a_clean_config_runs_nothing() {
    for text in [
        "",
        "# a comment\n; another\n",
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true",
        "[remote \"origin\"]\n\turl = git@example.com:a/b.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*",
        "[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main",
        "[diff]\n\talgorithm = histogram",
        "[filter \"lfs\"]\n\trequired = true",
        "[core]\n\tautocrlf = input # trailing comment",
    ] {
        assert!(!runs(text), "{text:?}");
    }
}

#[test]
fn includes_are_listed_for_the_caller_to_read() {
    let got = scan(
        "[include]\n\tpath = extra.conf\n[includeIf \"gitdir:~/w/\"]\n\tpath = \"/etc/w.conf\"\n[user]\n\tname = a",
    )
    .unwrap();
    assert_eq!(got.includes, ["extra.conf", "/etc/w.conf"]);
    assert!(!got.runs_program);
}

// What the scan cannot read for sure is no clean config: the caller asks.
#[test]
fn what_cannot_be_parsed_is_refused() {
    for text in [
        "fsmonitor = x",
        "[core\n\tpager = x",
        "[core]\n\t=x",
        "[core]\n\tpa ger = x",
        "[diff \"a]\n\ttextconv = x",
        "[include]\n\tpath = a\\\n b",
        "[include]\n\tpath = \"a\\\"b\"",
    ] {
        assert_eq!(scan(text), Err(Unparsed), "{text:?}");
    }
}
