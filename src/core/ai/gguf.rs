//! Where llama.cpp models live on this machine: the folders to look in and
//! which files there are models. Pure; the runtime reads the folders.

use std::path::{Path, PathBuf};

/// The environment the model folders depend on. Every path must be
/// absolute to count: a relative one would be read from wherever lazarobox
/// was started.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DirsEnv {
    pub home: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    /// `$LLAMA_CACHE`: where `llama-server -hf` downloads to.
    pub llama_cache: Option<PathBuf>,
    /// `$HF_HUB_CACHE`, then `$HF_HOME/hub`: the Hugging Face cache.
    pub hf_hub_cache: Option<PathBuf>,
    pub hf_home: Option<PathBuf>,
    /// On macOS llama.cpp keeps its cache in `~/Library/Caches/llama.cpp`.
    pub macos: bool,
}

/// The folders to look for models in, in this order: the llama.cpp models
/// folder of the data home, llama.cpp's download cache and the Hugging Face
/// hub cache. A folder that cannot be made absolute is left out.
pub fn model_dirs(env: &DirsEnv) -> Vec<PathBuf> {
    let absolute = |p: &Option<PathBuf>| p.clone().filter(|p| p.is_absolute());
    let home = absolute(&env.home);
    let under_home = |rel: &str| home.as_ref().map(|h| h.join(rel));
    let data = absolute(&env.xdg_data_home).or_else(|| under_home(".local/share"));
    let cache = absolute(&env.xdg_cache_home).or_else(|| under_home(".cache"));
    let mut dirs = vec![
        data.map(|d| d.join("llama.cpp/models")),
        absolute(&env.llama_cache).or_else(|| cache.as_ref().map(|c| c.join("llama.cpp"))),
    ];
    if env.macos {
        dirs.push(under_home("Library/Caches/llama.cpp"));
    }
    dirs.push(hub_dir(env));
    dirs.into_iter().flatten().collect()
}

/// The Hugging Face hub cache: `$HF_HUB_CACHE`, else `$HF_HOME/hub`, else
/// `huggingface/hub` in the cache home. Downloads go there (model-download)
/// and the model scan reads it.
pub fn hub_dir(env: &DirsEnv) -> Option<PathBuf> {
    let absolute = |p: &Option<PathBuf>| p.clone().filter(|p| p.is_absolute());
    absolute(&env.hf_hub_cache)
        .or_else(|| absolute(&env.hf_home).map(|h| h.join("hub")))
        .or_else(|| {
            absolute(&env.xdg_cache_home)
                .or_else(|| absolute(&env.home).map(|h| h.join(".cache")))
                .map(|c| c.join("huggingface/hub"))
        })
}

/// Whether a file named `name` is a model llama-server can load: a `.gguf`
/// that is not a vision projector (`mmproj`) and, of a split model
/// (`-00001-of-00003.gguf`), only the first part, which loads the rest.
pub fn is_model_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".gguf") else {
        return false;
    };
    if stem.is_empty() || stem.contains("mmproj") {
        return false;
    }
    match split_part(stem) {
        Some(part) => part == 1,
        None => true,
    }
}

/// The part number of a split model's stem `<name>-NNNNN-of-NNNNN`.
pub(crate) fn split_part(stem: &str) -> Option<u32> {
    let (rest, total) = stem.rsplit_once("-of-")?;
    let (_, part) = rest.rsplit_once('-')?;
    let digits = |s: &str| s.len() == 5 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(part) || !digits(total) {
        return None;
    }
    part.parse().ok()
}

/// `found` without repeats, sorted by file name without letter case, then by
/// path.
pub fn order(mut found: Vec<PathBuf>) -> Vec<PathBuf> {
    found.sort_by(|a, b| {
        label(a)
            .to_lowercase()
            .cmp(&label(b).to_lowercase())
            .then_with(|| a.cmp(b))
    });
    found.dedup();
    found
}

/// What the picker shows for a model: its file name.
pub fn label(model: &Path) -> String {
    model.file_name().map_or_else(
        || model.display().to_string(),
        |f| f.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests;
