//! The guided `llama-server` install (llama-server-install), as far as the
//! core needs to know it: which upstream asset fits this machine, which
//! release to take, which archive members and links are safe, how much disk
//! the install needs and when a newer build is worth a notice. Everything
//! here is pure; the runtime downloads, verifies, extracts and activates.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::jail::Os;

/// The build flavour of the upstream archive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Cpu,
    Vulkan,
    /// The macOS build (Metal is built in; the asset has no flavour).
    Metal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arch {
    X64,
    Arm64,
    Other,
}

impl Arch {
    /// The architecture this program was built for.
    pub const CURRENT: Self = if cfg!(target_arch = "x86_64") {
        Self::X64
    } else if cfg!(target_arch = "aarch64") {
        Self::Arm64
    } else {
        Self::Other
    };
}

/// The machine an asset is chosen for. Passed in, so the choice is testable
/// on any system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    /// The machine this program was built for.
    pub const CURRENT: Self = Self {
        os: Os::CURRENT,
        arch: Arch::CURRENT,
    };
}

/// The upstream archive name of build `tag` for `platform` and `variant`;
/// `None` when upstream ships no such build (Windows, Vulkan on macOS, ...).
pub fn asset_name(tag: &str, platform: Platform, variant: Variant) -> Option<String> {
    let arch = match platform.arch {
        Arch::X64 => "x64",
        Arch::Arm64 => "arm64",
        Arch::Other => return None,
    };
    let flavour = match (platform.os, variant) {
        (Os::Linux, Variant::Cpu) => "ubuntu",
        (Os::Linux, Variant::Vulkan) => "ubuntu-vulkan",
        (Os::Macos, Variant::Cpu | Variant::Metal) => "macos",
        _ => return None,
    };
    Some(format!("llama-{tag}-bin-{flavour}-{arch}.tar.gz"))
}

/// The variants the plan offers, best first: the CPU build always (when the
/// platform has one), Vulkan only on Linux with an ICD, Metal on macOS.
pub fn variants(platform: Platform, has_vulkan_icd: bool) -> Vec<Variant> {
    let mut offered = Vec::new();
    if platform.os == Os::Macos && asset_name("b0", platform, Variant::Metal).is_some() {
        offered.push(Variant::Metal);
    }
    if platform.os == Os::Linux && asset_name("b0", platform, Variant::Cpu).is_some() {
        offered.push(Variant::Cpu);
        if has_vulkan_icd && asset_name("b0", platform, Variant::Vulkan).is_some() {
            offered.push(Variant::Vulkan);
        }
    }
    offered
}

/// A release to install: the tag, the archive and the digest to hold it to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Release {
    pub tag: String,
    pub asset: String,
    pub size: u64,
    /// Lower-case hex SHA-256 of the archive.
    pub sha256: String,
}

/// What the plan says when no listed release holds the asset.
pub const NO_RELEASE: &str = "no llama.cpp release has a build for this machine";

/// The numeric part of a `b<digits>` tag; `None` for any other tag.
pub fn tag_number(tag: &str) -> Option<u64> {
    let digits = tag.strip_prefix('b')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Picks the newest `b<digits>` release of the GitHub releases list `json`
/// whose archive (`asset_of(tag)`) is attached. Tags compare by number, not
/// text. The chosen release must carry a well-formed `sha256:` digest: a
/// missing or malformed one is an error, never a reason to take an older
/// release.
pub fn pick_release(
    json: &str,
    asset_of: impl Fn(&str) -> Option<String>,
) -> Result<Release, String> {
    let list: Value =
        serde_json::from_str(json).map_err(|_| "GitHub request failed: unreadable reply")?;
    let releases = list
        .as_array()
        .ok_or("GitHub request failed: unreadable reply")?;
    let mut best: Option<(u64, Release)> = None;
    for release in releases {
        let Some(tag) = release.get("tag_name").and_then(Value::as_str) else {
            continue;
        };
        let Some(number) = tag_number(tag) else {
            continue;
        };
        if best.as_ref().is_some_and(|(top, _)| *top >= number) {
            continue;
        }
        let Some(wanted) = asset_of(tag) else {
            continue;
        };
        let asset = release
            .get("assets")
            .and_then(Value::as_array)
            .and_then(|assets| {
                assets
                    .iter()
                    .find(|a| a.get("name").and_then(Value::as_str) == Some(&wanted))
            });
        let Some(asset) = asset else {
            continue;
        };
        let digest = asset.get("digest").and_then(Value::as_str);
        let sha256 = digest
            .and_then(parse_digest)
            .ok_or_else(|| format!("release {tag} has no usable sha256 digest for {wanted}"))?;
        let size = asset.get("size").and_then(Value::as_u64).unwrap_or(0);
        best = Some((
            number,
            Release {
                tag: tag.to_owned(),
                asset: wanted,
                size,
                sha256,
            },
        ));
    }
    best.map(|(_, release)| release)
        .ok_or_else(|| NO_RELEASE.to_owned())
}

/// `sha256:<64 hex>` (hex in any case) as lower-case hex.
fn parse_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// Whether the archive member `name` is acceptable: it is the top directory
/// `top` or lies under it, with no absolute path, no `..` component, no
/// control character and no `\`.
pub fn check_member(name: &str, top: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("unsafe archive member {name:?}: {why}"));
    if name.chars().any(|c| c.is_control() || c == '\\') {
        return bad("control character or backslash");
    }
    if name.starts_with('/') {
        return bad("absolute path");
    }
    if name.split('/').any(|part| part == "..") {
        return bad("`..` component");
    }
    match name.strip_prefix(top) {
        Some("") => Ok(()),
        Some(rest) if rest.starts_with('/') => Ok(()),
        _ => bad("outside the top directory"),
    }
}

/// Whether a symlink may point at `target`: a bare name, so it can only
/// resolve inside its own directory.
pub fn check_link_target(target: &str) -> Result<(), String> {
    let ok = !target.is_empty()
        && target != "."
        && target != ".."
        && !target.contains('/')
        && !target.chars().any(|c| c.is_control() || c == '\\');
    if ok {
        Ok(())
    } else {
        Err(format!("unsafe symlink target {target:?}"))
    }
}

/// How much free space the install needs: five times the archive (the
/// archive plus its extraction), less what a resumed download already has.
pub fn needed_bytes(archive: u64, on_disk: u64) -> u64 {
    archive.saturating_mul(5).saturating_sub(on_disk)
}

/// Whether `candidate` is a newer build than `installed`, by number.
pub fn is_newer(installed: &str, candidate: &str) -> bool {
    match (tag_number(installed), tag_number(candidate)) {
        (Some(have), Some(offered)) => offered > have,
        _ => false,
    }
}

/// Whether the newer-build check may ask GitHub: only with an owned install
/// of a known tag, and never when `[updates] check = false`.
pub fn should_check_newer(update_check: bool, installed_tag: Option<&str>) -> bool {
    update_check && installed_tag.is_some_and(|tag| tag_number(tag).is_some())
}

/// `<data_home>/lazarobox/llama.cpp`: everything the install owns.
pub fn owned_root(data_home: &Path) -> PathBuf {
    data_home.join("lazarobox").join("llama.cpp")
}

/// Whether `binary` lives inside an owned install (`.../lazarobox/llama.cpp/...`),
/// as opposed to a system `llama-server`.
pub fn is_owned(binary: &Path) -> bool {
    let parts: Vec<_> = binary.components().collect();
    parts.windows(2).any(|pair| {
        matches!(pair, [Component::Normal(a), Component::Normal(b)]
            if *a == "lazarobox" && *b == "llama.cpp")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX_X64: Platform = Platform {
        os: Os::Linux,
        arch: Arch::X64,
    };

    fn digest(c: char) -> String {
        format!("sha256:{}", c.to_string().repeat(64))
    }

    fn release(tag: &str, assets: &[(&str, Option<String>)]) -> Value {
        let assets: Vec<Value> = assets
            .iter()
            .map(|(name, digest)| {
                let mut asset = serde_json::json!({ "name": name, "size": 1234 });
                if let Some(d) = digest {
                    asset["digest"] = Value::String(d.clone());
                }
                asset
            })
            .collect();
        serde_json::json!({ "tag_name": tag, "assets": assets })
    }

    fn cpu(tag: &str) -> Option<String> {
        asset_name(tag, LINUX_X64, Variant::Cpu)
    }

    fn with_cpu(tag: &str, d: Option<String>) -> Value {
        release(tag, &[(&cpu(tag).unwrap(), d)])
    }

    fn json_of(releases: Vec<Value>) -> String {
        Value::Array(releases).to_string()
    }

    #[test]
    fn asset_names_follow_the_upstream_scheme() {
        let name = |os, arch, v| asset_name("b9000", Platform { os, arch }, v);
        assert_eq!(
            name(Os::Linux, Arch::X64, Variant::Cpu).as_deref(),
            Some("llama-b9000-bin-ubuntu-x64.tar.gz")
        );
        assert_eq!(
            name(Os::Linux, Arch::X64, Variant::Vulkan).as_deref(),
            Some("llama-b9000-bin-ubuntu-vulkan-x64.tar.gz")
        );
        assert_eq!(
            name(Os::Linux, Arch::Arm64, Variant::Cpu).as_deref(),
            Some("llama-b9000-bin-ubuntu-arm64.tar.gz")
        );
        assert_eq!(
            name(Os::Macos, Arch::Arm64, Variant::Metal).as_deref(),
            Some("llama-b9000-bin-macos-arm64.tar.gz")
        );
        assert_eq!(
            name(Os::Macos, Arch::X64, Variant::Metal).as_deref(),
            Some("llama-b9000-bin-macos-x64.tar.gz")
        );
    }

    #[test]
    fn no_asset_where_upstream_ships_none() {
        let name = |os, arch, v| asset_name("b9000", Platform { os, arch }, v);
        assert_eq!(name(Os::Other, Arch::X64, Variant::Cpu), None);
        assert_eq!(name(Os::Macos, Arch::Arm64, Variant::Vulkan), None);
        assert_eq!(name(Os::Linux, Arch::X64, Variant::Metal), None);
        assert_eq!(name(Os::Linux, Arch::Other, Variant::Cpu), None);
    }

    #[test]
    fn vulkan_is_offered_only_on_linux_with_an_icd() {
        assert_eq!(
            variants(LINUX_X64, true),
            vec![Variant::Cpu, Variant::Vulkan]
        );
        assert_eq!(variants(LINUX_X64, false), vec![Variant::Cpu]);
        let mac = Platform {
            os: Os::Macos,
            arch: Arch::Arm64,
        };
        assert_eq!(variants(mac, true), vec![Variant::Metal]);
    }

    #[test]
    fn an_unsupported_platform_offers_nothing() {
        let windows = Platform {
            os: Os::Other,
            arch: Arch::X64,
        };
        assert!(variants(windows, true).is_empty());
    }

    #[test]
    fn the_newest_tag_with_the_asset_wins() {
        let json = json_of(vec![
            release("v0.7.0", &[]),
            with_cpu("b9000", Some(digest('a'))),
            release("b9010", &[("other.zip", None)]),
            with_cpu("b8990", Some(digest('b'))),
        ]);
        let release = pick_release(&json, cpu).unwrap();
        assert_eq!(release.tag, "b9000");
        assert_eq!(release.asset, "llama-b9000-bin-ubuntu-x64.tar.gz");
        assert_eq!(release.size, 1234);
        assert_eq!(release.sha256, "a".repeat(64));
    }

    #[test]
    fn tags_compare_by_number_not_text() {
        let json = json_of(vec![
            with_cpu("b999", Some(digest('a'))),
            with_cpu("b1000", Some(digest('b'))),
        ]);
        assert_eq!(pick_release(&json, cpu).unwrap().tag, "b1000");
    }

    #[test]
    fn no_usable_release_is_a_readable_error() {
        let json = json_of(vec![release("v0.7.0", &[]), release("b9010", &[])]);
        assert_eq!(pick_release(&json, cpu), Err(NO_RELEASE.to_owned()));
        assert!(pick_release("not json", cpu).is_err());
        assert!(pick_release("{}", cpu).is_err());
    }

    #[test]
    fn a_missing_or_malformed_digest_aborts() {
        for bad in [
            None,
            Some("sha1:abc".to_owned()),
            Some("sha256:abc".to_owned()),
            Some(format!("sha256:{}", "z".repeat(64))),
        ] {
            let json = json_of(vec![with_cpu("b9000", bad.clone())]);
            let err = pick_release(&json, cpu).unwrap_err();
            assert!(err.contains("digest"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn a_bad_digest_never_falls_back_to_an_older_release() {
        let json = json_of(vec![
            with_cpu("b9000", Some(digest('a'))),
            with_cpu("b9100", None),
        ]);
        assert!(pick_release(&json, cpu).is_err());
    }

    #[test]
    fn digest_hex_is_case_insensitive() {
        let json = json_of(vec![with_cpu(
            "b9000",
            Some(format!("sha256:{}", "AB".repeat(32))),
        )]);
        assert_eq!(pick_release(&json, cpu).unwrap().sha256, "ab".repeat(32));
    }

    #[test]
    fn members_under_the_top_directory_pass() {
        let top = "llama-b9000";
        for name in [
            "llama-b9000",
            "llama-b9000/",
            "llama-b9000/llama-server",
            "llama-b9000/lib/libllama.so.0",
        ] {
            assert_eq!(check_member(name, top), Ok(()), "{name}");
        }
    }

    #[test]
    fn unsafe_members_are_rejected() {
        let top = "llama-b9000";
        for name in [
            "llama-b9000/../../evil",
            "../llama-b9000/x",
            "/etc/cron.d/x",
            "llama-b9000/a\nb",
            "llama-b9000/a\\b",
            "llama-b9000/\u{7}",
            "other/llama-server",
            "llama-b90001/llama-server",
            "llama-b9000evil/x",
            "",
        ] {
            assert!(check_member(name, top).is_err(), "{name:?}");
        }
    }

    #[test]
    fn symlinks_may_only_name_a_sibling() {
        assert_eq!(check_link_target("libllama.so.0.7.0"), Ok(()));
        for target in ["", ".", "..", "../x", "/etc/passwd", "a/b", "a\\b", "a\nb"] {
            assert!(check_link_target(target).is_err(), "{target:?}");
        }
    }

    #[test]
    fn the_disk_estimate_is_five_archives_less_the_partial() {
        assert_eq!(needed_bytes(100, 0), 500);
        assert_eq!(needed_bytes(100, 40), 460);
        assert_eq!(needed_bytes(100, 900), 0);
        assert_eq!(needed_bytes(u64::MAX, 0), u64::MAX);
    }

    #[test]
    fn newer_compares_numbers() {
        assert!(is_newer("b999", "b1000"));
        assert!(!is_newer("b1000", "b999"));
        assert!(!is_newer("b1000", "b1000"));
        assert!(!is_newer("b1000", "v0.7.0"));
        assert!(!is_newer("weird", "b1"));
    }

    #[test]
    fn the_newer_check_respects_the_update_opt_out() {
        assert!(should_check_newer(true, Some("b9000")));
        assert!(!should_check_newer(false, Some("b9000")));
        assert!(!should_check_newer(true, None));
        assert!(!should_check_newer(true, Some("weird")));
    }

    #[test]
    fn owned_binaries_are_told_from_system_ones() {
        let root = owned_root(Path::new("/home/u/.local/share"));
        assert_eq!(root, Path::new("/home/u/.local/share/lazarobox/llama.cpp"));
        assert!(is_owned(&root.join("current/llama-server")));
        assert!(!is_owned(Path::new("/usr/bin/llama-server")));
        assert!(!is_owned(Path::new("/opt/lazarobox/other/llama-server")));
    }
}
