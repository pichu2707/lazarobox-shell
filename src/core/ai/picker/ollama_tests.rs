//! The `download from Ollama…` row of the ollama model level and its
//! screens (config-menu, Ollama download row and Ollama screens;
//! ollama-download, Exact sizes and license before a pull).

use std::path::PathBuf;

use super::machine_tests::state;
use super::*;
use crate::core::ai::ollama_lib::{Answer, Ask, Card, Licenses, Manifest, Pull};

const GB: u64 = 1_000_000_000;
const RAM: Option<u64> = Some(8 * GB);
const LLAMA: &str = "LLAMA 3.2 COMMUNITY LICENSE AGREEMENT";

fn ollama(status: Status) -> Detection {
    Detection {
        engines: vec![
            state("ollama", status, &["qwen3:4b"]),
            state("llamacpp", Status::NotRunning, &[]),
        ],
        local_models: vec![PathBuf::from("/m/b.gguf")],
        ..Default::default()
    }
}

fn card(name: &str, sizes: &[&str], pulls: &str) -> Card {
    Card {
        name: name.to_owned(),
        description: String::new(),
        capabilities: Vec::new(),
        sizes: sizes.iter().map(|s| (*s).to_owned()).collect(),
        pulls: pulls.to_owned(),
    }
}

fn labels(rows: &[PickRow]) -> Vec<String> {
    rows.iter().map(|r| r.label.clone()).collect()
}

/// A picker on `engine`'s model level, the cursor on the row keyed `key`.
fn at_row(engine: &str, key: &PickKey, d: &Detection) -> Picker {
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Chat), &draft, Some(d));
    p.level = Level::Models {
        engine: engine.to_owned(),
    };
    p.cursor = p
        .rows(&draft, Some(d))
        .iter()
        .position(|r| &r.key == key)
        .expect("the entry row");
    p
}

/// Enter on the Ollama row, answered with `cards`: the Popular screen.
fn at_popular(cards: Vec<Card>, fallback: bool, d: &Detection) -> Picker {
    let mut p = at_row("ollama", &PickKey::OllamaDownload, d);
    assert_eq!(
        p.enter(&AiConfig::default(), Some(d)),
        Outcome::OllamaAsk(Ask::Popular)
    );
    let reply = Answer::Popular {
        cards,
        fallback,
        ram: RAM,
    };
    assert_eq!(p.ollama_reply(&Ask::Popular, Ok(reply)), None);
    p
}

fn llama_manifests() -> Vec<(String, Option<Manifest>)> {
    vec![
        (
            "1b".to_owned(),
            Some(Manifest {
                size: 1_321_098_329,
                licenses: vec!["sha256:a".to_owned()],
            }),
        ),
        (
            "3b".to_owned(),
            Some(Manifest {
                size: 2_019_377_376 + 1429 + 7711 + 6016 + 96,
                licenses: vec!["sha256:a".to_owned()],
            }),
        ),
        ("70b".to_owned(), None),
    ]
}

/// Enter on `llama3.2`, then on its `3b`: the Confirm screen with `titles`.
fn at_confirm(titles: Vec<String>, d: &Detection) -> Picker {
    let draft = AiConfig::default();
    let mut p = at_popular(vec![card("llama3.2", &["1b", "3b"], "86.3M")], false, d);
    let sizes = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned(), "3b".to_owned()],
    };
    assert_eq!(p.enter(&draft, Some(d)), Outcome::OllamaAsk(sizes.clone()));
    p.ollama_reply(
        &sizes,
        Ok(Answer::Sizes {
            manifests: llama_manifests(),
            ram: RAM,
        }),
    );
    p.step(true, &draft, Some(d));
    let licenses = Ask::Licenses {
        model: "llama3.2".to_owned(),
        tag: "3b".to_owned(),
        digests: Some(vec!["sha256:a".to_owned()]),
    };
    assert_eq!(
        p.enter(&draft, Some(d)),
        Outcome::OllamaAsk(licenses.clone())
    );
    p.ollama_reply(
        &licenses,
        Ok(Answer::Licenses(Box::new(Licenses {
            size: None,
            titles,
            folder: Some(PathBuf::from("/m")),
            free: Some(50 * GB),
        }))),
    );
    p
}

// Scenario: Row only with a running server.
#[test]
fn the_row_is_last_only_while_the_server_answers() {
    let rows = |status| {
        model_rows(
            "ollama",
            Target::Task(Task::Chat),
            &AiConfig::default(),
            Some(&ollama(status)),
        )
    };
    let available = rows(Status::Available);
    let last = available.last().expect("rows");
    assert_eq!(
        (&last.key, last.label.as_str(), &last.mark, last.enabled),
        (&PickKey::OllamaDownload, OLLAMA_ROW, &Mark::Plain, true)
    );
    let stopped = rows(Status::NotRunning);
    assert!(
        !stopped.iter().any(|r| r.key == PickKey::OllamaDownload),
        "{stopped:?}"
    );
    // Not on another engine's level.
    let llamacpp = model_rows(
        "llamacpp",
        Target::Task(Task::Chat),
        &AiConfig::default(),
        Some(&ollama(Status::Available)),
    );
    assert!(!llamacpp.iter().any(|r| r.key == PickKey::OllamaDownload));
}

#[test]
fn popular_rows_show_the_pulls_and_only_the_sizes_that_fit() {
    let d = ollama(Status::Available);
    let p = at_popular(
        vec![
            card("llama3.2", &["1b", "3b"], "86.3M"),
            card("llama3.1", &["8b", "70b", "405b"], "110M"),
            card("big", &["405b"], "1M"),
        ],
        false,
        &d,
    );
    let o = p.ollama().expect("the Ollama screens");
    assert_eq!(o.title(), "Ollama: most popular for this PC");
    assert_eq!(o.hint(), "j/k move · Enter choose · Esc back");
    assert_eq!(
        labels(&o.rows()),
        ["llama3.2  86.3M pulls  1b 3b", "llama3.1  110M pulls  8b"]
    );
    assert!(o.rows().iter().all(|r| r.enabled));
}

#[test]
fn nothing_that_fits_reads_the_info_row() {
    let d = ollama(Status::Available);
    let p = at_popular(vec![card("big", &["405b"], "1M")], false, &d);
    let rows = p.ollama().unwrap().rows();
    assert_eq!(labels(&rows), [NONE_FITS]);
    assert!(!rows[0].enabled);
}

// Scenario: Unreadable page falls back.
#[test]
fn the_fallback_says_so_before_the_built_in_list() {
    let d = ollama(Status::Available);
    let mut p = at_popular(crate::core::ai::ollama_lib::fallback_cards(), true, &d);
    let rows = p.ollama().unwrap().rows();
    assert_eq!(
        rows[0].label,
        "could not read the Ollama library; showing the built-in list"
    );
    assert!(!rows[0].enabled);
    assert_eq!(rows[1].label, "llama3.2  3b");
    // The cursor starts on the first model, and Enter asks for its sizes.
    assert_eq!(p.cursor, 1);
    assert_eq!(
        p.enter(&AiConfig::default(), Some(&d)),
        Outcome::OllamaAsk(Ask::Sizes {
            model: "llama3.2".to_owned(),
            tags: vec!["3b".to_owned()],
        })
    );
}

// Scenario: Exact size from the manifest.
#[test]
fn sizes_show_the_exact_size_or_unknown_and_mark_what_is_too_big() {
    let d = ollama(Status::Available);
    let mut p = at_popular(vec![card("llama3.2", &["1b", "3b", "70b"], "")], false, &d);
    let sizes = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned(), "3b".to_owned(), "70b".to_owned()],
    };
    assert_eq!(
        p.enter(&AiConfig::default(), Some(&d)),
        Outcome::OllamaAsk(sizes.clone())
    );
    assert_eq!(p.ollama().unwrap().hint(), "asking ollama.com…");
    let mut manifests = llama_manifests();
    manifests[2].1 = Some(Manifest {
        size: 42 * GB,
        licenses: Vec::new(),
    });
    manifests.push(("8b".to_owned(), None));
    p.ollama_reply(
        &sizes,
        Ok(Answer::Sizes {
            manifests,
            ram: RAM,
        }),
    );
    let o = p.ollama().unwrap();
    assert_eq!(o.title(), "Ollama: llama3.2");
    assert_eq!(
        labels(&o.rows()),
        [
            "llama3.2:1b  1.3 GB",
            "llama3.2:3b  2.0 GB",
            "llama3.2:70b  42.0 GB · too big for this PC",
            "llama3.2:8b  size unknown",
        ]
    );
    // Every size stays choosable.
    assert!(o.rows().iter().all(|r| r.enabled));
}

#[test]
fn an_unknown_size_asks_for_its_manifest_again_with_its_licenses() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_popular(vec![card("llama3.2", &["1b", "3b", "70b"], "")], false, &d);
    let sizes = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned(), "3b".to_owned(), "70b".to_owned()],
    };
    p.enter(&draft, Some(&d));
    p.ollama_reply(
        &sizes,
        Ok(Answer::Sizes {
            manifests: llama_manifests(),
            ram: RAM,
        }),
    );
    p.step(true, &draft, Some(&d));
    p.step(true, &draft, Some(&d));
    let ask = Ask::Licenses {
        model: "llama3.2".to_owned(),
        tag: "70b".to_owned(),
        digests: None,
    };
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::OllamaAsk(ask.clone()));
    p.ollama_reply(
        &ask,
        Ok(Answer::Licenses(Box::new(Licenses {
            size: Some(43 * GB),
            titles: Vec::new(),
            folder: None,
            free: None,
        }))),
    );
    assert_eq!(
        labels(&p.ollama().unwrap().rows()),
        [
            "download llama3.2:70b (43.0 GB)",
            "license: not stated",
            "read it: https://ollama.com/library/llama3.2:70b",
            "to: the Ollama server's models folder",
            "free: unknown",
        ]
    );
}

// Scenario: License shown before the pull.
#[test]
fn the_confirm_screen_shows_the_license_and_where_it_goes() {
    let d = ollama(Status::Available);
    let p = at_confirm(vec![LLAMA.to_owned()], &d);
    let o = p.ollama().unwrap();
    assert_eq!(o.title(), "Ollama: download");
    assert_eq!(o.hint(), "y download · n back");
    let rows = o.rows();
    assert_eq!(
        labels(&rows),
        [
            "download llama3.2:3b (2.0 GB)",
            "license: LLAMA 3.2 COMMUNITY LICENSE AGREEMENT",
            "read it: https://ollama.com/library/llama3.2:3b",
            "to: /m",
            "free: 50.0 GB",
        ]
    );
    assert!(rows.iter().all(|r| !r.enabled));
}

#[test]
fn nothing_is_pulled_before_y_and_y_pulls_once() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_confirm(vec![LLAMA.to_owned()], &d);
    // Enter on the Confirm screen does nothing.
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(
        p.ollama_pull(&draft, Some(&d)),
        Outcome::Pull(Box::new(Pull {
            model: "llama3.2".to_owned(),
            tag: "3b".to_owned(),
            size: Some(2_019_392_628),
            license: LLAMA.to_owned(),
        }))
    );
    // Back on the ollama model level, on the entry row.
    assert_eq!(
        p.level,
        Level::Models {
            engine: "ollama".to_owned()
        }
    );
    assert_eq!(p.key(&draft, Some(&d)), Some(PickKey::OllamaDownload));
    assert_eq!(p.ollama_pull(&draft, Some(&d)), Outcome::Stay);
}

#[test]
fn n_or_esc_on_the_confirm_screen_goes_back_to_the_sizes() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_confirm(vec![LLAMA.to_owned()], &d);
    p.back(&draft, Some(&d));
    assert_eq!(p.ollama().unwrap().title(), "Ollama: llama3.2");
    assert_eq!(p.cursor, 1, "the cursor is back on llama3.2:3b");
    p.back(&draft, Some(&d));
    assert_eq!(
        p.ollama().unwrap().title(),
        "Ollama: most popular for this PC"
    );
    p.back(&draft, Some(&d));
    assert_eq!(p.key(&draft, Some(&d)), Some(PickKey::OllamaDownload));
}

// Scenario: Late reply dropped.
#[test]
fn esc_during_a_request_drops_its_late_reply_and_the_screen_stays() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_popular(vec![card("llama3.2", &["1b", "3b"], "86.3M")], false, &d);
    let sizes = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned(), "3b".to_owned()],
    };
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::OllamaAsk(sizes.clone()));
    p.back(&draft, Some(&d));
    let late = Ok(Answer::Sizes {
        manifests: llama_manifests(),
        ram: RAM,
    });
    assert_eq!(p.ollama_reply(&sizes, late), None);
    let o = p.ollama().unwrap();
    assert_eq!(o.title(), "Ollama: most popular for this PC");
    assert_eq!(o.asking(), None);
    // Esc while the Popular list is asked for leaves the screens, and
    // its reply is dropped too.
    let mut p = at_row("ollama", &PickKey::OllamaDownload, &d);
    p.enter(&draft, Some(&d));
    p.back(&draft, Some(&d));
    let popular = Ok(Answer::Popular {
        cards: vec![card("llama3.2", &["1b"], "")],
        fallback: false,
        ram: RAM,
    });
    assert_eq!(p.ollama_reply(&Ask::Popular, popular), None);
    assert_eq!(p.ollama(), None);
    assert_eq!(p.key(&draft, Some(&d)), Some(PickKey::OllamaDownload));
}

#[test]
fn a_failed_request_keeps_the_screen_that_asked_and_says_why() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_popular(vec![card("llama3.2", &["1b"], "")], false, &d);
    let sizes = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned()],
    };
    p.enter(&draft, Some(&d));
    let message = "Ollama library request failed: HTTP 500";
    assert_eq!(
        p.ollama_reply(&sizes, Err(message.to_owned())),
        Some(message.to_owned())
    );
    let o = p.ollama().unwrap();
    assert_eq!(o.title(), "Ollama: most popular for this PC");
    assert_eq!(o.asking(), None);
}

fn running(source: Source, file: &str, license: &str) -> Running {
    Running {
        source,
        file: file.to_owned(),
        license: license.to_owned(),
        done: 50,
        total: 100,
    }
}

// Spec (config-menu, Ollama download row): one download at a time.
#[test]
fn a_running_pull_shows_on_the_ollama_row_and_enter_asks_to_cancel() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_row("ollama", &PickKey::OllamaDownload, &d);
    p.set_download(Some(running(Source::Ollama, "llama3.2:3b", LLAMA)));
    assert_eq!(
        p.rows(&draft, Some(&d)).last().map(|r| r.label.as_str()),
        Some("↓ 50% llama3.2:3b · LLAMA 3.2 COMMUNITY LICENSE AGREEMENT")
    );
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert!(p.asks_cancel());
    assert_eq!(p.answer_cancel(true), Outcome::CancelDownload);
}

#[test]
fn a_hugging_face_download_keeps_the_ollama_row_busy() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_row("ollama", &PickKey::OllamaDownload, &d);
    p.set_download(Some(running(Source::HuggingFace, "a.gguf", "mit")));
    assert_eq!(
        p.rows(&draft, Some(&d)).last().map(|r| r.label.as_str()),
        Some(OLLAMA_ROW)
    );
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Busy);
    assert!(!p.asks_cancel());
    assert_eq!(p.ollama(), None);
}

#[test]
fn an_ollama_pull_keeps_the_hugging_face_row_busy() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_row("llamacpp", &PickKey::HfDownload, &d);
    p.set_download(Some(running(Source::Ollama, "llama3.2:3b", LLAMA)));
    assert_eq!(
        p.rows(&draft, Some(&d)).last().map(|r| r.label.as_str()),
        Some(HF_ROW)
    );
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Busy);
    assert!(!p.asks_cancel());
    assert_eq!(p.hub(), None);
}

#[test]
fn the_ollama_screens_neither_copy_nor_refresh() {
    let d = ollama(Status::Available);
    let p = at_popular(vec![card("llama3.2", &["1b"], "")], false, &d);
    assert_eq!(p.copy(&AiConfig::default(), Some(&d)), Outcome::Stay);
    assert_eq!(p.refresh(), Outcome::Stay);
}

// --- tool marks (model-marks, download screens) -----------------------------

fn tool_card(name: &str, sizes: &[&str]) -> Card {
    Card {
        capabilities: vec!["tools".to_owned()],
        ..card(name, sizes, "")
    }
}

fn mark_on(rows: &[PickRow], prefix: &str) -> Option<ToolMark> {
    rows.iter()
        .find(|r| r.label.starts_with(prefix))
        .unwrap_or_else(|| panic!("no row {prefix}"))
        .tools
}

#[test]
fn popular_rows_carry_the_mark_of_the_largest_fitting_size() {
    let d = ollama(Status::Available);
    let p = at_popular(
        vec![
            tool_card("llama3.2", &["1b", "3b"]),
            tool_card("smol", &["1b"]),
            card("chat-only", &["7b"], ""),
            tool_card("mix", &["1b", "70b"]),
            tool_card("mix2", &["1b", "3b", "8b"]),
            tool_card("edge", &["3b"]),
            tool_card("effective", &["e2b"]),
        ],
        false,
        &d,
    );
    let rows = p.ollama().unwrap().rows();
    assert_eq!(mark_on(&rows, "llama3.2 "), Some(ToolMark::Capable));
    assert_eq!(mark_on(&rows, "smol "), Some(ToolMark::Unreliable));
    assert_eq!(mark_on(&rows, "chat-only "), None);
    assert_eq!(mark_on(&rows, "mix "), Some(ToolMark::Unreliable));
    assert_eq!(mark_on(&rows, "mix2 "), Some(ToolMark::Capable));
    assert_eq!(mark_on(&rows, "edge "), Some(ToolMark::Capable));
    // An "effective" size has no parameter count: it never fits a known RAM.
    assert!(rows.iter().all(|r| !r.label.starts_with("effective ")));
}

#[test]
fn an_unknown_ram_lists_every_card_and_a_size_without_params_gets_no_mark() {
    let d = ollama(Status::Available);
    let mut p = at_row("ollama", &PickKey::OllamaDownload, &d);
    p.enter(&AiConfig::default(), Some(&d));
    let reply = Answer::Popular {
        cards: vec![
            tool_card("effective", &["e2b"]),
            tool_card("llama3.1", &["8b"]),
        ],
        fallback: false,
        ram: None,
    };
    assert_eq!(p.ollama_reply(&Ask::Popular, Ok(reply)), None);
    let rows = p.ollama().unwrap().rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(mark_on(&rows, "effective "), None);
    assert_eq!(mark_on(&rows, "llama3.1 "), Some(ToolMark::Capable));
}

#[test]
fn a_tag_without_a_known_size_carries_no_mark() {
    assert_eq!(ollama::tag_mark(true, "latest"), None);
    assert_eq!(ollama::tag_mark(true, "e2b"), None);
    assert_eq!(ollama::tag_mark(true, "8x7b"), Some(ToolMark::Capable));
    assert_eq!(ollama::tag_mark(false, "8b"), None);
}

#[test]
fn tools_in_a_card_is_read_ignoring_case() {
    let upper = Card {
        capabilities: vec!["Tools".to_owned()],
        ..card("m", &["8b"], "")
    };
    assert!(ollama::declares_tools(&upper));
}

#[test]
fn the_fallback_list_and_cards_without_data_carry_no_mark_and_no_row_is_dropped() {
    let d = ollama(Status::Available);
    let p = at_popular(
        vec![
            card("llama3.2", &["1b", "3b"], ""),
            card("qwen3", &["8b"], ""),
        ],
        true,
        &d,
    );
    let rows = p.ollama().unwrap().rows();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.tools.is_none()));
}

#[test]
fn sizes_rows_carry_the_mark_of_their_own_tag() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_popular(vec![tool_card("llama3.2", &["1b", "3b", "70b"])], false, &d);
    let sizes = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned(), "3b".to_owned(), "70b".to_owned()],
    };
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::OllamaAsk(sizes.clone()));
    let mut manifests = llama_manifests();
    manifests[2].1 = Some(Manifest {
        size: 42 * GB,
        licenses: Vec::new(),
    });
    p.ollama_reply(
        &sizes,
        Ok(Answer::Sizes {
            manifests,
            ram: RAM,
        }),
    );
    let o = p.ollama().unwrap();
    assert!(matches!(
        o.screen(),
        Some(ollama::Screen::Sizes { tools: true, .. })
    ));
    let rows = o.rows();
    assert_eq!(mark_on(&rows, "llama3.2:1b"), Some(ToolMark::Unreliable));
    assert_eq!(mark_on(&rows, "llama3.2:3b"), Some(ToolMark::Capable));
    assert_eq!(mark_on(&rows, "llama3.2:70b"), Some(ToolMark::Capable));
    assert!(rows[2].label.ends_with("· too big for this PC"));
    assert!(rows.iter().all(|r| r.enabled));
}

#[test]
fn sizes_of_a_model_without_tools_carry_no_mark() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    let mut p = at_popular(vec![card("llama3.2", &["1b", "3b"], "")], false, &d);
    let sizes = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned(), "3b".to_owned()],
    };
    p.enter(&draft, Some(&d));
    p.ollama_reply(
        &sizes,
        Ok(Answer::Sizes {
            manifests: llama_manifests(),
            ram: RAM,
        }),
    );
    let o = p.ollama().unwrap();
    assert!(matches!(
        o.screen(),
        Some(ollama::Screen::Sizes { tools: false, .. })
    ));
    assert!(o.rows().iter().all(|r| r.tools.is_none()));
}

#[test]
fn enter_on_a_marked_row_asks_exactly_what_an_unmarked_one_asks() {
    let d = ollama(Status::Available);
    let draft = AiConfig::default();
    for card in [
        tool_card("llama3.2", &["1b", "3b"]),
        card("llama3.2", &["1b", "3b"], ""),
    ] {
        let mut p = at_popular(vec![card], false, &d);
        assert_eq!(
            p.enter(&draft, Some(&d)),
            Outcome::OllamaAsk(Ask::Sizes {
                model: "llama3.2".to_owned(),
                tags: vec!["1b".to_owned(), "3b".to_owned()],
            })
        );
    }
}

#[test]
fn the_confirm_screen_carries_no_mark() {
    let d = ollama(Status::Available);
    let p = at_confirm(vec![LLAMA.to_owned()], &d);
    assert!(p.ollama().unwrap().rows().iter().all(|r| r.tools.is_none()));
}
