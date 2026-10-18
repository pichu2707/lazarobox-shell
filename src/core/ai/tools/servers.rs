//! Which user MCP servers a chat uses, and how their tools are named.
//!
//! Pure: no file is read and nothing is started (slice 4a). A server is only
//! ever declared in the user's own `config.toml`; a repository cannot add one.

use std::path::Path;

use super::ExternalTool;
use crate::core::ai::config::McpServer;

/// The longest server or tool part of a qualified name, in characters.
pub const MAX_PART: usize = 24;

/// The servers a chat opened under `origin_root` (the git root of its origin
/// cwd, `None` outside a repository) uses: the enabled global ones, plus the
/// enabled ones declared for exactly that root. A project entry replaces a
/// global one of the same name.
pub fn resolve<'a>(servers: &'a [McpServer], origin_root: Option<&Path>) -> Vec<&'a McpServer> {
    let mut active: Vec<&McpServer> = servers
        .iter()
        .filter(|s| s.enabled && s.project.is_none())
        .collect();
    let local = servers.iter().filter(|s| {
        s.enabled
            && s.project
                .as_deref()
                .is_some_and(|root| Some(root) == origin_root)
    });
    for server in local {
        match active.iter().position(|kept| kept.name == server.name) {
            Some(at) => active[at] = server,
            None => active.push(server),
        }
    }
    active
}

/// One part of a qualified name: lowercase ASCII, anything outside
/// `[a-z0-9_-]` becomes `_`, cut at [`MAX_PART`]. Empty gives `None`.
fn clean_part(part: &str) -> Option<String> {
    let clean: String = part
        .chars()
        .map(|c| match c.to_ascii_lowercase() {
            c @ ('a'..='z' | '0'..='9' | '_' | '-') => c,
            _ => '_',
        })
        .take(MAX_PART)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

/// `<server>__<tool>`, each part matching `[a-z0-9_-]{1,24}`. Always holds a
/// `__`, which no built-in name does, so it can never equal a built-in.
pub fn qualified_name(server: &str, tool: &str) -> Option<String> {
    Some(format!("{}__{}", clean_part(server)?, clean_part(tool)?))
}

/// The tools of `server` under their qualified names. A tool whose name
/// cannot be qualified is dropped, and of tools that qualify to the same name
/// the first is kept.
pub fn qualify(server: &str, tools: &[ExternalTool]) -> Vec<ExternalTool> {
    let mut out: Vec<ExternalTool> = Vec::new();
    for tool in tools {
        let Some(name) = qualified_name(server, &tool.name) else {
            continue;
        };
        if out.iter().all(|kept| kept.name != name) {
            out.push(ExternalTool {
                name,
                ..tool.clone()
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::core::ai::config::McpServer;
    use crate::core::ai::tools::{ExternalTool, registry};

    fn server(name: &str, command: &str, project: Option<&str>) -> McpServer {
        McpServer {
            name: name.to_owned(),
            command: command.to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            project: project.map(Into::into),
            enabled: true,
        }
    }

    fn active<'a>(list: &'a [McpServer], root: Option<&str>) -> Vec<(&'a str, &'a str)> {
        resolve(list, root.map(Path::new))
            .into_iter()
            .map(|s| (s.name.as_str(), s.command.as_str()))
            .collect()
    }

    fn lists() -> Vec<McpServer> {
        vec![
            server("a", "ga", None),
            server("b", "gb", None),
            server("c", "pc", Some("/p")),
        ]
    }

    // Spec (mcp-servers): Global plus project.
    #[test]
    fn global_and_the_project_of_the_origin_root_are_active() {
        assert_eq!(
            active(&lists(), Some("/p")),
            [("a", "ga"), ("b", "gb"), ("c", "pc")]
        );
    }

    // Spec: Other project excluded.
    #[test]
    fn another_root_or_no_root_gets_the_global_list_only() {
        assert_eq!(active(&lists(), Some("/q")), [("a", "ga"), ("b", "gb")]);
        assert_eq!(active(&lists(), None), [("a", "ga"), ("b", "gb")]);
        // A subdirectory is not the root: only the git root matches.
        assert_eq!(active(&lists(), Some("/p/src")), [("a", "ga"), ("b", "gb")]);
        assert_eq!(
            active(&lists(), Some("/p/")).len(),
            3,
            "a trailing slash is the same root"
        );
    }

    // Spec: Same name, project wins.
    #[test]
    fn the_project_entry_wins_a_name_clash_and_only_in_its_root() {
        let list = vec![
            server("a", "global", None),
            server("a", "project", Some("/p")),
        ];
        assert_eq!(active(&list, Some("/p")), [("a", "project")]);
        assert_eq!(active(&list, Some("/q")), [("a", "global")]);
    }

    // Spec: enabled false.
    #[test]
    fn a_disabled_entry_is_not_active_and_a_disabled_project_entry_does_not_win() {
        let mut off = server("a", "project", Some("/p"));
        off.enabled = false;
        let list = vec![server("a", "global", None), off, server("b", "gb", None)];
        assert_eq!(active(&list, Some("/p")), [("a", "global"), ("b", "gb")]);
    }

    // Spec: qualified name `<server>__<tool>`, each part `[a-z0-9_-]{1,24}`.
    #[test]
    fn a_qualified_name_joins_clean_parts_with_a_double_underscore() {
        assert_eq!(
            qualified_name("fs", "read_file").as_deref(),
            Some("fs__read_file")
        );
        assert_eq!(
            qualified_name("My-Server", "Search.Docs").as_deref(),
            Some("my-server__search_docs")
        );
        assert_eq!(
            qualified_name("fs", "buscar año").as_deref(),
            Some("fs__buscar_a_o")
        );
    }

    #[test]
    fn every_part_is_capped_at_24_and_never_empty() {
        let long = "x".repeat(40);
        let got = qualified_name(&long, &long).unwrap();
        assert_eq!(got, format!("{}__{}", "x".repeat(24), "x".repeat(24)));
        assert_eq!(qualified_name("fs", ""), None);
        assert_eq!(qualified_name("", "t"), None);
    }

    // Spec (4a.2): a colliding external tool is qualified, never dropped.
    #[test]
    fn a_tool_named_like_a_built_in_is_exposed_under_its_qualified_name() {
        let ext = |name: &str| ExternalTool {
            name: name.to_owned(),
            description: "d".to_owned(),
            schema: json!({"type": "object"}),
        };
        let qualified = qualify("fs", &[ext("run_in_pane"), ext("read_file"), ext("search")]);
        let names: Vec<_> = qualified.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["fs__run_in_pane", "fs__read_file", "fs__search"]);
        let tools = registry(&qualified);
        let built_in = registry(&[]);
        assert_eq!(&tools[..4], &built_in[..], "the built-ins are unchanged");
        assert_eq!(tools.len(), 7);
    }

    // Spec (4a.2): same-named externals are deduplicated here.
    #[test]
    fn tools_that_qualify_to_the_same_name_keep_the_first() {
        let ext = |name: &str, description: &str| ExternalTool {
            name: name.to_owned(),
            description: description.to_owned(),
            schema: json!({}),
        };
        let qualified = qualify(
            "fs",
            &[
                ext("Search", "first"),
                ext("search", "second"),
                ext("", "none"),
                ext("list", "l"),
            ],
        );
        let got: Vec<_> = qualified
            .iter()
            .map(|t| (t.name.as_str(), t.description.as_str()))
            .collect();
        assert_eq!(got, [("fs__search", "first"), ("fs__list", "l")]);
        assert_eq!(qualify("", &[ext("x", "d")]), Vec::new());
    }

    // Spec (agent-tools): a duplicate in the registry never reaches a model.
    #[test]
    fn the_registry_lists_a_qualified_name_once() {
        let ext = |description: &str| ExternalTool {
            name: "fs__search".to_owned(),
            description: description.to_owned(),
            schema: json!({}),
        };
        let tools = registry(&[ext("first"), ext("second")]);
        assert_eq!(tools.len(), 5);
        assert_eq!(tools[4].description, "first");
    }
}
