use super::*;

// What `git init` and `git clone` write is plain.
#[test]
fn what_init_and_clone_write_is_plain() {
    for text in [
        "",
        "# a comment\n; another\n",
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n",
        "[core]\r\n\tbare = false\r\n",
        "[remote \"origin\"]\n\turl = git@example.com:a/b.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
        "[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n",
        "[remote.origin]\n\turl = x\n",
        "[user]\n\tname = A\n\temail = a@b.c\n[init]\n\tdefaultBranch = main\n",
        "[core]\n\tautocrlf = input # a comment\n",
        "[core] ignorecase = true",
    ] {
        assert!(is_plain(text), "{text:?}");
    }
}

// Ruling 2 and L29: every key that is not known harmless asks, the ones
// that run a program first among them.
#[test]
fn any_other_key_is_not_plain() {
    for text in [
        "[core]\n\tfsmonitor = .git/hooks/x",
        "[core]\n  pager = less",
        "[Core]\nFSMonitor",
        "[core]\n\thooksPath = /x",
        "[core]\n\tworktree = /srv",
        "[core]\n\tbare = true",
        "[core]\n\tbare",
        "[pager]\n\tlog = sh -c x",
        "[diff]\n\texternal = ./x",
        "[diff \"bin\"]\n\ttextconv = ./x",
        "[diff.bin]\n\tcommand = ./x",
        "[filter \"lfs\"]\n\tclean = ./x",
        "[log]\n\tshowSignature = false",
        "[gpg]\n\tprogram = ./x",
        "[include]\n\tpath = extra",
        "[includeIf \"gitdir:/x/\"]\n\tpath = extra",
        "[user]\n\tsigningkey = x",
        "[remote \"o\"]\n\tuploadpack = ./x",
    ] {
        assert!(!is_plain(text), "{text:?}");
    }
}

// B1: a continued line hides a header and a key from a line reader.
#[test]
fn a_continued_line_or_anything_unclear_is_not_plain() {
    for text in [
        "[core]\n\tcomment = x \\\n[foo]\n\tfsmonitor = /x/hook\n",
        "[user]\n\tname = \"a\\\"b\"",
        "\u{feff}[core]\n\tbare = false",
        "[core]\n\tbare = false\u{0}",
        "fsmonitor = x",
        "[core\n\tbare = false",
        "[core]\n\t=x",
        "[core]\n\tbare false = x",
        "[remote \"a]\n\turl = x",
    ] {
        assert!(!is_plain(text), "{text:?}");
    }
}
