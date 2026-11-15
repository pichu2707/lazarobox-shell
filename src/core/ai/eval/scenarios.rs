//! The tasks of the baseline (openspec `local-multistep`, C2): plain data,
//! engine-neutral.

use serde_json::Value;

use super::{Category, Expect, Literal, Scenario};

/// A `path` argument whose last segment is exactly `name`.
fn path_ends(args: &Value, name: &str) -> bool {
    args["path"]
        .as_str()
        .is_some_and(|p| p.rsplit('/').next() == Some(name))
}

/// A `run_command` that runs pytest.
fn runs_pytest(args: &Value) -> bool {
    args["program"] == "pytest"
        || args["args"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == "pytest"))
}

const ANY: fn(&Value) -> bool = |_| true;

const BATCH: Expect = Expect {
    label: "batch",
    tool: "propose_batch",
    args: ANY,
};
const PYTEST: Expect = Expect {
    label: "pytest",
    tool: "run_command",
    args: runs_pytest,
};

/// (a) A new function, its test, a test run and a commit with the user's text.
pub const WRITE_AND_TEST: Scenario = Scenario {
    id: "write-and-test",
    category: Category::WriteAndTest,
    request: "Write a function fib(n) in fib.py that returns the n-th Fibonacci number, add a pytest test for it in test_fib.py, run the test with pytest, and commit with the message \"add fibonacci\".",
    seed: &[("README.md", "# Sample project\n")],
    steps: &[
        Expect {
            label: "write",
            tool: "write_file",
            args: |a| path_ends(a, "fib.py"),
        },
        Expect {
            label: "write-test",
            tool: "write_file",
            args: |a| path_ends(a, "test_fib.py"),
        },
        BATCH,
        PYTEST,
        Expect {
            label: "commit",
            tool: "git_commit",
            args: ANY,
        },
    ],
    order: &[
        ("batch", "pytest"),
        ("batch", "commit"),
        ("pytest", "commit"),
    ],
    literals: &[Literal {
        step: "commit",
        text: "add fibonacci",
    }],
    forbid: &[],
    ambiguous: false,
};

/// (b) An edit of a file that already has tests.
pub const EDIT_EXISTING: Scenario = Scenario {
    id: "edit-existing",
    category: Category::EditExisting,
    request: "In calc.py, make add(a, b) also accept a list as its only argument and return the sum of the list. Keep the existing tests passing and run them with pytest.",
    seed: &[
        ("calc.py", "def add(a, b):\n    return a + b\n"),
        (
            "test_calc.py",
            "from calc import add\n\n\ndef test_add():\n    assert add(1, 2) == 3\n",
        ),
    ],
    steps: &[
        Expect {
            label: "edit",
            tool: "edit_file",
            args: |a| path_ends(a, "calc.py"),
        },
        BATCH,
        PYTEST,
    ],
    order: &[("batch", "pytest")],
    literals: &[],
    forbid: &[],
    ambiguous: false,
};

/// (c) A question about the repo: nothing may be written, run or committed.
pub const READ_ONLY: Scenario = Scenario {
    id: "read-only",
    category: Category::ReadOnly,
    request: "What does the function in text_utils.py do?",
    seed: &[(
        "text_utils.py",
        "def slug(text):\n    return text.strip().lower().replace(\" \", \"-\")\n",
    )],
    steps: &[Expect {
        label: "read",
        tool: "read_file",
        args: |a| path_ends(a, "text_utils.py"),
    }],
    order: &[],
    literals: &[],
    forbid: &[
        "write_file",
        "edit_file",
        "create_dir",
        "propose_batch",
        "run_command",
        "git_add",
        "git_commit",
    ],
    ambiguous: false,
};

/// A `handoff` that closes the step `name`.
fn closes(args: &Value, name: &str) -> bool {
    args["step"] == name
}

/// (d) A coordinator that must CALL the orchestration tools: close the steps,
/// then spawn a simple worker with a file to write. Describing the calls in
/// prose, or writing them as text, completes nothing.
pub const COORDINATE: Scenario = Scenario {
    id: "coordinate",
    category: Category::Coordinate,
    request: "Create greeting.py with a function greet(name) that returns \"Hello, \" followed by the name.",
    seed: &[("README.md", "# Sample project\n")],
    steps: &[
        Expect {
            label: "authorize",
            tool: "handoff",
            args: |a| closes(a, "authorize") && a["scope"] == "implement",
        },
        Expect {
            label: "explore",
            tool: "handoff",
            args: |a| closes(a, "explore"),
        },
        Expect {
            label: "resolve",
            tool: "handoff",
            args: |a| closes(a, "resolve"),
        },
        Expect {
            label: "classify",
            tool: "handoff",
            args: |a| closes(a, "classify") && a["size"] == "small",
        },
        Expect {
            label: "spawn",
            tool: "spawn_agent",
            args: |a| {
                a["kind"] == "simple" && a["outputs"].as_array().is_some_and(|o| !o.is_empty())
            },
        },
    ],
    order: &[
        ("authorize", "explore"),
        ("explore", "resolve"),
        ("resolve", "classify"),
        ("classify", "spawn"),
    ],
    literals: &[],
    forbid: &[
        "write_file",
        "edit_file",
        "create_dir",
        "propose_batch",
        "run_command",
        "git_add",
        "git_commit",
    ],
    ambiguous: false,
};

/// Every task, in the order the table lists them.
pub const ALL: [&Scenario; 4] = [&WRITE_AND_TEST, &EDIT_EXISTING, &READ_ONLY, &COORDINATE];
