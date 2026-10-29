use super::*;
use crate::runtime::hf::tests::{curl_available, ok, reply, serve};
use tokio::sync::mpsc;

const PAGE_HTML: &str = include_str!("../../../tests/fixtures/ollama/library-popular.html");

macro_rules! need_curl {
    () => {
        if !curl_available() {
            eprintln!("SKIPPED: curl is not installed; the curl-backed checks did not run");
            return;
        }
    };
}

/// A test curl of the Ollama library: plain HTTP to the local server.
fn local(base: &str) -> Curl {
    Curl::local_ollama(base)
}

// Spec (ollama-download, Requests are HTTPS curl GETs).
#[test]
fn the_production_requests_are_https_only_with_the_url_last() {
    let (site, registry) = curls("/usr/bin".into());
    let page = site.fetch_args(ollama_lib::library_path(), PAGE);
    assert!(
        page.windows(2).any(|w| w == ["--proto", "=https"]),
        "{page:?}"
    );
    assert!(
        page.windows(2)
            .any(|w| w == ["--header", "Accept: text/html"])
    );
    assert!(!page.contains(&"--location".to_owned()), "no redirects");
    assert_eq!(
        page[page.len() - 2..],
        [
            "--".to_owned(),
            "https://ollama.com/library?sort=popular".to_owned()
        ]
    );
    let blob = registry.fetch_args("/v2/library/m/blobs/x", LICENSE);
    assert!(
        blob.contains(&"--location".to_owned()),
        "the blob follows its redirect"
    );
    assert!(blob.windows(2).any(|w| w == ["--proto-redir", "=https"]));
    assert_eq!(
        blob.last().unwrap(),
        "https://registry.ollama.ai/v2/library/m/blobs/x"
    );
}

#[tokio::test]
async fn the_library_page_reads_into_cards() {
    need_curl!();
    let (base, seen) = serve(Box::new(|_| ok(PAGE_HTML)));
    let cards = library(&local(&base)).await.unwrap();
    assert_eq!(cards.len(), 9);
    let seen = seen.lock().unwrap();
    assert!(
        seen[0].starts_with("GET /library?sort=popular "),
        "{seen:?}"
    );
}

#[tokio::test]
async fn a_failed_page_says_so() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| reply("503 Service Unavailable", b"down")));
    assert_eq!(
        library(&local(&base)).await,
        Err("Ollama library request failed: HTTP 503".to_owned())
    );
}

#[tokio::test]
async fn a_manifest_gives_its_size_and_a_license_its_title() {
    need_curl!();
    let digest = format!("sha256:{}", "a".repeat(64));
    let manifest_body = format!(
        r#"{{"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{}","size":10}},
            {{"mediaType":"application/vnd.ollama.image.license","digest":"{digest}","size":5}}]}}"#,
        "b".repeat(64)
    );
    let (base, seen) = serve(Box::new(move |request| {
        if request.contains("/manifests/") {
            ok(&manifest_body)
        } else {
            ok("\nMIT License\n\nCopyright…")
        }
    }));
    let registry = local(&base);
    let found = manifest(&registry, "qwen3", "4b").await.unwrap();
    assert_eq!(
        (found.size, found.licenses.clone()),
        (15, vec![digest.clone()])
    );
    assert_eq!(
        license(&registry, "qwen3", &digest).await,
        Ok("MIT License".to_owned())
    );
    let seen = seen.lock().unwrap();
    assert!(
        seen[0].starts_with("GET /v2/library/qwen3/manifests/4b "),
        "{seen:?}"
    );
    assert!(
        seen[0].contains("application/vnd.docker.distribution.manifest.v2+json"),
        "{seen:?}"
    );
    assert!(seen[1].starts_with(&format!("GET /v2/library/qwen3/blobs/{digest} ")));
}

#[tokio::test]
async fn a_long_license_is_cut_not_refused() {
    need_curl!();
    let text = format!("LONG LICENSE\n{}", "x".repeat(ollama_lib::LICENSE_CAP * 2));
    let (base, _) = serve(Box::new(move |_| ok(&text)));
    let digest = format!("sha256:{}", "a".repeat(64));
    assert_eq!(
        license(&local(&base), "m", &digest).await,
        Ok("LONG LICENSE".to_owned())
    );
}

// Spec (ollama-download, Unsafe names are never sent).
#[tokio::test]
async fn unsafe_names_are_never_sent() {
    let (base, seen) = serve(Box::new(|_| ok("{}")));
    let registry = local(&base);
    assert!(manifest(&registry, "../x", "4b").await.is_err());
    assert!(manifest(&registry, "m", "a b").await.is_err());
    assert!(license(&registry, "a/b", "sha256:x").await.is_err());
    assert!(seen.lock().unwrap().is_empty(), "no request was sent");
}

/// A worker whose library and registry are the local server `base`, its
/// replies on the returned receiver.
fn worker(base: &str, folder: Option<PathBuf>) -> (Ollama, mpsc::Receiver<AppEvent>) {
    let (tx, rx) = mpsc::channel(8);
    let ollama =
        Ollama::new(std::ffi::OsString::new(), folder, tx).with_curls(local(base), local(base));
    (ollama, rx)
}

async fn reply_to(ollama: &Ollama, rx: &mut mpsc::Receiver<AppEvent>, ask: Ask) -> Answer {
    ollama.ask(ask.clone());
    let event = tokio::time::timeout(std::time::Duration::from_secs(20), rx.recv())
        .await
        .expect("a reply")
        .expect("the worker is alive");
    let AppEvent::OllamaReply(asked, answer) = event else {
        panic!("not an Ollama reply");
    };
    assert_eq!(asked, ask);
    answer.expect("an answer")
}

// Scenario: Unreadable page falls back.
#[tokio::test]
async fn popular_falls_back_to_the_curated_list_when_the_page_fails_or_holds_none() {
    need_curl!();
    for page in [
        reply("503 Service Unavailable", b"down"),
        ok("<html>changed</html>"),
    ] {
        let (base, _) = serve(Box::new(move |_| page.clone()));
        let (ollama, mut rx) = worker(&base, None);
        let Answer::Popular {
            cards, fallback, ..
        } = reply_to(&ollama, &mut rx, Ask::Popular).await
        else {
            panic!("not the Popular answer");
        };
        assert!(fallback);
        assert_eq!(cards, ollama_lib::fallback_cards());
    }
    let (base, _) = serve(Box::new(|_| ok(PAGE_HTML)));
    let (ollama, mut rx) = worker(&base, None);
    let Answer::Popular {
        cards, fallback, ..
    } = reply_to(&ollama, &mut rx, Ask::Popular).await
    else {
        panic!("not the Popular answer");
    };
    assert!(!fallback);
    assert_eq!(cards.len(), 9);
}

#[tokio::test]
async fn sizes_read_one_manifest_per_tag_and_an_unreadable_one_is_unknown() {
    need_curl!();
    let (base, seen) = serve(Box::new(|request| {
        if request.starts_with("GET /v2/library/llama3.2/manifests/3b ") {
            ok(r#"{"layers":[{"mediaType":"m","size":2019377376},{"mediaType":"t","size":15252}]}"#)
        } else {
            reply("404 Not Found", b"{}")
        }
    }));
    let (ollama, mut rx) = worker(&base, None);
    let ask = Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["3b".to_owned(), "1b".to_owned()],
    };
    let Answer::Sizes { manifests, .. } = reply_to(&ollama, &mut rx, ask).await else {
        panic!("not the Sizes answer");
    };
    assert_eq!(
        manifests,
        [
            (
                "3b".to_owned(),
                Some(Manifest {
                    size: 2_019_392_628,
                    licenses: Vec::new()
                })
            ),
            ("1b".to_owned(), None),
        ]
    );
    assert_eq!(seen.lock().unwrap().len(), 2);
}

// Scenario: License shown before the pull.
#[tokio::test]
async fn licenses_read_each_layer_and_name_the_models_folder() {
    need_curl!();
    let digest = format!("sha256:{}", "a".repeat(64));
    let (base, seen) = serve(Box::new(|_| {
        ok("LLAMA 3.2 COMMUNITY LICENSE AGREEMENT\nLlama 3.2 Version Release Date")
    }));
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (ollama, mut rx) = worker(&base, Some(folder.clone()));
    let ask = Ask::Licenses {
        model: "llama3.2".to_owned(),
        tag: "3b".to_owned(),
        digests: Some(vec![digest.clone()]),
    };
    let Answer::Licenses(licenses) = reply_to(&ollama, &mut rx, ask).await else {
        panic!("not the Licenses answer");
    };
    assert_eq!(licenses.titles, ["LLAMA 3.2 COMMUNITY LICENSE AGREEMENT"]);
    assert_eq!(licenses.size, None);
    assert_eq!(licenses.folder.as_ref(), Some(&folder));
    assert!(licenses.free.is_some());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "one GET per license layer: {seen:?}");
    assert!(seen[0].starts_with(&format!("GET /v2/library/llama3.2/blobs/{digest} ")));
}

#[tokio::test]
async fn licenses_of_an_unknown_size_read_its_manifest_first() {
    need_curl!();
    let (base, seen) = serve(Box::new(|request| {
        if request.contains("/manifests/") {
            ok(r#"{"layers":[{"mediaType":"m","size":7}]}"#)
        } else {
            reply("500 Internal Server Error", b"")
        }
    }));
    let (ollama, mut rx) = worker(&base, None);
    let ask = Ask::Licenses {
        model: "qwen3".to_owned(),
        tag: "4b".to_owned(),
        digests: None,
    };
    let Answer::Licenses(licenses) = reply_to(&ollama, &mut rx, ask).await else {
        panic!("not the Licenses answer");
    };
    assert_eq!((licenses.size, licenses.titles.len()), (Some(7), 0));
    assert_eq!((licenses.folder, licenses.free), (None, None));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn the_models_folder_is_the_variable_then_the_first_that_exists() {
    let home = Path::new("/home/u");
    let none = |_: &Path| false;
    assert_eq!(
        models_folder(Some("/srv/models".into()), Some(home), none),
        Some(PathBuf::from("/srv/models"))
    );
    let user = |p: &Path| p == Path::new("/home/u/.ollama/models");
    assert_eq!(
        models_folder(Some("".into()), Some(home), user),
        Some(PathBuf::from("/home/u/.ollama/models"))
    );
    let service = |p: &Path| p == Path::new("/usr/share/ollama/.ollama/models");
    assert_eq!(
        models_folder(None, Some(home), service),
        Some(PathBuf::from("/usr/share/ollama/.ollama/models"))
    );
    assert_eq!(models_folder(None, None, none), None);
}
