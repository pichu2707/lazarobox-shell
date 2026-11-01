use super::*;
use crate::core::ai::config::read_with_port_env;

fn read_text(text: &str) -> (UsageConfig, Vec<String>) {
    let table: Table = text.parse().unwrap();
    let mut problems = Vec::new();
    let ai = read_with_port_env(&table, &mut problems, None);
    (ai.usage().clone(), problems)
}

// Spec (configuration, Usage keys): defaults.
#[test]
fn no_table_means_everything_on_thresholds_off_and_subscription() {
    for text in ["", "[ai]\n", "[ai.usage]\n"] {
        let (usage, problems) = read_text(text);
        assert_eq!(usage, UsageConfig::default(), "{text:?}");
        assert!(problems.is_empty(), "{problems:?}");
    }
    let usage = UsageConfig::default();
    assert!(usage.line() && usage.statusline());
    assert_eq!(usage.turn_tokens(), None);
    assert_eq!(usage.day_tokens(), None);
    assert_eq!(usage.day_usd_micros(), None);
    for engine in ["claude", "codex", "gemini"] {
        assert_eq!(usage.billing(engine), Some(Billing::Subscription));
    }
}

#[test]
fn local_engines_have_no_billing_mode() {
    for engine in ["ollama", "lmstudio", "llamacpp", "vllm", "nope"] {
        assert_eq!(UsageConfig::default().billing(engine), None, "{engine}");
    }
}

#[test]
fn explicit_values_are_read() {
    let (usage, problems) = read_text(
        "[ai.usage]\nline = false\nturn_tokens = 4000\nday_usd = 5\n\
         [ai.usage.billing]\nclaude = \"per-token\"\n",
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert!(!usage.line() && usage.statusline());
    assert_eq!(usage.turn_tokens(), Some(4000));
    assert_eq!(usage.day_usd_micros(), Some(5_000_000));
    assert_eq!(usage.billing("claude"), Some(Billing::PerToken));
    assert_eq!(usage.billing("codex"), Some(Billing::Subscription));
    assert_eq!(usage.billing("gemini"), Some(Billing::Subscription));
}

#[test]
fn a_float_dollar_threshold_is_kept_to_the_micro() {
    let (usage, problems) = read_text("[ai.usage]\nday_usd = 2.5\n");
    assert!(problems.is_empty());
    assert_eq!(usage.day_usd_micros(), Some(2_500_000));
}

#[test]
fn wrong_types_and_ranges_fall_back_with_one_exact_problem_each() {
    let (usage, problems) = read_text(
        "[ai.usage]\nturn_tokens = 0\nday_tokens = -3\nday_usd = \"5\"\nline = \"yes\"\n\
         statusline = 1\n",
    );
    assert_eq!(usage, UsageConfig::default());
    assert_eq!(
        problems,
        [
            "ai.usage.line: expected true or false",
            "ai.usage.statusline: expected true or false",
            "ai.usage.turn_tokens: expected a whole number above 0",
            "ai.usage.day_tokens: expected a whole number above 0",
            "ai.usage.day_usd: expected a number above 0",
        ]
    );
}

#[test]
fn a_zero_or_negative_dollar_threshold_is_off() {
    for value in ["0", "0.0", "-1", "-0.5"] {
        let (usage, problems) = read_text(&format!("[ai.usage]\nday_usd = {value}\n"));
        assert_eq!(usage.day_usd_micros(), None, "{value}");
        assert_eq!(problems.len(), 1, "{value}");
    }
}

#[test]
fn a_fractional_whole_number_is_off() {
    let (usage, problems) = read_text("[ai.usage]\nturn_tokens = 1.5\n");
    assert_eq!(usage.turn_tokens(), None);
    assert_eq!(
        problems,
        ["ai.usage.turn_tokens: expected a whole number above 0"]
    );
}

#[test]
fn a_wrong_billing_value_falls_back_to_subscription() {
    let (usage, problems) = read_text(
        "[ai.usage.billing]\ncodex = \"Subscription\"\nclaude = 3\ngemini = \"per-token\"\n",
    );
    assert_eq!(usage.billing("codex"), Some(Billing::Subscription));
    assert_eq!(usage.billing("claude"), Some(Billing::Subscription));
    assert_eq!(usage.billing("gemini"), Some(Billing::PerToken));
    assert_eq!(
        problems,
        [
            "ai.usage.billing.claude: expected \"subscription\" or \"per-token\"",
            "ai.usage.billing.codex: expected \"subscription\" or \"per-token\"",
        ]
    );
}

#[test]
fn local_engines_and_unknown_keys_are_ignored() {
    let (usage, problems) =
        read_text("[ai.usage]\nfoo = 1\n[ai.usage.billing]\nollama = \"per-token\"\nfoo = \"x\"\n");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(usage.billing("ollama"), None);
    assert_eq!(usage, UsageConfig::default());
}

#[test]
fn a_usage_that_is_not_a_table_falls_back_to_defaults() {
    let (usage, problems) = read_text("[ai]\nusage = 3\n");
    assert_eq!(usage, UsageConfig::default());
    assert_eq!(problems, ["ai.usage: expected a table"]);
}

#[test]
fn a_billing_that_is_not_a_table_is_told() {
    let (usage, problems) = read_text("[ai.usage]\nbilling = 1\n");
    assert_eq!(usage, UsageConfig::default());
    assert_eq!(problems, ["ai.usage.billing: expected a table"]);
}

#[test]
fn usage_problems_come_after_the_notify_ones() {
    let (_, problems) = read_text("[ai]\nnotify = 3\n[ai.usage]\nline = 1\n");
    assert_eq!(problems.len(), 2);
    assert!(problems[0].starts_with("ai.notify"), "{problems:?}");
    assert!(problems[1].starts_with("ai.usage.line"), "{problems:?}");
}
