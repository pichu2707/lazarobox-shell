//! The `[ai]` section of `config.toml`: the saved engine and model ids.

use toml::{Table, Value};

/// The saved AI choice. `None` means unset (a missing key or `""`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AiConfig {
    pub engine: Option<String>,
    pub model: Option<String>,
}

/// Reads `[ai]`. A key of the wrong type falls back to unset on its own and
/// notes a problem; ids are kept verbatim, known or not.
pub fn read(table: &Table, problems: &mut Vec<String>) -> AiConfig {
    let section = match table.get("ai") {
        None => return AiConfig::default(),
        Some(Value::Table(section)) => section,
        Some(_) => {
            problems.push("ai: expected a table".to_owned());
            return AiConfig::default();
        }
    };
    let mut key = |name: &str| match section.get(name) {
        None => None,
        Some(Value::String(id)) => Some(id.clone()).filter(|id| !id.is_empty()),
        Some(_) => {
            problems.push(format!("ai.{name}: expected a string"));
            None
        }
    };
    let engine = key("engine");
    let model = key("model");
    AiConfig { engine, model }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_text(text: &str) -> (AiConfig, Vec<String>) {
        let table: Table = text.parse().unwrap();
        let mut problems = Vec::new();
        let ai = read(&table, &mut problems);
        (ai, problems)
    }

    fn ai(engine: Option<&str>, model: Option<&str>) -> AiConfig {
        AiConfig {
            engine: engine.map(str::to_owned),
            model: model.map(str::to_owned),
        }
    }

    // Spec: AI keys parsed.
    #[test]
    fn engine_and_model_are_read() {
        assert_eq!(
            read_text("[ai]\nengine = \"ollama\"\nmodel = \"llama3:8b\"\n"),
            (ai(Some("ollama"), Some("llama3:8b")), vec![])
        );
    }

    #[test]
    fn missing_section_and_keys_are_unset() {
        assert_eq!(read_text(""), (AiConfig::default(), vec![]));
        assert_eq!(read_text("[ai]\n"), (AiConfig::default(), vec![]));
        assert_eq!(
            read_text("[ai]\nmodel = \"m\"\n"),
            (ai(None, Some("m")), vec![])
        );
    }

    // Spec: Unknown engine id kept.
    #[test]
    fn an_unknown_engine_id_is_kept_verbatim() {
        assert_eq!(
            read_text("[ai]\nengine = \"future-engine\"\n"),
            (ai(Some("future-engine"), None), vec![])
        );
    }

    // Spec: Empty string is unset.
    #[test]
    fn an_empty_string_is_unset() {
        assert_eq!(
            read_text("[ai]\nengine = \"\"\nmodel = \"\"\n"),
            (AiConfig::default(), vec![])
        );
    }

    // Spec: Invalid AI types fall back per key.
    #[test]
    fn a_non_string_key_falls_back_alone() {
        assert_eq!(
            read_text("[ai]\nengine = 3\nmodel = \"m\"\n"),
            (
                ai(None, Some("m")),
                vec!["ai.engine: expected a string".to_owned()]
            )
        );
        assert_eq!(
            read_text("[ai]\nengine = \"claude\"\nmodel = true\n"),
            (
                ai(Some("claude"), None),
                vec!["ai.model: expected a string".to_owned()]
            )
        );
    }

    // Spec: Non-table ai.
    #[test]
    fn a_non_table_ai_is_one_problem() {
        assert_eq!(
            read_text("ai = 3\n"),
            (AiConfig::default(), vec!["ai: expected a table".to_owned()])
        );
    }
}
