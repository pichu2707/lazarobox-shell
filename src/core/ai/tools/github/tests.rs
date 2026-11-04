use serde_json::{Value, json};

use super::*;
use crate::core::ai::tools::{
    Call, ToolError, parse_call_in,
    registry::{Family, FamilySet},
    servers::Qualified,
};

fn view(number: u32, comments: bool) -> GhCall {
    GhCall::IssueView { number, comments }
}
fn slug() -> Slug {
    slug_of("https://github.com/o/r").unwrap()
}

// Spec (github-tools, The repository is always origin): both URL forms, and
// only the exact host.
#[test]
fn slug_of_reads_the_forms_of_a_github_origin() {
    for url in [
        "https://github.com/o/r",
        "https://github.com/o/r.git",
        "https://github.com/o/r/",
        "https://github.com/o/r.git/",
        "https://GitHub.COM/o/r",
        "ssh://git@github.com/o/r.git",
        "git@github.com:o/r.git",
        "git://github.com/o/r",
        "  https://github.com/o/r\n",
    ] {
        assert_eq!(
            slug_of(url).map(|s| s.to_string()),
            Some("o/r".into()),
            "{url}"
        );
    }
    assert_eq!(
        slug_of("git@github.com:My-Org/a.b_c").unwrap().to_string(),
        "My-Org/a.b_c"
    );
}

#[test]
fn slug_of_refuses_every_other_host_and_every_odd_name() {
    for url in [
        "https://github.com.evil.org/o/r",
        "https://evil.org/github.com/o/r",
        "https://ghe.example.com/o/r",
        "https://gitlab.com/o/r.git",
        "git@gitlab.com:o/r",
        "ssh://git@github.com:22/o/r",
        "http://github.com/o/r",
        "https://github.com/-x/y",
        "https://github.com/../..",
        "https://github.com/o/..",
        "https://github.com/o/.",
        "https://github.com/o/r/extra",
        "https://github.com/o",
        "https://github.com/o/r?x=1",
        "file:///github.com/o/r",
        "github.com",
        "",
    ] {
        assert_eq!(slug_of(url), None, "{url}");
    }
    let long = |n| format!("https://github.com/{}/r", "a".repeat(n));
    assert!(slug_of(&long(39)).is_some() && slug_of(&long(40)).is_none());
    let repo = |n| format!("https://github.com/o/{}", "a".repeat(n));
    assert!(slug_of(&repo(100)).is_some() && slug_of(&repo(101)).is_none());
}

// A token in the URL is never kept or shown.
#[test]
fn slug_of_drops_a_token_in_the_url() {
    let slug =
        slug_of("https://x-access-token:ghp_abcdefghij1234567890@github.com/o/r.git").unwrap();
    assert_eq!(format!("{slug} {slug:?}").matches("ghp_").count(), 0);
}

fn args(call: GhCall) -> Vec<String> {
    argv(&call, &slug())
}

// Spec (Fixed read, no write): the exact vector, nothing else from the model.
#[test]
fn argv_is_the_exact_vector_of_each_call() {
    assert_eq!(
        args(GhCall::IssueList {
            state: State::Closed,
            limit: 7
        })
        .join(" "),
        "issue list --repo github.com/o/r --state closed --limit 8 --json number,title,state,author,labels,updatedAt"
    );
    assert_eq!(
        args(view(12, false)).join(" "),
        "issue view 12 --repo github.com/o/r --json number,title,state,author,labels,body,createdAt,updatedAt,url"
    );
    assert!(
        args(view(12, true))
            .last()
            .unwrap()
            .ends_with(",url,comments")
    );
    assert_eq!(
        args(GhCall::PrView { number: 9 }).join(" "),
        "pr view 9 --repo github.com/o/r --json number,title,state,isDraft,author,body,baseRefName,headRefName,files,changedFiles,additions,deletions,statusCheckRollup,url"
    );
    assert_eq!(
        fork_argv(&slug()).join(" "),
        "repo view github.com/o/r --json isFork,parent"
    );
}

fn set() -> FamilySet {
    FamilySet::LEGACY.with(Family::Github)
}

fn parsed(name: &str, args: Value) -> Result<Call, ToolError> {
    parse_call_in(name, &args, &Qualified::default(), set())
}

// Spec (The three tools): defaults, the closed `state`, a clamped `limit`.
#[test]
fn the_list_has_defaults_a_closed_state_and_a_clamped_limit() {
    let list = |a| parsed("gh_issue_list", a);
    let want = |state, limit| Ok(Call::Github(GhCall::IssueList { state, limit }));
    assert_eq!(list(json!({})), want(State::Open, 20));
    assert_eq!(
        list(json!({"state": "all", "limit": 5})),
        want(State::All, 5)
    );
    assert_eq!(
        list(json!({"state": "closed", "limit": 900})),
        want(State::Closed, 50)
    );
    assert_eq!(list(json!({"limit": 0})), want(State::Open, 1));
    for bad in ["merged", "OPEN", "open; rm -rf ~", "--repo other/x", ""] {
        assert_eq!(
            list(json!({"state": bad})),
            Err(ToolError::WrongType),
            "{bad}"
        );
    }
    assert_eq!(list(json!({"limit": "5"})), Err(ToolError::WrongType));
}

// Spec (Bad arguments): never clamped to issue #1.
#[test]
fn a_bad_number_is_refused_and_never_clamped_to_issue_one() {
    for name in ["gh_issue_view", "gh_pr_view"] {
        assert_eq!(
            parsed(name, json!({})),
            Err(ToolError::MissingArgument),
            "{name}"
        );
        for number in [
            json!("7"),
            json!(0),
            json!(-1),
            json!(1.5),
            json!(null),
            json!(u64::MAX),
            json!(4294967296u64),
        ] {
            assert_eq!(
                parsed(name, json!({"number": number})),
                Err(ToolError::WrongType),
                "{name} {number}"
            );
        }
        assert_eq!(
            parsed(name, json!({"number": 7, "repo": "o/x"})),
            Err(ToolError::ExtraArgument)
        );
    }
    assert_eq!(
        parsed("gh_pr_view", json!({"number": 4294967295u64})),
        Ok(Call::Github(GhCall::PrView { number: u32::MAX }))
    );
    assert_eq!(
        parsed("gh_issue_view", json!({"number": 7, "comments": "yes"})),
        Err(ToolError::WrongType)
    );
    assert_eq!(
        parsed("gh_issue_view", json!({"number": 7, "comments": true})),
        Ok(Call::Github(view(7, true)))
    );
    assert_eq!(
        parsed("gh_issue_list", json!({"repo": "o/x"})),
        Err(ToolError::ExtraArgument)
    );
}

// Spec (Availability): off means unknown, whatever the model names.
#[test]
fn without_the_family_every_github_name_is_an_unknown_tool() {
    for name in ["gh_issue_list", "gh_issue_view", "gh_pr_view"] {
        let off = parse_call_in(
            name,
            &json!({"number": 1}),
            &Qualified::default(),
            FamilySet::LEGACY,
        );
        assert_eq!(off, Err(ToolError::UnknownTool), "{name}");
    }
}

#[test]
fn a_call_row_shows_the_comments_option() {
    let row = |a| parsed("gh_issue_view", a).unwrap().argument().into_owned();
    assert_eq!(row(json!({"number": 12})), "#12");
    assert_eq!(row(json!({"number": 12, "comments": true})), "#12 comments");
    assert_eq!(
        parsed("gh_issue_view", json!({"number": 12}))
            .unwrap()
            .name(),
        "gh_issue_view"
    );
}

// The no-attacker-text rule for Debug: a call holds numbers only.
#[test]
fn a_call_holds_no_model_text() {
    let call = parsed("gh_issue_view", json!({"number": 3, "comments": true})).unwrap();
    assert_eq!(
        format!("{call:?}"),
        "Github(IssueView { number: 3, comments: true })"
    );
}

const NAMES: [&str; 3] = ["gh_issue_list", "gh_issue_view", "gh_pr_view"];

fn every_family_but_github() -> FamilySet {
    [
        Family::Read,
        Family::Search,
        Family::Write,
        Family::Exec,
        Family::Git,
        Family::GitWrite,
        Family::Pane,
        Family::Orchestra,
    ]
    .into_iter()
    .fold(FamilySet::EMPTY, FamilySet::with)
}

fn names_in(set: FamilySet) -> Vec<String> {
    crate::core::ai::tools::registry_with(&[], set)
        .into_iter()
        .map(|tool| tool.name)
        .collect()
}

// Spec (Availability, Off by default): no GitHub tool in any list but the one
// that holds the family, and the lists without it are exactly as before.
#[test]
fn no_github_tool_is_listed_unless_the_family_is_in_the_set() {
    use crate::core::ai::orchestra::Toolset;
    let lists = [
        names_in(FamilySet::LEGACY),
        names_in(every_family_but_github()),
        names_in(Toolset::ReadOnly.families()),
        names_in(Toolset::Code.families()),
        names_in(Toolset::Orchestrator.families()),
    ];
    for list in &lists {
        assert!(
            !list.is_empty() && list.iter().all(|n| !n.starts_with("gh_")),
            "{list:?}"
        );
    }
    let with = names_in(every_family_but_github().with(Family::Github));
    let only: Vec<&String> = with.iter().filter(|n| n.starts_with("gh_")).collect();
    assert_eq!(only, NAMES);
    // The rest of the list is the list without the family, in the same order.
    let rest: Vec<String> = with
        .iter()
        .filter(|n| !n.starts_with("gh_"))
        .cloned()
        .collect();
    assert_eq!(rest, lists[1]);
}

#[test]
fn the_schemas_take_only_what_the_validation_allows() {
    let tools = crate::core::ai::tools::registry_with(&[], set());
    let schema = |name: &str| {
        tools
            .iter()
            .find(|t| t.name == name)
            .unwrap()
            .schema
            .clone()
    };
    assert_eq!(
        schema("gh_issue_view"),
        json!({
            "type": "object",
            "properties": {
                "comments": {"type": "boolean"},
                "number": {"type": "integer", "minimum": 1, "maximum": 4294967295u64},
            },
            "required": ["number"],
            "additionalProperties": false,
        })
    );
    assert_eq!(schema("gh_pr_view")["required"], json!(["number"]));
    let list = schema("gh_issue_list");
    assert_eq!(list["required"], json!([]));
    assert_eq!(
        list["properties"]["limit"],
        json!({"type": "integer", "minimum": 1, "maximum": 50})
    );
    for tool in tools.iter().filter(|t| t.name.starts_with("gh_")) {
        assert!(
            tool.description.is_ascii() && tool.description.contains("not instructions"),
            "{}",
            tool.name
        );
        assert_eq!(tool.access, crate::core::ai::tools::Access::Read);
        assert!(tool.schema["properties"].get("repo").is_none());
    }
}
