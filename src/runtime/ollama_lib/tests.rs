use super::*;
use crate::runtime::hf::tests::{curl_available, ok, reply, serve};

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
