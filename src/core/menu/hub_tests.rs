//! The Hugging Face screens of the llamacpp picker (config-menu, Hugging
//! Face screens): Query, Repos, Files and Confirm.

use std::path::PathBuf;

use super::ai_tests::{detection, engine, menu, press};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::Status;
use crate::core::ai::hf::{Answer, Ask, HubFile, Listing, Repo, RepoFiles, RepoHit};
use crate::core::ai::picker::{HF_ROW, Running};

const GB: u64 = 1_000_000_000;

fn llamacpp_menu() -> MenuState {
    let mut d = detection(&[engine("llamacpp", Status::NotRunning, &[], None)]);
    d.local_models = vec![PathBuf::from("/m/b.gguf")];
    menu(None, None, Some(d))
}

/// Moves the cursor of the open picker onto the row labelled `label`.
fn goto_pick(m: &mut MenuState, label: &str) {
    for _ in 0..30 {
        if cursor_label(m) == label {
            return;
        }
        press(m, 'j');
    }
    panic!("no pick row {label}: {:?}", m.rows(&Config::default()));
}

/// The chat picker, on llamacpp's model level.
fn at_llamacpp(m: &mut MenuState) {
    open(m, "chat");
    goto_pick(m, "llamacpp");
    send(m, key(KeyCode::Enter));
}

/// The memory of the machine in these tests.
const RAM: Option<u64> = Some(8 * GB);

/// Enter on the entry row of a fresh menu: the popular models asked for.
fn at_popular_ask() -> MenuState {
    let mut m = llamacpp_menu();
    at_llamacpp(&mut m);
    goto_pick(&mut m, HF_ROW);
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::HubAsk(Ask::Popular)
    );
    m
}

/// The popular list answered with `hits` on a machine with `ram`.
fn at_popular(hits: Vec<RepoHit>, ram: Option<u64>) -> MenuState {
    let mut m = at_popular_ask();
    m.hub_reply(Ask::Popular, Ok(Answer::Repos { hits, ram }));
    m
}

/// The Query level: Esc on an empty popular list.
fn at_query() -> MenuState {
    let mut m = at_popular(Vec::new(), RAM);
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    m
}

fn hit(id: &str, downloads: u64, params: Option<u64>) -> RepoHit {
    RepoHit {
        id: id.to_owned(),
        downloads,
        params,
    }
}

fn title(m: &MenuState) -> String {
    m.ai.as_ref()
        .and_then(|ai| ai.picker())
        .and_then(|p| p.hub())
        .map(|hub| hub.title())
        .expect("a Hugging Face screen")
}

fn type_text(m: &mut MenuState, text: &str) {
    for c in text.chars() {
        let event = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        assert_eq!(send(m, event), MenuCommand::None);
    }
}

fn hint(m: &MenuState) -> String {
    match m.footer() {
        Footer::Hint(hint) => hint.into_owned(),
        Footer::Error(error) => panic!("an error, not a hint: {error}"),
    }
}

fn field(m: &MenuState) -> Option<String> {
    m.rows(&Config::default())
        .into_iter()
        .find_map(|r| match r {
            Row::Field(text) => Some(text),
            _ => None,
        })
}

fn pick_labels(m: &MenuState) -> Vec<String> {
    m.rows(&Config::default())
        .into_iter()
        .filter_map(|r| match r {
            Row::Pick { label, .. } => Some(label),
            _ => None,
        })
        .collect()
}

fn hits() -> Vec<RepoHit> {
    vec![hit("o/small", 10, None), hit("o/a", 1200, None)]
}

fn file(path: &str, size: u64) -> HubFile {
    HubFile {
        path: path.to_owned(),
        size,
        oid: "a".repeat(64),
    }
}

fn listing(free: u64) -> Listing {
    let repo = Repo {
        id: "o/a".to_owned(),
        commit: "c1".to_owned(),
        license_id: Some("apache-2.0".to_owned()),
        license_link: Some("https://example.com/LICENSE".to_owned()),
        files: Vec::new(),
    };
    let files = RepoFiles::new(
        repo,
        &[file("a-q8_0.gguf", 8 * GB), file("a-Q4_K_M.gguf", 4 * GB)],
    );
    Listing {
        missing: vec![4 * GB, 8 * GB],
        files,
        hub: Some(PathBuf::from("/home/u/.cache/huggingface/hub")),
        free: Some(free),
        ram: None,
    }
}

/// The Confirm level of `o/a`'s Q4_K_M choice, reached by a pasted id.
fn at_confirm(free: u64) -> MenuState {
    let mut m = at_query();
    m.on_paste("o/a");
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::HubAsk(Ask::Files("o/a".to_owned()))
    );
    m.hub_reply(
        Ask::Files("o/a".to_owned()),
        Ok(Answer::Files(Box::new(listing(free)))),
    );
    assert_eq!(cursor_label(&m), "Q4_K_M  4.0 GB  a-Q4_K_M.gguf");
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    m
}

// Spec (hf-popular S1): Enter on the row asks for the popular models; no
// typing is needed.
#[test]
fn enter_on_the_row_asks_for_the_popular_models() {
    let m = at_popular_ask();
    assert_eq!(hint(&m), "asking Hugging Face…");
}

// Spec (hf-popular S2, S3): the list holds what fits this PC, the most
// downloaded first, sized; what does not fit or has no size is left out.
#[test]
fn the_popular_list_keeps_what_fits_this_pc_most_downloaded_first() {
    let mut m = at_popular(
        vec![
            hit("o/huge", 9_000, Some(70_000_000_000)),
            hit("o/one-b", 50, Some(1_000_000_000)),
            hit("o/unknown", 8_000, None),
            hit("o/four-b", 700, Some(4_000_000_000)),
        ],
        RAM,
    );
    assert_eq!(
        pick_labels(&m),
        ["o/four-b  700 ↓  ~2.5 GB", "o/one-b  50 ↓  ~625 MB"]
    );
    assert_eq!(cursor_label(&m), "o/four-b  700 ↓  ~2.5 GB");
    assert_eq!(title(&m), "Hugging Face: most downloaded for this PC");
    assert_eq!(hint(&m), "j/k move · Enter choose · Esc search");
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::HubAsk(Ask::Files("o/four-b".to_owned()))
    );
}

#[test]
fn a_popular_list_with_nothing_that_fits_says_so() {
    let m = at_popular(vec![hit("o/huge", 9, Some(70_000_000_000))], RAM);
    assert_eq!(
        m.rows(&Config::default())
            .into_iter()
            .filter_map(|r| match r {
                Row::Pick { label, .. } => Some(label),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["no popular model fits this PC; Esc to search"]
    );
}

// Spec (hf-popular S1): the search stays, one Esc away.
#[test]
fn esc_on_the_popular_list_opens_the_search_field() {
    let mut m = at_popular(vec![hit("o/one-b", 50, Some(1_000_000_000))], RAM);
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    assert_eq!(field(&m), Some(String::new()));
    assert_eq!(hint(&m), "Enter search · Esc back");
}

// With the memory unknown nothing is hidden or marked.
#[test]
fn an_unknown_memory_hides_and_marks_nothing() {
    let m = at_popular(
        vec![
            hit("o/huge", 9_000, Some(70_000_000_000)),
            hit("o/unknown", 8_000, None),
        ],
        None,
    );
    assert_eq!(
        pick_labels(&m),
        ["o/huge  9000 ↓  ~43.8 GB", "o/unknown  8000 ↓"]
    );
}

// Spec (hf-popular S3): a typed search shows everything and marks what does
// not fit.
#[test]
fn a_search_marks_what_is_too_big_for_this_pc() {
    let mut m = at_query();
    type_text(&mut m, "qwen");
    send(&mut m, key(KeyCode::Enter));
    m.hub_reply(
        Ask::Search("qwen".to_owned()),
        Ok(Answer::Repos {
            hits: vec![
                hit("o/huge", 9_000, Some(70_000_000_000)),
                hit("o/four-b", 700, Some(4_000_000_000)),
                hit("o/unknown", 8, None),
            ],
            ram: RAM,
        }),
    );
    assert_eq!(
        pick_labels(&m),
        [
            "o/huge  9000 ↓  ~43.8 GB · too big for this PC",
            "o/four-b  700 ↓  ~2.5 GB",
            "o/unknown  8 ↓",
        ]
    );
    assert_eq!(hint(&m), "j/k move · Enter choose · Esc back");
}

// Spec (hf-popular S3): the files too big for this PC are marked.
#[test]
fn files_too_big_for_this_pc_are_marked() {
    let mut m = at_query();
    m.on_paste("o/a");
    send(&mut m, key(KeyCode::Enter));
    m.hub_reply(
        Ask::Files("o/a".to_owned()),
        Ok(Answer::Files(Box::new(Listing {
            ram: RAM,
            ..listing(100 * GB)
        }))),
    );
    assert_eq!(
        pick_labels(&m)[1..],
        [
            "Q4_K_M  4.0 GB  a-Q4_K_M.gguf",
            "Q8_0  8.0 GB  a-q8_0.gguf · too big for this PC",
        ]
    );
}

#[test]
fn a_failed_popular_request_keeps_the_search_field() {
    let mut m = at_popular_ask();
    m.hub_reply(Ask::Popular, Err("offline".to_owned()));
    assert_eq!(m.footer(), Footer::Error("offline"));
    assert_eq!(field(&m), Some(String::new()));
}

#[test]
fn enter_on_the_row_opens_the_query_level() {
    let m = at_query();
    assert_eq!(field(&m), Some(String::new()));
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Text("search the Hub or owner/repo".into()))
    );
    assert_eq!(hint(&m), "Enter search · Esc back");
}

// Scenario: Paste a repo id.
#[test]
fn a_pasted_repo_id_asks_for_its_files() {
    let mut m = at_query();
    m.on_paste("Qwen/Qwen2.5-0.5B-Instruct-GGUF");
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::HubAsk(Ask::Files("Qwen/Qwen2.5-0.5B-Instruct-GGUF".to_owned()))
    );
    assert_eq!(hint(&m), "asking Hugging Face…");
}

#[test]
fn empty_text_does_nothing_and_other_text_searches() {
    let mut m = at_query();
    type_text(&mut m, "  ");
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert_eq!(hint(&m), "Enter search · Esc back");
    type_text(&mut m, "a/b/c");
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::HubAsk(Ask::Search("a/b/c".to_owned()))
    );
}

// Scenario: Search then pick.
#[test]
fn search_then_pick_shows_the_confirmation_and_downloads_nothing() {
    let mut m = at_query();
    let mut commands = Vec::new();
    type_text(&mut m, "qwen 0.5b");
    commands.push(send(&mut m, key(KeyCode::Enter)));
    assert_eq!(
        commands[0],
        MenuCommand::HubAsk(Ask::Search("qwen 0.5b".to_owned()))
    );
    m.hub_reply(
        Ask::Search("qwen 0.5b".to_owned()),
        Ok(Answer::Repos {
            hits: hits(),
            ram: RAM,
        }),
    );
    assert_eq!(pick_labels(&m), ["o/a  1200 ↓", "o/small  10 ↓"]);
    assert_eq!(hint(&m), "j/k move · Enter choose · Esc back");
    commands.push(send(&mut m, key(KeyCode::Enter)));
    assert_eq!(
        commands[1],
        MenuCommand::HubAsk(Ask::Files("o/a".to_owned()))
    );
    m.hub_reply(
        Ask::Files("o/a".to_owned()),
        Ok(Answer::Files(Box::new(listing(50 * GB)))),
    );
    assert_eq!(
        pick_labels(&m),
        [
            "license: apache-2.0",
            "Q4_K_M  4.0 GB  a-Q4_K_M.gguf",
            "Q8_0  8.0 GB  a-q8_0.gguf",
        ]
    );
    goto_pick(&mut m, "Q4_K_M  4.0 GB  a-Q4_K_M.gguf");
    commands.push(send(&mut m, key(KeyCode::Enter)));
    assert_eq!(
        pick_labels(&m),
        [
            "download a-Q4_K_M.gguf (4.0 GB)",
            "from o/a",
            "license: apache-2.0",
            "read it: https://example.com/LICENSE",
            "to: /home/u/.cache/huggingface/hub",
            "free: 50.0 GB",
        ]
    );
    assert_eq!(hint(&m), "y download · n back");
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, MenuCommand::Download(_))),
        "{commands:?}"
    );
}

#[test]
fn no_result_and_no_files_read_as_info_rows() {
    let mut m = at_query();
    type_text(&mut m, "zzz");
    send(&mut m, key(KeyCode::Enter));
    m.hub_reply(
        Ask::Search("zzz".to_owned()),
        Ok(Answer::Repos {
            hits: Vec::new(),
            ram: RAM,
        }),
    );
    assert_eq!(pick_labels(&m), ["no GGUF models found for \"zzz\""]);
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);

    let mut m = at_query();
    m.on_paste("o/a");
    send(&mut m, key(KeyCode::Enter));
    let mut empty = listing(GB);
    empty.files.choices.clear();
    m.hub_reply(
        Ask::Files("o/a".to_owned()),
        Ok(Answer::Files(Box::new(empty))),
    );
    assert_eq!(
        pick_labels(&m),
        ["license: apache-2.0", "no .gguf files in o/a"]
    );
}

// Scenario: Nothing downloads before y.
#[test]
fn only_y_on_the_confirmation_downloads_and_only_once() {
    for code in [
        KeyCode::Char('n'),
        KeyCode::Esc,
        KeyCode::Char('j'),
        KeyCode::Enter,
        KeyCode::Char('s'),
    ] {
        let mut m = at_confirm(50 * GB);
        let command = send(&mut m, key(code));
        assert!(
            !matches!(command, MenuCommand::Download(_) | MenuCommand::Save),
            "{code:?}: {command:?}"
        );
        assert_eq!(m.edits(&Config::default()), Vec::new(), "{code:?}");
    }
    let mut m = at_confirm(50 * GB);
    let MenuCommand::Download(job) = press(&mut m, 'y') else {
        panic!("y did not download");
    };
    assert_eq!(job.id, "o/a");
    assert_eq!(job.commit, "c1");
    assert_eq!(job.choice.name(), "a-Q4_K_M.gguf");
    assert_eq!(job.license.id, "apache-2.0");
    // Back on llamacpp's model level, whose last row is the entry.
    assert_eq!(cursor_label(&m), HF_ROW);
    assert!(!matches!(press(&mut m, 'y'), MenuCommand::Download(_)));
    assert_eq!(m.edits(&Config::default()), Vec::new());
}

#[test]
fn n_and_esc_go_back_to_the_files() {
    for code in [KeyCode::Char('n'), KeyCode::Esc] {
        let mut m = at_confirm(50 * GB);
        send(&mut m, key(code));
        assert_eq!(cursor_label(&m), "Q4_K_M  4.0 GB  a-Q4_K_M.gguf");
    }
}

#[test]
fn short_free_space_shows_the_error_and_y_does_nothing() {
    let mut m = at_confirm(GB / 2);
    assert_eq!(
        m.footer(),
        Footer::Error("not enough disk space: 4.0 GB needed, 500 MB free")
    );
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    assert!(pick_labels(&m).contains(&"free: 500 MB".to_owned()));
}

// Scenario: Error stays on its level.
#[test]
fn a_failed_request_shows_its_message_and_keeps_the_query() {
    let mut m = at_query();
    type_text(&mut m, "gated/model");
    send(&mut m, key(KeyCode::Enter));
    let message = "gated/model needs a Hugging Face token: set HF_TOKEN";
    m.hub_reply(
        Ask::Files("gated/model".to_owned()),
        Err(message.to_owned()),
    );
    assert_eq!(m.footer(), Footer::Error(message));
    assert_eq!(field(&m), Some("gated/model".to_owned()));
    // Not waiting any more: Enter asks again.
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::HubAsk(Ask::Files("gated/model".to_owned()))
    );
}

// Scenario: Late reply dropped.
#[test]
fn a_reply_for_a_level_the_user_left_is_dropped() {
    let mut m = at_query();
    type_text(&mut m, "qwen");
    send(&mut m, key(KeyCode::Enter));
    m.hub_reply(
        Ask::Search("qwen".to_owned()),
        Ok(Answer::Repos {
            hits: hits(),
            ram: RAM,
        }),
    );
    send(&mut m, key(KeyCode::Enter));
    send(&mut m, key(KeyCode::Esc));
    // Back on the Query level, with its text.
    assert_eq!(field(&m), Some("qwen".to_owned()));
    m.hub_reply(
        Ask::Files("o/a".to_owned()),
        Ok(Answer::Files(Box::new(listing(GB)))),
    );
    assert_eq!(field(&m), Some("qwen".to_owned()));
    assert_eq!(hint(&m), "Enter search · Esc back");
}

// A terminal that sends ^H for Backspace delivers Ctrl+H.
#[test]
fn ctrl_h_deletes_like_backspace_in_the_query() {
    let mut m = at_query();
    type_text(&mut m, "qwen");
    send(
        &mut m,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
    );
    assert_eq!(field(&m), Some("qwe".to_owned()));
}

#[test]
fn keys_other_than_esc_are_ignored_while_asking() {
    let mut m = at_query();
    type_text(&mut m, "qwen");
    send(&mut m, key(KeyCode::Enter));
    for code in [KeyCode::Char('x'), KeyCode::Backspace, KeyCode::Enter] {
        assert_eq!(send(&mut m, key(code)), MenuCommand::None);
    }
    m.on_paste("more");
    assert_eq!(field(&m), Some("qwen".to_owned()));
    assert_eq!(hint(&m), "asking Hugging Face…");
    // Esc leaves the level, and the reply that comes later is dropped.
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(cursor_label(&m), HF_ROW);
    m.hub_reply(
        Ask::Search("qwen".to_owned()),
        Ok(Answer::Repos {
            hits: hits(),
            ram: RAM,
        }),
    );
    assert_eq!(cursor_label(&m), HF_ROW);
}

#[test]
fn esc_on_the_query_goes_back_to_the_entry_row() {
    let mut m = at_query();
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(cursor_label(&m), HF_ROW);
    assert_eq!(m.edits(&Config::default()), Vec::new());
}

#[test]
fn hub_labels_are_sanitized() {
    let mut m = at_query();
    m.on_paste("o/a");
    send(&mut m, key(KeyCode::Enter));
    let mut bad = listing(50 * GB);
    bad.files.license.id = "mit\u{1b}[2J\u{202e}".to_owned();
    m.hub_reply(
        Ask::Files("o/a".to_owned()),
        Ok(Answer::Files(Box::new(bad))),
    );
    assert_eq!(pick_labels(&m)[0], "license: mit?[2J?");
}

fn running() -> Running {
    Running {
        source: crate::core::ai::picker::Source::HuggingFace,
        file: "a-Q4_K_M.gguf".to_owned(),
        license: "apache-2.0".to_owned(),
        done: 42,
        total: 100,
    }
}

#[test]
fn enter_on_a_running_download_asks_to_cancel_it() {
    let mut m = llamacpp_menu();
    m.set_download(Some(running()));
    at_llamacpp(&mut m);
    goto_pick(&mut m, "↓ 42% a-Q4_K_M.gguf · apache-2.0");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), "cancel the download? (y/n)");
    // Any other key declines.
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_ne!(hint(&m), "cancel the download? (y/n)");
    assert_eq!(cursor_label(&m), "↓ 42% a-Q4_K_M.gguf · apache-2.0");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(press(&mut m, 'y'), MenuCommand::CancelDownload);
    assert_ne!(hint(&m), "cancel the download? (y/n)");
}

#[test]
fn the_end_of_a_download_puts_the_entry_back() {
    let mut m = llamacpp_menu();
    m.set_download(Some(running()));
    at_llamacpp(&mut m);
    goto_pick(&mut m, "↓ 42% a-Q4_K_M.gguf · apache-2.0");
    send(&mut m, key(KeyCode::Enter));
    m.set_download(None);
    assert_eq!(cursor_label(&m), HF_ROW);
    assert_ne!(hint(&m), "cancel the download? (y/n)");
}

// The Ollama screens of the ollama picker (config-menu, Ollama screens).

/// The chat picker, on the Popular screen of a running Ollama.
fn at_ollama_popular() -> MenuState {
    use crate::core::ai::ollama_lib::{Answer, Ask, Card};
    let d = detection(&[engine("ollama", Status::Available, &["qwen3:4b"], None)]);
    let mut m = menu(None, None, Some(d));
    open(&mut m, "chat");
    goto_pick(&mut m, "ollama");
    send(&mut m, key(KeyCode::Enter));
    goto_pick(&mut m, crate::core::ai::picker::OLLAMA_ROW);
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::OllamaAsk(Ask::Popular)
    );
    // Every key but Esc waits for the reply.
    assert_eq!(hint(&m), "asking ollama.com…");
    assert_eq!(press(&mut m, 'j'), MenuCommand::None);
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    let card = |name: &str| Card {
        name: name.to_owned(),
        description: String::new(),
        capabilities: Vec::new(),
        sizes: vec!["1b".to_owned()],
        pulls: "1M".to_owned(),
    };
    m.ollama_reply(
        Ask::Popular,
        Ok(Answer::Popular {
            cards: vec![card("a"), card("b")],
            fallback: false,
            ram: RAM,
        }),
    );
    m
}

#[test]
fn the_ollama_screens_move_with_j_and_k_and_only_confirm_takes_y() {
    let mut m = at_ollama_popular();
    assert_eq!(hint(&m), "j/k move · Enter choose · Esc back");
    assert_eq!(cursor_label(&m), "a  1M pulls  1b");
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "b  1M pulls  1b");
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(cursor_label(&m), "b  1M pulls  1b");
    // Esc goes back to the ollama model level, on the entry row.
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(cursor_label(&m), crate::core::ai::picker::OLLAMA_ROW);
}

#[test]
fn a_failed_ollama_request_is_the_footer_error() {
    use crate::core::ai::ollama_lib::Ask;
    let mut m = at_ollama_popular();
    let ask = Ask::Sizes {
        model: "a".to_owned(),
        tags: vec!["1b".to_owned()],
    };
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::OllamaAsk(ask.clone())
    );
    let message = "Ollama library request failed: HTTP 500";
    m.ollama_reply(ask, Err(message.to_owned()));
    assert_eq!(m.footer(), Footer::Error(message));
}
