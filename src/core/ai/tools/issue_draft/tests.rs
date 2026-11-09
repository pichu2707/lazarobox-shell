//! Spec (issue-draft, agent-tools): the arguments of `gh_issue_draft` are
//! checked here; a field that would not fit the form is refused, never cut.

use serde_json::{Value, json};

use super::*;
use crate::core::ai::{
    orchestra::Toolset,
    tools::{Access, Call, mcp::refusal_text, parse_call_in, registry::Family, registry_with},
};
use crate::core::menu::feedback::Kind;

fn set() -> crate::core::ai::tools::registry::FamilySet {
    crate::core::ai::tools::registry::FamilySet::LEGACY.with(Family::IssueDraft)
}

fn parse(args: Value) -> Result<Call, ToolError> {
    parse_call_in("gh_issue_draft", &args, &Default::default(), set())
}

fn ok(kind: &str, title: &str, description: &str) -> Value {
    json!({"kind": kind, "title": title, "description": description})
}

#[test]
fn a_valid_draft_parses_into_its_call() {
    let call = parse(ok("idea", "See the shell over ssh", "Why and what.")).unwrap();
    assert_eq!(
        call,
        Call::IssueDraft(IssueDraft {
            kind: Kind::Idea,
            title: "See the shell over ssh".into(),
            description: "Why and what.".into(),
        })
    );
    assert_eq!(call.name(), "gh_issue_draft");
    assert_eq!(call.family(), Some(Family::IssueDraft));
    assert_eq!(call.argument(), "Idea or goal: See the shell over ssh");
    let bug = parse(ok("bug", "It crashes", "Steps.")).unwrap();
    assert!(matches!(
        bug,
        Call::IssueDraft(IssueDraft {
            kind: Kind::Bug,
            ..
        })
    ));
    // The title and the description stay the model's own text: not in Debug.
    assert!(!format!("{call:?}").contains("ssh"));
}

// D4: kind, title and description are required, and each error is the model's
// to fix.
#[test]
fn each_required_argument_is_required() {
    for name in ["kind", "title", "description"] {
        let mut args = ok("idea", "t", "d");
        args.as_object_mut().unwrap().remove(name);
        assert_eq!(parse(args), Err(ToolError::MissingArgument), "{name}");
    }
    assert_eq!(
        parse(json!({"kind": "idea", "title": "t", "description": "d", "extra": 1})),
        Err(ToolError::ExtraArgument)
    );
    assert_eq!(
        parse(json!({"kind": 1, "title": "t", "description": "d"})),
        Err(ToolError::WrongType)
    );
}

#[test]
fn an_unknown_kind_is_refused() {
    for kind in ["goal", "Idea", "", " bug"] {
        assert_eq!(
            parse(ok(kind, "t", "d")),
            Err(ToolError::DraftKind),
            "{kind:?}"
        );
    }
    assert_eq!(
        refusal_text(ToolError::DraftKind),
        "kind must be idea or bug"
    );
}

#[test]
fn an_empty_title_or_description_is_refused() {
    for (title, description) in [("", "d"), ("t", ""), ("   ", "d"), ("t", " \n\t ")] {
        assert_eq!(
            parse(ok("idea", title, description)),
            Err(ToolError::DraftEmpty),
            "{title:?} {description:?}"
        );
    }
    assert_eq!(
        refusal_text(ToolError::DraftEmpty),
        "title and description must not be empty"
    );
}

// D4: a field over its cap is refused, never cut; the cap is the form's and
// counts characters (a wide title can be longer in bytes).
#[test]
fn a_field_over_its_cap_is_refused_and_the_limit_itself_is_accepted() {
    assert_eq!(TITLE_CAP, 120);
    assert_eq!(DESCRIPTION_CAP, 2000);
    let at = "é".repeat(TITLE_CAP);
    assert!(at.len() > TITLE_CAP);
    assert_eq!(
        parse(ok("idea", &at, "d")).map(|c| c.name().to_owned()),
        Ok("gh_issue_draft".to_owned())
    );
    assert_eq!(
        parse(ok("idea", &"é".repeat(TITLE_CAP + 1), "d")),
        Err(ToolError::DraftTitleLong)
    );
    assert_eq!(
        refusal_text(ToolError::DraftTitleLong),
        "title must be at most 120 characters"
    );
    assert!(parse(ok("idea", "t", &"d".repeat(DESCRIPTION_CAP))).is_ok());
    assert_eq!(
        parse(ok("idea", "t", &"d".repeat(DESCRIPTION_CAP + 1))),
        Err(ToolError::DraftBodyLong)
    );
    assert_eq!(
        refusal_text(ToolError::DraftBodyLong),
        "description must be at most 2000 characters"
    );
    // Far over the cap is still the same specific refusal.
    assert_eq!(
        parse(ok("idea", &"t".repeat(5_000), "d")),
        Err(ToolError::DraftTitleLong)
    );
}

// Nothing is cut: a tab grows into spaces in the form, so a text that would
// grow over the cap is refused rather than clipped by the paste.
#[test]
fn a_description_that_grows_past_its_cap_in_the_form_is_refused() {
    let tabs = format!("a\t{}", "d".repeat(DESCRIPTION_CAP - 2));
    assert_eq!(parse(ok("idea", "t", &tabs)), Err(ToolError::DraftBodyLong));
}

#[test]
fn a_title_with_a_line_break_is_refused() {
    assert_eq!(
        parse(ok("idea", "first\nsecond", "d")),
        Err(ToolError::DraftTitleLines)
    );
    assert_eq!(
        refusal_text(ToolError::DraftTitleLines),
        "title must be one line"
    );
}

// What the form will hold is what the prompt names: outer spaces and control
// characters are dropped exactly as a paste would drop them.
#[test]
fn the_draft_is_kept_as_the_form_will_hold_it() {
    let call = parse(ok("idea", "  a\u{1b}[0m b  ", "\n one\ntwo \n")).unwrap();
    assert_eq!(
        call,
        Call::IssueDraft(IssueDraft {
            kind: Kind::Idea,
            title: "a[0m b".into(),
            description: "one\ntwo".into(),
        })
    );
}

// agent-tools, Tool registry (D1): the family adds exactly one tool, read-only
// in its classification.
#[test]
fn the_family_adds_exactly_one_tool() {
    let base = crate::core::ai::tools::registry::FamilySet::LEGACY;
    let off = registry_with(&[], base);
    assert!(off.iter().all(|t| t.name != "gh_issue_draft"));
    let on = registry_with(&[], base.with(Family::IssueDraft));
    assert_eq!(on.len(), off.len() + 1);
    let tool = on.iter().find(|t| t.name == "gh_issue_draft").unwrap();
    assert_eq!(tool.access, Access::Read);
    assert_eq!(
        tool.schema["required"],
        json!(["kind", "title", "description"])
    );
    assert_eq!(tool.schema["additionalProperties"], json!(false));
    // Off, a call is answered like a name that does not exist.
    assert_eq!(
        parse_call_in(
            "gh_issue_draft",
            &ok("idea", "t", "d"),
            &Default::default(),
            base
        ),
        Err(ToolError::UnknownTool)
    );
}

// Workers and the orchestrator turn get no draft tool (D: agent-tools).
#[test]
fn no_worker_toolset_is_offered_the_draft() {
    for toolset in [Toolset::ReadOnly, Toolset::Code, Toolset::Orchestrator] {
        assert!(
            !toolset.families().contains(Family::IssueDraft),
            "{toolset:?}"
        );
    }
}

// github-tools, Fixed read, no write: the draft tool runs no `gh`, so with
// every family on the only write-shaped name is still refused and
// gh_issue_draft is the sole addition to the read tools.
#[test]
fn with_every_family_on_no_tool_writes_to_github() {
    let all = crate::core::ai::tools::registry::FamilySet::ALL;
    let listed = registry_with(&[], all);
    let github: Vec<&str> = listed
        .iter()
        .map(|t| t.name.as_str())
        .filter(|n| n.starts_with("gh"))
        .collect();
    assert_eq!(
        github,
        [
            "gh_issue_list",
            "gh_issue_view",
            "gh_pr_view",
            "gh_issue_draft"
        ]
    );
    for write in [
        "gh_issue_create",
        "gh_issue_comment",
        "gh_pr_create",
        "gh_api",
        "gh",
    ] {
        assert_eq!(
            parse_call_in(write, &json!({}), &Default::default(), all),
            Err(ToolError::UnknownTool),
            "{write}"
        );
    }
}
