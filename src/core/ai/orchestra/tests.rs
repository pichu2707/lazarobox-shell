use serde_json::json;

use super::tools::{MAX_PROMPT_BYTES, OrchCall, OrchError, WAIT_MAX_SECS, parse, specs};
use super::{Toolset, WorkerId};
use crate::core::ai::config::Task;

#[test]
fn spawn_accepts_the_five_worker_kinds() {
    for task in Task::SET_ALL {
        let args = json!({ "kind": task.id(), "prompt": "look at src" });
        assert_eq!(
            parse("spawn_agent", &args),
            Ok(OrchCall::Spawn {
                task,
                prompt: "look at src".to_owned()
            })
        );
    }
}

#[test]
fn spawn_refuses_what_a_worker_cannot_be() {
    let long = "x".repeat(MAX_PROMPT_BYTES + 1);
    for (args, error) in [
        (
            json!({ "kind": "deploy", "prompt": "p" }),
            OrchError::BadKind,
        ),
        (
            json!({ "kind": "orchestrate", "prompt": "p" }),
            OrchError::BadKind,
        ),
        (
            json!({ "kind": "review", "prompt": "" }),
            OrchError::EmptyPrompt,
        ),
        (
            json!({ "kind": "review", "prompt": long }),
            OrchError::TooLong,
        ),
        (json!({ "kind": "review" }), OrchError::MissingArgument),
        (json!({ "kind": 3, "prompt": "p" }), OrchError::WrongType),
        (
            json!({ "kind": "review", "prompt": "p", "x": 1 }),
            OrchError::ExtraArgument,
        ),
        (json!(["review"]), OrchError::NotAnObject),
    ] {
        assert_eq!(parse("spawn_agent", &args), Err(error), "{args}");
    }
}

#[test]
fn wait_is_clamped_to_the_ceiling() {
    for (args, secs) in [
        (json!({ "ids": [1], "timeout": 600 }), WAIT_MAX_SECS),
        (json!({ "ids": [1], "timeout": 5 }), 5),
        (json!({ "ids": [1] }), WAIT_MAX_SECS),
    ] {
        assert_eq!(
            parse("wait", &args),
            Ok(OrchCall::Wait {
                ids: vec![WorkerId(1)],
                timeout: secs
            }),
            "{args}"
        );
    }
}

#[test]
fn wait_collect_and_cancel_validate_their_ids() {
    assert_eq!(
        parse("collect", &json!({ "id": 2 })),
        Ok(OrchCall::Collect { id: WorkerId(2) })
    );
    assert_eq!(
        parse("cancel", &json!({ "id": 2 })),
        Ok(OrchCall::Cancel { id: WorkerId(2) })
    );
    for (name, args, error) in [
        ("wait", json!({ "ids": [] }), OrchError::MissingArgument),
        ("wait", json!({ "ids": ["a"] }), OrchError::WrongType),
        (
            "wait",
            json!({ "ids": [1], "timeout": -1 }),
            OrchError::WrongType,
        ),
        (
            "collect",
            json!({ "id": 4294967296u64 }),
            OrchError::WrongType,
        ),
        ("cancel", json!({}), OrchError::MissingArgument),
        (
            "run_in_pane",
            json!({ "command": "ls" }),
            OrchError::UnknownTool,
        ),
    ] {
        assert_eq!(parse(name, &args), Err(error), "{name} {args}");
    }
}

#[test]
fn the_registry_is_exactly_the_four_tools_and_workers_are_read_only() {
    let names: Vec<_> = specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(names, ["spawn_agent", "wait", "collect", "cancel"]);
    for task in Task::SET_ALL {
        assert_eq!(Toolset::for_kind(task), Toolset::ReadOnly);
    }
}
