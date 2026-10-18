//! Which user MCP servers a chat uses, and how their tools are named.
//!
//! Pure: no file is read and nothing is started (slice 4a). A server is only
//! ever declared in the user's own `config.toml`; a repository cannot add one.

use std::path::Path;

use super::ExternalTool;
use crate::core::ai::config::McpServer;
use crate::core::ai::sanitize::clean;

/// The longest server or tool part of a qualified name, in characters.
pub const MAX_PART: usize = 24;

/// The servers a chat opened under `origin_root` (the git root of its origin
/// cwd, `None` outside a repository) uses: the enabled global ones, then the
/// entries declared for exactly that root, which decide for that root. An
/// enabled project entry replaces a global one of the same name (or is added);
/// a disabled one removes it. A project entry can never add a server by being
/// disabled.
pub fn resolve<'a>(servers: &'a [McpServer], origin_root: Option<&Path>) -> Vec<&'a McpServer> {
    let mut active: Vec<&McpServer> = servers
        .iter()
        .filter(|s| s.enabled && s.project.is_none())
        .collect();
    let local = servers.iter().filter(|s| {
        s.project
            .as_deref()
            .is_some_and(|root| Some(root) == origin_root)
    });
    for server in local {
        let at = active.iter().position(|kept| kept.name == server.name);
        match (at, server.enabled) {
            (Some(at), true) => active[at] = server,
            (None, true) => active.push(server),
            (Some(at), false) => {
                active.remove(at);
            }
            (None, false) => {}
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

/// A server tool under its qualified name, with what routing needs. 4b finds
/// the server and the raw name by looking the qualified name up (`raw_of`),
/// never by splitting it on `__`: a raw name may hold that itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QualifiedTool {
    pub server: String,
    pub raw: String,
    /// The tool as the registry lists it (its `name` is the qualified one).
    pub tool: ExternalTool,
}

/// A tool left out of the registry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dropped {
    pub server: String,
    pub raw: String,
    /// The `(server, raw name)` of the tool that kept the name, or `None` when
    /// the name cannot be qualified at all.
    pub kept: Option<(String, String)>,
}

/// The longest raw name shown in a notice, in characters.
const NOTICE_NAME_MAX: usize = 64;

fn shown(raw: &str) -> String {
    clean(raw).chars().take(NOTICE_NAME_MAX).collect()
}

impl Dropped {
    /// The one-line chat notice. Names come from a server, so they are
    /// sanitized and capped.
    pub fn notice(&self) -> String {
        let (server, raw) = (shown(&self.server), shown(&self.raw));
        match &self.kept {
            Some((kept_server, kept_raw)) => format!(
                "mcp: {server}: {raw} dropped, same name as {}/{}",
                shown(kept_server),
                shown(kept_raw)
            ),
            None => format!("mcp: {server}: {raw} dropped, no usable name"),
        }
    }
}

/// The tools of every resolved server under their qualified names.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Qualified {
    pub tools: Vec<QualifiedTool>,
    pub dropped: Vec<Dropped>,
}

impl Qualified {
    /// The tools in the form the registry takes.
    pub fn external(&self) -> Vec<ExternalTool> {
        self.tools.iter().map(|t| t.tool.clone()).collect()
    }

    /// The server and raw tool name behind a qualified name.
    pub fn raw_of(&self, qualified: &str) -> Option<(&str, &str)> {
        self.tools
            .iter()
            .find(|t| t.tool.name == qualified)
            .map(|t| (t.server.as_str(), t.raw.as_str()))
    }
}

/// Qualifies the tools of each `(server, tools)` pair. Collisions are checked
/// across all of them: the first tool keeps a qualified name and a later one
/// is dropped and reported, as is a tool whose name cannot be qualified.
pub fn qualify(servers: &[(&str, &[ExternalTool])]) -> Qualified {
    let mut out = Qualified::default();
    for (server, tools) in servers {
        for tool in *tools {
            let dropped = |kept| Dropped {
                server: (*server).to_owned(),
                raw: tool.name.clone(),
                kept,
            };
            let Some(name) = qualified_name(server, &tool.name) else {
                out.dropped.push(dropped(None));
                continue;
            };
            if let Some(first) = out.tools.iter().find(|t| t.tool.name == name) {
                let kept = Some((first.server.clone(), first.raw.clone()));
                out.dropped.push(dropped(kept));
                continue;
            }
            out.tools.push(QualifiedTool {
                server: (*server).to_owned(),
                raw: tool.name.clone(),
                tool: ExternalTool {
                    name,
                    ..tool.clone()
                },
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

    // Spec: enabled false; design D16: a project entry decides for its root.
    #[test]
    fn a_disabled_entry_is_not_active() {
        let mut off = server("a", "ga", None);
        off.enabled = false;
        let list = vec![off, server("b", "gb", None)];
        assert_eq!(active(&list, None), [("b", "gb")]);
    }

    #[test]
    fn a_disabled_project_entry_removes_the_global_one_of_that_name_for_its_root_only() {
        let mut off = server("a", "project", Some("/p"));
        off.enabled = false;
        let list = vec![server("a", "global", None), off, server("b", "gb", None)];
        assert_eq!(active(&list, Some("/p")), [("b", "gb")]);
        assert_eq!(active(&list, Some("/q")), [("a", "global"), ("b", "gb")]);
        assert_eq!(active(&list, None), [("a", "global"), ("b", "gb")]);
    }

    #[test]
    fn a_disabled_project_entry_never_adds_a_server() {
        let mut off = server("only", "project", Some("/p"));
        off.enabled = false;
        assert_eq!(active(&[off], Some("/p")), []);
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

    fn ext(name: &str, description: &str) -> ExternalTool {
        ExternalTool {
            name: name.to_owned(),
            description: description.to_owned(),
            schema: json!({"type": "object"}),
        }
    }

    fn names(q: &Qualified) -> Vec<&str> {
        q.tools.iter().map(|t| t.tool.name.as_str()).collect()
    }

    // Spec (4a.2): a colliding external tool is qualified, never dropped.
    #[test]
    fn a_tool_named_like_a_built_in_is_exposed_under_its_qualified_name() {
        let tools = [
            ext("run_in_pane", "d"),
            ext("read_file", "d"),
            ext("search", "d"),
        ];
        let q = qualify(&[("fs", &tools)]);
        assert_eq!(
            names(&q),
            ["fs__run_in_pane", "fs__read_file", "fs__search"]
        );
        assert_eq!(q.dropped, Vec::new());
        let listed = registry(&q.external());
        let built_in = registry(&[]);
        assert_eq!(&listed[..4], &built_in[..], "the built-ins are unchanged");
        assert_eq!(listed.len(), 7);
    }

    // 4b routes by lookup, never by splitting on `__`.
    #[test]
    fn the_raw_name_and_server_are_kept_for_routing() {
        let tools = [ext("a.b", "one"), ext("deep__tool", "two")];
        let q = qualify(&[("fs", &tools), ("git", &[ext("log", "l")])]);
        assert_eq!(q.raw_of("fs__a_b"), Some(("fs", "a.b")));
        assert_eq!(q.raw_of("fs__deep__tool"), Some(("fs", "deep__tool")));
        assert_eq!(q.raw_of("git__log"), Some(("git", "log")));
        assert_eq!(q.raw_of("fs__deep"), None, "no splitting on the separator");
        assert_eq!(q.raw_of("read_file"), None);
    }

    // W1: tools of one server that qualify to the same name.
    #[test]
    fn same_server_collisions_keep_the_first_and_report_the_dropped_one() {
        let tools = [
            ext("a.b", "first"),
            ext("a_b", "second"),
            ext("Search", "s1"),
            ext("search", "s2"),
        ];
        let q = qualify(&[("fs", &tools)]);
        assert_eq!(names(&q), ["fs__a_b", "fs__search"]);
        assert_eq!(q.tools[0].tool.description, "first");
        let got: Vec<_> = q.dropped.iter().map(Dropped::notice).collect();
        assert_eq!(
            got,
            [
                "mcp: fs: a_b dropped, same name as fs/a.b",
                "mcp: fs: search dropped, same name as fs/Search",
            ]
        );
    }

    // W1: collisions are global across the resolved servers.
    #[test]
    fn a_collision_across_servers_is_detected_too() {
        let q = qualify(&[("a", &[ext("t", "x")]), ("A", &[ext("T", "y")])]);
        assert_eq!(names(&q), ["a__t"]);
        assert_eq!(q.dropped[0].notice(), "mcp: A: T dropped, same name as a/t");
    }

    #[test]
    fn a_tool_without_a_usable_name_is_dropped_with_a_notice() {
        let q = qualify(&[("fs", &[ext("", "none"), ext("ok", "o")])]);
        assert_eq!(names(&q), ["fs__ok"]);
        assert_eq!(q.dropped[0].notice(), "mcp: fs:  dropped, no usable name");
    }

    #[test]
    fn a_notice_is_sanitized_and_capped() {
        let hostile = format!("t\u{1b}[31m{}", "x".repeat(100));
        let notice = Dropped {
            server: "fs".to_owned(),
            raw: hostile.clone(),
            kept: Some(("fs".to_owned(), hostile)),
        }
        .notice();
        assert!(!notice.contains('\u{1b}'), "{notice:?}");
        assert!(notice.chars().count() < 200, "{notice:?}");
    }

    // Spec (agent-tools): a duplicate in the registry never reaches a model.
    #[test]
    fn the_registry_lists_a_qualified_name_once() {
        let tools = [ext("fs__search", "first"), ext("fs__search", "second")];
        let listed = registry(&tools);
        assert_eq!(listed.len(), 5);
        assert_eq!(listed[4].description, "first");
    }
}
