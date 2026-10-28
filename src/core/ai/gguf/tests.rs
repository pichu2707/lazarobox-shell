use std::path::{Path, PathBuf};

use super::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn home(home: &str) -> DirsEnv {
    DirsEnv {
        home: Some(p(home)),
        ..DirsEnv::default()
    }
}

// Spec S2: the usual folders, with the defaults of each tool.
#[test]
fn the_usual_folders_are_looked_in_with_their_defaults() {
    assert_eq!(
        model_dirs(&home("/h")),
        vec![
            p("/h/.local/share/llama.cpp/models"),
            p("/h/.cache/llama.cpp"),
            p("/h/.cache/huggingface/hub"),
        ]
    );
}

#[test]
fn each_folder_follows_its_tool_s_variables() {
    let env = DirsEnv {
        home: Some(p("/h")),
        xdg_data_home: Some(p("/data")),
        xdg_cache_home: Some(p("/cache")),
        ..DirsEnv::default()
    };
    assert_eq!(
        model_dirs(&env),
        vec![
            p("/data/llama.cpp/models"),
            p("/cache/llama.cpp"),
            p("/cache/huggingface/hub"),
        ]
    );
    let env = DirsEnv {
        home: Some(p("/h")),
        llama_cache: Some(p("/lc")),
        hf_home: Some(p("/hf")),
        ..DirsEnv::default()
    };
    assert_eq!(
        model_dirs(&env),
        vec![
            p("/h/.local/share/llama.cpp/models"),
            p("/lc"),
            p("/hf/hub"),
        ]
    );
    // `HF_HUB_CACHE` wins over `HF_HOME`.
    let env = DirsEnv {
        hf_hub_cache: Some(p("/hub")),
        hf_home: Some(p("/hf")),
        ..home("/h")
    };
    assert_eq!(model_dirs(&env)[2], p("/hub"));
}

#[test]
fn macos_also_looks_in_the_llama_cpp_cache_of_its_library() {
    let env = DirsEnv {
        macos: true,
        ..home("/Users/u")
    };
    assert_eq!(
        model_dirs(&env),
        vec![
            p("/Users/u/.local/share/llama.cpp/models"),
            p("/Users/u/.cache/llama.cpp"),
            p("/Users/u/Library/Caches/llama.cpp"),
            p("/Users/u/.cache/huggingface/hub"),
        ]
    );
}

#[test]
fn a_relative_or_missing_folder_is_never_looked_in() {
    assert_eq!(model_dirs(&DirsEnv::default()), Vec::<PathBuf>::new());
    let env = DirsEnv {
        home: Some(p("h")),
        xdg_cache_home: Some(p("cache")),
        llama_cache: Some(p("rel")),
        ..DirsEnv::default()
    };
    assert_eq!(model_dirs(&env), Vec::<PathBuf>::new());
    // A relative variable falls back to the home default.
    let env = DirsEnv {
        xdg_cache_home: Some(p("cache")),
        ..home("/h")
    };
    assert_eq!(model_dirs(&env)[1], p("/h/.cache/llama.cpp"));
}

// Spec S2 (assumption): of a split model only the first part is listed.
#[test]
fn only_model_files_are_listed() {
    for name in [
        "qwen2.5-0.5b-instruct-q4_k_m.gguf",
        "Model.GGUF",
        "big-00001-of-00003.gguf",
    ] {
        assert!(is_model_file(name), "{name}");
    }
    for name in [
        "big-00002-of-00003.gguf",
        "mmproj-model-f16.gguf",
        "notes.txt",
        "model.gguf.part",
        ".gguf",
        "model.gguf.downloadInProgress",
    ] {
        assert!(!is_model_file(name), "{name}");
    }
}

#[test]
fn found_models_are_listed_once_by_file_name() {
    let found = vec![
        p("/b/zeta.gguf"),
        p("/a/Alpha.gguf"),
        p("/b/zeta.gguf"),
        p("/a/beta.gguf"),
    ];
    assert_eq!(
        order(found),
        vec![p("/a/Alpha.gguf"), p("/a/beta.gguf"), p("/b/zeta.gguf")]
    );
}

#[test]
fn the_label_of_a_model_is_its_file_name() {
    assert_eq!(label(Path::new("/m/qwen.gguf")), "qwen.gguf");
    assert_eq!(label(Path::new("/")), "/");
}

// Spec (model-download): downloads go to the same hub cache the scan reads.
#[test]
fn the_hub_dir_is_the_last_model_dir() {
    let env = DirsEnv {
        home: Some(PathBuf::from("/h")),
        hf_home: Some(PathBuf::from("/hf")),
        ..DirsEnv::default()
    };
    assert_eq!(hub_dir(&env), Some(PathBuf::from("/hf/hub")));
    assert_eq!(model_dirs(&env).last(), hub_dir(&env).as_ref());
    assert_eq!(hub_dir(&DirsEnv::default()), None);
    let relative = DirsEnv {
        hf_hub_cache: Some(PathBuf::from("hub")),
        ..DirsEnv::default()
    };
    assert_eq!(hub_dir(&relative), None);
}
