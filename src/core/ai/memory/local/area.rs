//! The area of a note: the folder of the chat, relative to the work root.

/// `cwd` relative to `root`, `/`-joined (`.` for the root itself). Written
/// over text so a Windows path (`C:\repo\frontend`) works anywhere; case is
/// ignored when matching the root. `None` when `cwd` is not below `root` or
/// holds a `..` or a drive prefix after it.
pub fn area_of(root: &str, cwd: &str) -> Option<String> {
    let (root, cwd) = (parts(root), parts(cwd));
    if cwd.len() < root.len()
        || !root
            .iter()
            .zip(&cwd)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    {
        return None;
    }
    let rest = &cwd[root.len()..];
    let drive = |part: &&str| part.len() == 2 && part.ends_with(':');
    if rest.iter().any(|part| *part == ".." || drive(part)) {
        return None;
    }
    Some(if rest.is_empty() {
        ".".to_owned()
    } else {
        rest.join("/")
    })
}

/// The meaningful parts of a path: split on `/` and `\`, without the
/// verbatim prefix, empty parts or `.`.
fn parts(path: &str) -> Vec<&str> {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_a_dot_and_a_subfolder_is_its_relative_path() {
        assert_eq!(area_of("/r", "/r").as_deref(), Some("."));
        assert_eq!(
            area_of("/r", "/r/backend/api").as_deref(),
            Some("backend/api")
        );
        assert_eq!(area_of("/r/", "/r/./a//b/").as_deref(), Some("a/b"));
    }

    #[test]
    fn a_windows_path_gives_a_slash_joined_area() {
        assert_eq!(
            area_of(r"C:\repo", r"C:\repo\frontend\src").as_deref(),
            Some("frontend/src")
        );
        assert_eq!(
            area_of(r"\\?\C:\repo", r"c:\REPO\frontend").as_deref(),
            Some("frontend")
        );
    }

    #[test]
    fn a_prefix_a_root_or_a_parent_part_is_rejected() {
        assert_eq!(area_of("/r", "/other/x"), None);
        assert_eq!(area_of("/r/a", "/r"), None);
        assert_eq!(area_of("/r", "/r/../x"), None);
        assert_eq!(area_of("/r", "/r/a/../../x"), None);
        assert_eq!(area_of(r"C:\repo", r"C:\repo\D:\x"), None);
    }
}
