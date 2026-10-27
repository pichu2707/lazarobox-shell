use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;

const COMMIT: &str = "9217f5db79a29953eb74d5343926648285ec7e67";

fn oid(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn file(path: &str, size: u64, n: u8) -> HubFile {
    HubFile {
        path: path.to_owned(),
        size,
        oid: oid(n),
    }
}

const GB: u64 = 1_000_000_000;

// Spec (model-download, Hub requests): the search reply is a list of repos.
#[test]
fn a_search_reply_lists_repos_with_their_downloads() {
    let body = r#"[
        {"id":"Qwen/Qwen2.5-0.5B-Instruct-GGUF","downloads":335444,"tags":["gguf"]},
        {"id":"bartowski/x-GGUF","downloads":19,"gguf":{"total":4022468096,"architecture":"qwen3"}},
        {"id":"o/no-total","downloads":7,"gguf":{"architecture":"llama"}},
        {"id":"../evil","downloads":5},
        {"downloads":1}
    ]"#;
    assert_eq!(
        parse_search(body),
        Ok(vec![
            RepoHit {
                id: "Qwen/Qwen2.5-0.5B-Instruct-GGUF".into(),
                downloads: 335_444,
                params: None,
            },
            RepoHit {
                id: "bartowski/x-GGUF".into(),
                downloads: 19,
                params: Some(4_022_468_096),
            },
            RepoHit {
                id: "o/no-total".into(),
                downloads: 7,
                params: None,
            },
        ])
    );
    assert_eq!(parse_search("{}"), Err(UNREADABLE.to_owned()));
    assert_eq!(parse_search("not json"), Err(UNREADABLE.to_owned()));
}

#[test]
fn a_repo_reply_brings_its_commit_license_and_files() {
    let body = format!(
        r#"{{"id":"o/r","sha":"{COMMIT}","gated":false,
            "cardData":{{"license":"apache-2.0","license_link":"https://huggingface.co/o/r/blob/main/LICENSE"}},
            "tags":["license:mit"],
            "siblings":[{{"rfilename":"LICENSE"}},{{"rfilename":"a.gguf"}}]}}"#
    );
    let repo = parse_repo(&body).unwrap();
    assert_eq!(repo.id, "o/r");
    assert_eq!(repo.commit, COMMIT);
    assert_eq!(repo.license_id.as_deref(), Some("apache-2.0"));
    assert_eq!(
        repo.license_link.as_deref(),
        Some("https://huggingface.co/o/r/blob/main/LICENSE")
    );
    assert_eq!(repo.files, ["LICENSE", "a.gguf"]);
    // A commit that is not 40 hex digits could name a path: refused.
    let bad = body.replace(COMMIT, "../../x");
    assert_eq!(parse_repo(&bad), Err(UNREADABLE.to_owned()));
}

#[test]
fn a_repo_reply_falls_back_to_the_license_tag() {
    let body = format!(r#"{{"id":"o/r","sha":"{COMMIT}","tags":["gguf","license:llama3.1"]}}"#);
    let repo = parse_repo(&body).unwrap();
    assert_eq!(repo.license_id.as_deref(), Some("llama3.1"));
    assert_eq!(repo.license_link, None);
}

#[test]
fn a_tree_reply_keeps_files_with_a_safe_path_and_an_oid() {
    let lfs = oid(1);
    let body = format!(
        r#"[
        {{"type":"file","oid":"8a56296b52e3eaf9dbd1784787e6c9d577b15b30","size":135,"lfs":{{"oid":"{lfs}","size":1266}},"path":"a.gguf"}},
        {{"type":"file","oid":"8a56296b52e3eaf9dbd1784787e6c9d577b15b30","size":20,"path":"README.md"}},
        {{"type":"directory","oid":"8a56296b52e3eaf9dbd1784787e6c9d577b15b30","size":0,"path":"sub"}},
        {{"type":"file","oid":"{lfs}","size":9,"lfs":{{"oid":"{lfs}","size":9}},"path":"../x.gguf"}},
        {{"type":"file","oid":"{lfs}","size":9,"lfs":{{"oid":"nothex","size":9}},"path":"b.gguf"}}
    ]"#
    );
    assert_eq!(
        parse_tree(&body),
        Ok(vec![
            file("a.gguf", 1266, 1),
            HubFile {
                path: "README.md".into(),
                size: 20,
                oid: "8a56296b52e3eaf9dbd1784787e6c9d577b15b30".into(),
            },
        ])
    );
}

// Spec (model-download, GGUF choices): Choices from a tree.
#[test]
fn the_choices_of_a_tree_group_splits_and_sort_by_size() {
    let files = [
        file("a-Q4_K_M.gguf", 4 * GB, 1),
        file("a-q8_0.gguf", 8 * GB, 2),
        file("mmproj-a-f16.gguf", GB, 3),
        file("big-Q6_K-00001-of-00002.gguf", 5 * GB, 4),
        file("big-Q6_K-00002-of-00002.gguf", 5 * GB, 5),
        file("README.md", 10, 6),
    ];
    let got: Vec<(String, u64, usize, String)> = choices(&files)
        .into_iter()
        .map(|c| (c.quant.clone(), c.size, c.files.len(), c.name().to_owned()))
        .collect();
    assert_eq!(
        got,
        [
            ("Q4_K_M".into(), 4 * GB, 1, "a-Q4_K_M.gguf".into()),
            ("Q8_0".into(), 8 * GB, 1, "a-q8_0.gguf".into()),
            (
                "Q6_K".into(),
                10 * GB,
                2,
                "big-Q6_K-00001-of-00002.gguf".into()
            ),
        ]
    );
    let labels: Vec<String> = choices(&files).iter().map(Choice::label).collect();
    assert_eq!(
        labels,
        [
            "Q4_K_M  4.0 GB  a-Q4_K_M.gguf",
            "Q8_0  8.0 GB  a-q8_0.gguf",
            "Q6_K  10.0 GB (2 parts)  big-Q6_K-00001-of-00002.gguf",
        ]
    );
}

// Spec (model-download, GGUF choices): Missing part.
#[test]
fn a_split_model_missing_a_part_is_not_a_choice() {
    let files = [file("big-Q6_K-00001-of-00002.gguf", 5 * GB, 4)];
    assert_eq!(choices(&files), vec![]);
}

#[test]
fn the_quantization_is_the_last_quant_token_of_the_name() {
    for (name, want) in [
        ("qwen2.5-0.5b-instruct-q2_k.gguf", "Q2_K"),
        ("Model.IQ3_XXS.gguf", "IQ3_XXS"),
        ("m-UD-Q4_K_XL.gguf", "Q4_K_XL"),
        ("m-bf16.gguf", "BF16"),
        ("m-F32.gguf", "F32"),
        ("m-Q8_0-00001-of-00003.gguf", "Q8_0"),
        ("plain.gguf", "?"),
        ("q.gguf", "?"),
    ] {
        assert_eq!(quant(name), want, "{name}");
    }
}

#[test]
fn sizes_use_decimal_units() {
    for (bytes, text) in [
        (999, "999 B"),
        (1_500, "1 kB"),
        (415_182_688, "415 MB"),
        (500_000_000, "500 MB"),
        (GB, "1.0 GB"),
        (1_266_425_696, "1.3 GB"),
        (10 * GB, "10.0 GB"),
        (2_000 * GB, "2.0 TB"),
    ] {
        assert_eq!(size_label(bytes), text, "{bytes}");
    }
}

// Spec (model-download, License before download): License sources.
#[test]
fn the_license_comes_with_a_link_to_read_it() {
    let repo = |id: Option<&str>, link: Option<&str>, files: &[&str]| Repo {
        id: "o/r".into(),
        commit: COMMIT.into(),
        license_id: id.map(Into::into),
        license_link: link.map(Into::into),
        files: files.iter().map(|f| (*f).to_owned()).collect(),
    };
    let link = "https://huggingface.co/o/r/blob/main/LICENSE";
    assert_eq!(
        license(&repo(Some("apache-2.0"), Some(link), &["LICENSE"])),
        License {
            id: "apache-2.0".into(),
            link: link.into()
        }
    );
    assert_eq!(
        license(&repo(
            Some("gemma"),
            None,
            &["a.gguf", "sub/LICENSE", "License.txt"]
        )),
        License {
            id: "gemma".into(),
            link: format!("https://huggingface.co/o/r/blob/{COMMIT}/License.txt"),
        }
    );
    assert_eq!(
        license(&repo(None, None, &["a.gguf"])),
        License {
            id: "not stated".into(),
            link: "https://huggingface.co/o/r".into(),
        }
    );
    // Only an https link is offered.
    assert_eq!(
        license(&repo(Some("mit"), Some("javascript:alert(1)"), &[])).link,
        "https://huggingface.co/o/r"
    );
}

#[test]
fn a_repo_id_is_owner_slash_name() {
    for ok in ["Qwen/Qwen2.5-0.5B-Instruct-GGUF", "a/b", "o_1/r.x-y"] {
        assert!(is_repo_id(ok), "{ok}");
    }
    for bad in [
        "", "qwen", "a/b/c", "/b", "a/", "a b/c", "../x", "a/..", ".a/b", "a/b c", "a/-b",
        // As on the Hub: no `--` (the cache folder's separator) and no
        // `-` or `.` at the end of a part.
        "a--b/c", "a/b--c", "a-/b", "a/b-", "a/b.",
    ] {
        assert!(!is_repo_id(bad), "{bad}");
    }
}

#[test]
fn the_search_text_is_percent_encoded() {
    assert_eq!(
        search_path("qwen 0.5b&x=1"),
        "/api/models?search=qwen%200.5b%26x%3D1&filter=gguf&sort=downloads&limit=20\
         &expand%5B%5D=gguf&expand%5B%5D=downloads"
    );
    assert_eq!(
        popular_path(),
        "/api/models?filter=gguf&pipeline_tag=text-generation&sort=downloads&limit=100\
         &expand%5B%5D=gguf&expand%5B%5D=downloads"
    );
    assert_eq!(repo_path("o/r"), "/api/models/o/r");
    assert_eq!(
        tree_path("o/r", COMMIT),
        format!("/api/models/o/r/tree/{COMMIT}?recursive=true")
    );
    assert_eq!(
        resolve_url("o/r", COMMIT, "sub/a.gguf"),
        format!("https://huggingface.co/o/r/resolve/{COMMIT}/sub/a.gguf")
    );
}

// Spec (model-download, Hub requests): Failures by status.
#[test]
fn failures_read_by_status() {
    assert_eq!(
        status_message("o/r", 401),
        "o/r needs a Hugging Face token: set HF_TOKEN"
    );
    assert_eq!(
        status_message("o/r", 403),
        "o/r needs a Hugging Face token: set HF_TOKEN"
    );
    assert_eq!(status_message("o/r", 404), "o/r not found on Hugging Face");
    assert_eq!(
        status_message("o/r", 500),
        "Hugging Face request failed: HTTP 500"
    );
}

// Spec (model-download, Token): Lookup order.
#[test]
fn the_token_is_looked_up_in_order() {
    let files: HashMap<PathBuf, String> = [
        (PathBuf::from("/t/path"), "  t1\n".to_owned()),
        (PathBuf::from("/hf/token"), "t2".to_owned()),
        (PathBuf::from("/c/huggingface/token"), "t3".to_owned()),
        (PathBuf::from("/blank"), " \n".to_owned()),
    ]
    .into();
    let read = |p: &Path| files.get(p).cloned();
    let mut env = TokenEnv {
        hf_token: Some(String::new()),
        hf_token_path: Some("/t/path".into()),
        hf_home: Some("/hf".into()),
        xdg_cache_home: Some("/c".into()),
        home: Some("/h".into()),
    };
    assert_eq!(token(&env, read).as_deref(), Some("t1"));
    env.hf_token_path = None;
    assert_eq!(token(&env, read).as_deref(), Some("t2"));
    env.hf_home = None;
    assert_eq!(token(&env, read).as_deref(), Some("t3"));
    env.hf_token = Some(" t0 ".into());
    assert_eq!(token(&env, read).as_deref(), Some("t0"));
    // A blank file counts as none, and nothing is found past it.
    let blank = TokenEnv {
        hf_token: None,
        hf_token_path: Some("/blank".into()),
        hf_home: None,
        xdg_cache_home: None,
        home: None,
    };
    assert_eq!(token(&blank, read), None);
    // A relative path is never read.
    let relative = TokenEnv {
        hf_token_path: Some("t/path".into()),
        ..blank
    };
    assert_eq!(token(&relative, |_| Some("x".into())), None);
}

#[test]
fn the_default_token_file_is_under_the_home_cache() {
    let env = TokenEnv {
        hf_token: None,
        hf_token_path: None,
        hf_home: None,
        xdg_cache_home: None,
        home: Some("/h".into()),
    };
    let read = |p: &Path| (p == Path::new("/h/.cache/huggingface/token")).then(|| "t".to_owned());
    assert_eq!(token(&env, read).as_deref(), Some("t"));
}

#[test]
fn the_cache_paths_follow_the_hub_layout() {
    let cache = RepoCache::new(Path::new("/hub"), "o/r");
    assert_eq!(cache.dir(), Path::new("/hub/models--o--r"));
    assert_eq!(
        cache.blob(&oid(1)),
        PathBuf::from(format!("/hub/models--o--r/blobs/{}", oid(1)))
    );
    assert_eq!(
        cache.incomplete(&oid(1)),
        PathBuf::from(format!("/hub/models--o--r/blobs/{}.incomplete", oid(1)))
    );
    assert_eq!(
        cache.snapshot(COMMIT, "sub/a.gguf"),
        PathBuf::from(format!("/hub/models--o--r/snapshots/{COMMIT}/sub/a.gguf"))
    );
    assert_eq!(cache.refs_main(), Path::new("/hub/models--o--r/refs/main"));
    assert_eq!(
        link_target("a.gguf", &oid(1)),
        PathBuf::from(format!("../../blobs/{}", oid(1)))
    );
    assert_eq!(
        link_target("sub/a.gguf", &oid(1)),
        PathBuf::from(format!("../../../blobs/{}", oid(1)))
    );
}

// Spec (model-download, Connect a Hugging Face account): the token file is
// the one of the Token lookup.
#[test]
fn the_token_file_follows_the_lookup_and_ignores_hf_token() {
    let mut env = TokenEnv {
        hf_token: Some("t0".into()),
        hf_token_path: Some("/t/path".into()),
        hf_home: Some("/hf".into()),
        xdg_cache_home: Some("/c".into()),
        home: Some("/h".into()),
    };
    assert_eq!(token_file(&env), Some(PathBuf::from("/t/path")));
    env.hf_token_path = Some("relative".into());
    assert_eq!(token_file(&env), Some(PathBuf::from("/hf/token")));
    env.hf_home = None;
    assert_eq!(
        token_file(&env),
        Some(PathBuf::from("/c/huggingface/token"))
    );
    env.xdg_cache_home = None;
    assert_eq!(
        token_file(&env),
        Some(PathBuf::from("/h/.cache/huggingface/token"))
    );
    env.home = Some("h".into());
    assert_eq!(token_file(&env), None, "only absolute paths count");
}

#[test]
fn the_saved_token_is_the_file_whatever_hf_token_says() {
    let env = TokenEnv {
        hf_token: Some("t0".into()),
        hf_token_path: None,
        hf_home: Some("/hf".into()),
        xdg_cache_home: None,
        home: None,
    };
    let read = |p: &Path| (p == Path::new("/hf/token")).then(|| " t2\n".to_owned());
    assert_eq!(saved_token(&env, read).as_deref(), Some("t2"));
    assert_eq!(token(&env, read).as_deref(), Some("t0"));
    assert_eq!(saved_token(&env, |_| Some("  ".into())), None);
}

#[test]
fn a_whoami_reply_gives_the_account_name() {
    assert_eq!(
        parse_whoami(r#"{"type":"user","name":"ana","orgs":[]}"#),
        Ok("ana".to_owned())
    );
    for bad in ["{}", r#"{"name":""}"#, r#"{"name":3}"#, "[]", "nope"] {
        assert_eq!(parse_whoami(bad), Err(UNREADABLE.to_owned()), "{bad}");
    }
}

// Spec (Connect a Hugging Face account): a 401 reads as a refused token;
// anything else as the failure of Hub requests.
#[test]
fn whoami_failures_read_by_status() {
    assert_eq!(whoami_message(401), REJECTED);
    assert_eq!(REJECTED, "Hugging Face did not accept the token");
    assert_eq!(whoami_message(500), "Hugging Face request failed: HTTP 500");
}

#[test]
fn a_secret_is_never_shown_by_debug() {
    let mut secret = Secret::new("hf_abc".into());
    let shown = format!("{secret:?} {:?}", AccountAsk::Connect(secret.clone()));
    assert!(!shown.contains("hf_abc"), "{shown}");
    assert_eq!(secret.chars(), 6);
    secret.clear();
    assert_eq!((secret.expose(), secret.chars()), ("", 0));
}

// Spec (hf-popular S2): a model is sized from its parameters, about Q4_K_M,
// and fits when it takes at most 70% of the memory.
#[test]
fn a_model_is_sized_from_its_parameters_and_fits_in_seventy_percent_of_the_memory() {
    assert_eq!(model_bytes(8_000_000_000), 5_000_000_000);
    assert_eq!(model_bytes(u64::MAX), u64::MAX / 8 * 5);
    let ram = 8 * GB;
    assert!(fits(5_600_000_000, ram));
    assert!(!fits(5_600_000_001, ram));
    assert!(!fits(model_bytes(70_000_000_000), ram));
}
