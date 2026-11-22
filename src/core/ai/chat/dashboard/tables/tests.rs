use super::*;
use crate::core::ai::chat::usage_view::Billing;

fn used(engine: &str, model: Option<&str>, input: u64, output: u64) -> UsageRow {
    UsageRow {
        engine: engine.to_owned(),
        model: model.map(str::to_owned),
        turns: 2,
        records: 2,
        input: Some(input),
        output: Some(output),
        total: Some(input + output),
        ..UsageRow::default()
    }
}

fn data(rows: Vec<UsageRow>) -> UsageData {
    UsageData {
        rows,
        ..UsageData::default()
    }
}

fn billed() -> Billed {
    vec![
        ("claude".to_owned(), Billing::Subscription),
        ("codex".to_owned(), Billing::PerToken),
    ]
}

fn built(rows: Vec<UsageRow>) -> Vec<ModelRow> {
    models(&data(rows), Range::All, &billed())
}

/// The table lines as one string.
fn shown(models: &[ModelRow], width: usize) -> String {
    let t = table(models, width);
    let rows = t.rows.iter().map(|(_, line)| line.clone());
    std::iter::once(t.head)
        .chain(rows)
        .chain(t.notes)
        .collect::<Vec<_>>()
        .join("\n")
}

fn priced() -> Vec<ModelRow> {
    let mut claude = used("claude", Some("sonnet"), 2_000, 1_000);
    claude.cost_micros = Some(500_000);
    claude.costed = 2;
    built(vec![
        claude,
        used("ollama", Some("llama3:8b"), 4_000, 1_000),
    ])
}

#[test]
fn rows_are_ordered_by_total_tokens_with_dollars_for_billed_engines_only() {
    let text = shown(&priced(), 100);
    let (ollama, claude) = (
        text.find("llama3:8b").expect("ollama row"),
        text.find("sonnet").expect("claude row"),
    );
    assert!(ollama < claude, "{text}");
    let line = |name: &str| text.lines().find(|l| l.contains(name)).unwrap().to_owned();
    assert!(!line("llama3").contains('$'), "{text}");
    assert!(
        line("sonnet").contains("$0.5000 list-price equivalent"),
        "{text}"
    );
}

#[test]
fn a_per_token_engine_shows_the_plain_figure_and_a_local_one_never_dollars() {
    let mut codex = used("codex", Some("gpt-5"), 10, 10);
    codex.cost_micros = Some(4_900);
    codex.costed = 2;
    let mut local = used("ollama", Some("llama3"), 10, 10);
    local.cost_micros = Some(9_000_000);
    let text = shown(&built(vec![codex, local]), 100);
    let line = |name: &str| text.lines().find(|l| l.contains(name)).unwrap().to_owned();
    assert!(line("gpt-5").contains("$0.0049") && !line("gpt-5").contains("list-price"));
    assert!(!line("llama3").contains('$'), "{text}");
}

#[test]
fn a_billed_engine_with_no_figure_says_n_a_and_a_floor_is_marked() {
    let none = built(vec![used("claude", Some("sonnet"), 1, 1)]);
    assert!(shown(&none, 100).contains("n/a"));
    let mut part = used("codex", Some("gpt-5"), 1, 1);
    part.cost_micros = Some(4_900);
    part.costed = 1;
    assert!(shown(&built(vec![part]), 100).contains("~$0.0049"));
}

#[test]
fn a_missing_figure_is_n_a_and_an_incomplete_one_is_approximate() {
    let mut gap = used("ollama", Some("a"), 1_500, 0);
    gap.output = None;
    gap.partial = true;
    let text = shown(&built(vec![gap]), 100);
    assert!(text.contains("~1.5k") && text.contains("n/a"), "{text}");
}

#[test]
fn a_null_model_reads_unknown_and_mixed_stays_mixed() {
    let rows = built(vec![
        used("claude", None, 5, 5),
        used("claude", Some("mixed"), 4, 4),
    ]);
    let text = shown(&rows, 100);
    assert!(text.contains("unknown") && text.contains("mixed"), "{text}");
}

#[test]
fn every_row_starts_with_its_glyph_and_carries_the_provider_label() {
    for (engine, model, provider) in [
        ("claude", "sonnet", Provider::Anthropic),
        ("gemini", "flash", Provider::Google),
        ("codex", "gpt-5", Provider::OpenAi),
        ("ollama", "llama3", Provider::Local),
        ("foo", "bar", Provider::Other),
    ] {
        let rows = built(vec![used(engine, Some(model), 1, 1)]);
        let (p, line) = &table(&rows, 100).rows[0];
        assert_eq!(*p, provider);
        assert!(line.starts_with(provider.glyph()), "{line}");
        assert!(
            line.contains(provider.label()) && line.contains(model),
            "{line}"
        );
    }
}

#[test]
fn the_range_leaves_older_models_out() {
    let mut old = used("ollama", Some("old"), 1, 1);
    old.days_ago = 9;
    let rows = vec![used("claude", Some("new"), 1, 1), old];
    assert_eq!(models(&data(rows.clone()), Range::Week, &billed()).len(), 1);
    assert_eq!(models(&data(rows), Range::All, &billed()).len(), 2);
}

#[test]
fn rows_of_one_pair_on_different_days_add_up() {
    let mut earlier = used("ollama", Some("a"), 100, 50);
    earlier.days_ago = 3;
    let rows = built(vec![used("ollama", Some("a"), 1_000, 500), earlier]);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].input, rows[0].output), (Some(1_100), Some(550)));
    assert_eq!(rows[0].turns, 4);
}

#[test]
fn the_columns_drop_by_width_at_59_60_and_79_80() {
    let rows = priced();
    let cols = |w| {
        let t = shown(&rows, w);
        (
            t.contains("turns"),
            t.contains('$'),
            t.contains("$0.5000 list-price"),
        )
    };
    assert_eq!(cols(40), (false, false, false));
    assert_eq!(cols(59), (false, false, false));
    assert_eq!(cols(60), (false, true, false));
    assert_eq!(cols(79), (false, true, false));
    assert_eq!(cols(80), (true, true, true));
}

#[test]
fn a_narrow_dollar_marks_the_list_price_figure_and_explains_the_mark() {
    let text = shown(&priced(), 70);
    assert!(text.contains("$0.5000*"), "{text}");
    assert!(text.contains("* list-price equivalent"), "{text}");
    assert!(!shown(&priced(), 100).contains("* list-price"));
}

#[test]
fn no_line_is_wider_than_the_table_at_any_width() {
    let long = "x".repeat(60);
    let wide = "模型名称模型名称模型名称模型名称模型名称";
    let mut rows = vec![
        used("claude", Some(&long), 123_456_789, 987_654_321),
        used("ollama", Some(wide), 5, 5),
    ];
    rows[0].cost_micros = Some(12_345_678);
    rows[0].costed = 2;
    let rows = built(rows);
    for width in 40..=120 {
        let t = table(&rows, width);
        for line in std::iter::once(&t.head).chain(t.rows.iter().map(|(_, l)| l)) {
            assert!(line.width() <= width, "{width}: {line}");
        }
    }
    assert!(shown(&rows, 40).contains('…'));
}

#[test]
fn many_models_are_cut_with_a_visible_count() {
    let rows = built(
        (0..MODELS_MAX + 3)
            .map(|n| used("ollama", Some(&format!("m{n:02}")), 100 - n as u64, 1))
            .collect(),
    );
    let t = table(&rows, 100);
    assert_eq!(t.rows.len(), MODELS_MAX);
    assert_eq!(t.notes, ["+3 more models"]);
    assert_eq!(table(&rows[..MODELS_MAX], 100).notes, Vec::<String>::new());
    // The hidden models still count in the shares.
    let all: u64 = shares(&rows).iter().map(|s| s.tokens).sum();
    assert_eq!(all, rows.iter().filter_map(|r| r.total).sum::<u64>());
}

#[test]
fn the_largest_remainder_makes_33_4_33_3_33_3_into_34_33_33() {
    assert_eq!(apportion(&[334, 333, 333], 100), [34, 33, 33]);
    // A tie goes to the earlier entry.
    assert_eq!(apportion(&[1, 1, 1], 100), [34, 33, 33]);
}

#[test]
fn the_segments_always_fill_the_bar_and_a_small_provider_keeps_a_cell() {
    for width in 40..=120u64 {
        for values in [
            vec![1, 1_000_000_000],
            vec![7, 5, 3, 1, 1],
            vec![u64::MAX, u64::MAX, 1],
            vec![3],
        ] {
            let cells = apportion(&values, width);
            assert_eq!(cells.iter().sum::<u64>(), width, "{values:?}");
            assert!(
                cells.iter().all(|c| *c >= 1),
                "{width} {values:?} {cells:?}"
            );
        }
    }
    assert_eq!(apportion(&[0, 5], 10), [0, 10]);
    assert_eq!(apportion(&[0, 0], 10), [0, 0]);
}

#[test]
fn shares_sum_to_100_and_a_single_engine_is_one_full_segment() {
    let one = shares(&built(vec![used("ollama", Some("a"), 7, 3)]));
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].percent, 100);
    assert_eq!(segments(&one, 60), [(Provider::Local, 60)]);
    let many = shares(&priced());
    assert_eq!(many.iter().map(|s| s.percent).sum::<u64>(), 100);
    assert_eq!(many[0].provider, Provider::Local);
}

#[test]
fn models_of_one_provider_share_one_segment() {
    let rows = built(vec![
        used("claude", Some("a"), 10, 0),
        used("claude", Some("b"), 10, 0),
        used("foo", Some("c"), 20, 0),
    ]);
    let s = shares(&rows);
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].percent, 50);
}

#[test]
fn zero_or_unknown_tokens_make_no_segment() {
    let mut gap = used("ollama", Some("a"), 0, 0);
    gap.total = None;
    assert!(shares(&built(vec![gap])).is_empty());
    assert!(shares(&built(vec![used("ollama", Some("a"), 0, 0)])).is_empty());
    assert!(shares(&[]).is_empty());
}

#[test]
fn the_legend_names_every_provider_with_its_percent_and_never_clips() {
    let rows = built(vec![
        used("claude", Some("a"), 50, 0),
        used("gemini", Some("b"), 20, 0),
        used("codex", Some("c"), 15, 0),
        used("ollama", Some("d"), 10, 0),
        used("foo", Some("e"), 5, 0),
    ]);
    let s = shares(&rows);
    for width in 40..=120 {
        let lines = share_legend(&s, width);
        for line in &lines {
            assert!(line.width() <= width, "{width}: {line}");
        }
        let all = lines.join(" ");
        for sh in &s {
            let item = format!(
                "{} {} {}%",
                sh.provider.glyph(),
                sh.provider.label(),
                sh.percent
            );
            assert!(all.contains(&item), "{width}: {all}");
        }
    }
    assert_eq!(share_legend(&s, 120).len(), 1);
    assert!(share_legend(&s, 40).len() > 1);
}

#[test]
fn the_blocks_take_no_rows_without_models_and_grow_with_them() {
    assert_eq!(blocks_rows(&[], &[], 100), 0);
    let r = priced();
    let s = shares(&r);
    assert!(blocks_rows(&r, &s, 100) >= 9);
    assert!(blocks_rows(&r, &s, 40) >= blocks_rows(&r, &s, 100));
}
