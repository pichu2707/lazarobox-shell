use super::*;

const PAGE: &str = include_str!("../../../../tests/fixtures/ollama/library-popular.html");

const GB: u64 = 1_000_000_000;

fn card<'a>(cards: &'a [Card], name: &str) -> &'a Card {
    cards
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no card {name}"))
}

// Spec (ollama-download, Read the real page).
#[test]
fn the_real_page_reads_into_cards() {
    let cards = parse_library(PAGE);
    let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "llama3.1",
            "deepseek-r1",
            "nomic-embed-text",
            "llama3.2",
            "gemma3",
            "gemma3n",
            "smollm2",
            "mixtral",
            "minimax-m2.7",
        ],
        "page order, the unsafe `../evil` card skipped"
    );
    let llama = card(&cards, "llama3.2");
    assert_eq!(
        llama.description,
        "Meta's Llama 3.2 goes small with 1B and 3B models."
    );
    assert_eq!(llama.capabilities, ["tools"]);
    assert_eq!(llama.sizes, ["1b", "3b"]);
    assert_eq!(llama.pulls, "86.3M");
    assert_eq!(
        card(&cards, "gemma3").sizes,
        ["270m", "1b", "4b", "12b", "27b"]
    );
    assert_eq!(card(&cards, "mixtral").sizes, ["8x7b", "8x22b"]);
    assert_eq!(card(&cards, "nomic-embed-text").capabilities, ["embedding"]);
}

#[test]
fn a_page_with_no_card_reads_as_none() {
    assert_eq!(parse_library(""), Vec::new());
    assert_eq!(
        parse_library("<html><body>Just a moment…</body></html>"),
        Vec::new()
    );
}

#[test]
fn size_tags_give_their_parameters() {
    assert_eq!(size_params("8b"), Some(8 * GB));
    assert_eq!(size_params("1.7b"), Some(1_700_000_000));
    assert_eq!(size_params("270m"), Some(270_000_000));
    assert_eq!(size_params("8x7b"), Some(56 * GB));
    // "Effective" parameters say little about the download: not estimated.
    assert_eq!(size_params("e2b"), None);
    assert_eq!(size_params("latest"), None);
    assert_eq!(size_params("b"), None);
}

#[test]
fn names_and_tags_are_checked_before_any_request() {
    for ok in [
        "llama3.2",
        "qwen2.5-coder",
        "gpt-oss",
        "3b",
        "8x7b",
        "q4_K_M",
    ] {
        assert!(is_name(ok), "{ok}");
    }
    for bad in [
        "",
        "../evil",
        ".x",
        "-x",
        "a b",
        "a/b",
        "a:b",
        &"x".repeat(65),
    ] {
        assert!(!is_name(bad), "{bad}");
    }
}

// Spec (ollama-download, Only what fits).
#[test]
fn the_popular_list_keeps_the_sizes_that_fit_this_pc() {
    let cards = parse_library(PAGE);
    let picks = popular(cards, Some(8 * GB));
    let rows: Vec<(&str, Vec<&str>)> = picks
        .iter()
        .map(|p| {
            (
                p.card.name.as_str(),
                p.fitting.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("llama3.1", vec!["8b"]),
            ("deepseek-r1", vec!["1.5b", "7b", "8b"]),
            ("llama3.2", vec!["1b", "3b"]),
            ("gemma3", vec!["270m", "1b", "4b"]),
            ("smollm2", vec!["135m", "360m", "1.7b"]),
        ],
        "embedding, cloud-only, effective-size-only and too-big models left out"
    );
}

#[test]
fn with_the_memory_unknown_no_card_is_left_out_for_its_size() {
    let picks = popular(parse_library(PAGE), None);
    let names: Vec<&str> = picks.iter().map(|p| p.card.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "llama3.1",
            "deepseek-r1",
            "llama3.2",
            "gemma3",
            "gemma3n",
            "smollm2",
            "mixtral"
        ]
    );
    assert_eq!(picks[4].fitting, ["e2b", "e4b"]);
}

#[test]
fn the_popular_list_holds_at_most_twenty() {
    let one = card(&parse_library(PAGE), "llama3.2").clone();
    let cards = (0..30)
        .map(|i| Card {
            name: format!("m{i}"),
            ..one.clone()
        })
        .collect();
    assert_eq!(popular(cards, Some(8 * GB)).len(), POPULAR_LIMIT);
}

#[test]
fn the_requests_are_built_from_checked_names() {
    assert_eq!(library_url(), "https://ollama.com/library?sort=popular");
    assert_eq!(
        manifest_url("llama3.2", "3b"),
        "https://registry.ollama.ai/v2/library/llama3.2/manifests/3b"
    );
    let digest = format!("sha256:{}", "a".repeat(64));
    assert_eq!(
        blob_url("llama3.2", &digest),
        format!("https://registry.ollama.ai/v2/library/llama3.2/blobs/{digest}")
    );
    assert_eq!(
        page_url("llama3.2", "3b"),
        "https://ollama.com/library/llama3.2:3b"
    );
}

// Spec (ollama-download, Exact size from the manifest).
#[test]
fn a_manifest_gives_the_exact_size_and_the_license_layers() {
    let digest = |c: char| format!("sha256:{}", c.to_string().repeat(64));
    let body = format!(
        r#"{{"schemaVersion":2,"config":{{"digest":"{}","size":487}},"layers":[
            {{"mediaType":"application/vnd.ollama.image.model","digest":"{}","size":2019377376}},
            {{"mediaType":"application/vnd.ollama.image.template","digest":"{}","size":1429}},
            {{"mediaType":"application/vnd.ollama.image.license","digest":"{}","size":7711}},
            {{"mediaType":"application/vnd.ollama.image.license","digest":"{}","size":6016}},
            {{"mediaType":"application/vnd.ollama.image.params","digest":"{}","size":96}}
        ]}}"#,
        digest('0'),
        digest('1'),
        digest('2'),
        digest('3'),
        digest('4'),
        digest('5')
    );
    assert_eq!(
        parse_manifest(&body),
        Ok(Manifest {
            size: 2_019_392_628,
            licenses: vec![digest('3'), digest('4')],
        })
    );
    assert_eq!(parse_manifest("not json"), Err(UNREADABLE.to_owned()));
    // A license digest that is not sha256 hex is never asked for.
    let odd = body.replace(&digest('3'), "sha256:../../x");
    assert_eq!(parse_manifest(&odd).unwrap().licenses, [digest('4')]);
}

// Spec (ollama-download, License shown before the pull).
#[test]
fn a_license_title_is_its_first_line_cleaned_and_cut() {
    assert_eq!(
        license_title("\n\n  LLAMA 3.2 COMMUNITY LICENSE AGREEMENT\nLlama 3.2 Version…"),
        "LLAMA 3.2 COMMUNITY LICENSE AGREEMENT"
    );
    assert_eq!(
        license_title("Apache\u{1b}[31m 2.0\u{202e}"),
        "Apache?[31m 2.0?"
    );
    assert_eq!(license_title(&"x".repeat(500)).chars().count(), 120);
    assert_eq!(license_title("  \n \n"), NOT_STATED);
}

// Spec (ollama-download, Unreadable page falls back).
#[test]
fn the_fallback_is_the_curated_catalog_as_cards() {
    let cards = fallback_cards();
    assert_eq!(cards.len(), 10);
    let qwen = card(&cards, "qwen3");
    assert_eq!(
        (qwen.sizes.as_slice(), qwen.pulls.as_str()),
        (["8b".to_owned()].as_slice(), "")
    );
    let picks = popular(cards, Some(8 * GB));
    let names: Vec<&str> = picks.iter().map(|p| p.card.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "llama3.2",
            "llama3.1",
            "qwen3",
            "qwen2.5-coder",
            "gemma3",
            "mistral",
            "deepseek-r1"
        ],
        "14b, 20b and 30b do not fit 8 GB"
    );
}
