use std::collections::HashSet;

pub fn short_path(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Return the last `depth` non-empty path components joined by `/`.
/// Always returns a relative-style segment string (no leading `/`).
/// Used as a building block by `truncate_path`.
pub fn tail_path(path: &str, depth: usize) -> String {
    if depth == 0 {
        return String::new();
    }
    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        return path.to_string();
    }
    if components.len() <= depth {
        components.join("/")
    } else {
        components[components.len() - depth..].join("/")
    }
}

/// Truncate a display path (which may be absolute like `/usr` or tilded like `~/foo`)
/// to at most the last `depth` components.
///
/// If the path has `depth` or fewer components, the original string is returned
/// unchanged (preserving leading `/` or `~` formatting).
/// Otherwise only the tail is returned.
pub fn truncate_path(path: &str, depth: usize) -> String {
    if depth == 0 {
        return String::new();
    }
    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if components.len() <= depth {
        path.to_string()
    } else {
        tail_path(path, depth)
    }
}

pub fn parse_git_root(stdout: &[u8]) -> Option<String> {
    let root = String::from_utf8_lossy(stdout).trim().to_string();
    if root.is_empty() { None } else { Some(root) }
}

/// Skips wrapper programs (e.g. "sudo") in the skip set.
pub fn extract_program(cmd: &[&str], skip: &HashSet<String>) -> Option<String> {
    for token in cmd {
        let basename = token.rsplit('/').next().unwrap_or(token);
        if basename.is_empty() {
            continue;
        }
        if skip.contains(basename) {
            continue;
        }
        return Some(basename.to_string());
    }
    None
}

pub fn tilde_path(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    let home = home.trim_end_matches('/');
    let prefix = format!("{home}/");
    if let Some(rest) = path.strip_prefix(&prefix) {
        if rest.is_empty() {
            return "~".to_string();
        }
        return format!("~/{rest}");
    }
    if path == home {
        return "~".to_string();
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_path() {
        assert_eq!(short_path("/home/user/Projects/my-project"), "my-project");
        assert_eq!(short_path("/home/user/Projects/my-project/"), "my-project");
        assert_eq!(short_path("/"), "/");
        assert_eq!(short_path("~"), "~");
    }

    #[test]
    fn test_tail_path() {
        // tail_path always produces a relative segment string (no leading / or ~ prefix)
        assert_eq!(tail_path("/home/user/Projects/my-project", 2), "Projects/my-project");
        assert_eq!(tail_path("~/Projects/foo/bar", 1), "bar");
        assert_eq!(tail_path("~", 1), "~");
        assert_eq!(tail_path("/usr", 1), "usr");
        assert_eq!(tail_path("/usr/local/bin", 1), "bin");
        assert_eq!(tail_path("/usr/local/bin", 2), "local/bin");
        assert_eq!(tail_path("/home/user/Projects/my-project", 1), "my-project");
        assert_eq!(tail_path("/", 5), "/"); // empty components case
    }

    #[test]
    fn test_truncate_path() {
        // When <= depth components, returns original unchanged (preserves / and ~)
        assert_eq!(truncate_path("/usr", 1), "/usr");
        assert_eq!(truncate_path("/mnt", 5), "/mnt");
        assert_eq!(truncate_path("/usr/local/bin", 3), "/usr/local/bin");
        assert_eq!(truncate_path("/usr/local/bin", 10), "/usr/local/bin");
        assert_eq!(truncate_path("/", 10), "/");
        assert_eq!(truncate_path("~/foo", 5), "~/foo");
        assert_eq!(truncate_path("~/Projects/my-project", 3), "~/Projects/my-project");

        // When truncating, uses tail (no root prefix)
        assert_eq!(truncate_path("/usr/local/bin", 1), "bin");
        assert_eq!(truncate_path("/usr/local/bin", 2), "local/bin");
        assert_eq!(truncate_path("/home/user/Projects/my-project", 2), "Projects/my-project");
        assert_eq!(truncate_path("~/Projects/foo/bar", 1), "bar");
    }

    #[test]
    fn test_parse_git_root() {
        assert_eq!(parse_git_root(b"/home/user/project\n"), Some("/home/user/project".into()));
        assert_eq!(parse_git_root(b""), None);
    }

    #[test]
    fn test_extract_program() {
        let no_skip = HashSet::new();
        assert_eq!(extract_program(&["nvim", "src/main.rs"], &no_skip), Some("nvim".into()));
        assert_eq!(extract_program(&["/usr/bin/nvim"], &no_skip), Some("nvim".into()));
        assert_eq!(extract_program(&["cargo", "build", "--release"], &no_skip), Some("cargo".into()));
        assert_eq!(extract_program(&[], &no_skip), None);
    }

    #[test]
    fn test_extract_program_skips_wrappers() {
        let skip: HashSet<String> = ["sudo".to_string()].into();
        assert_eq!(extract_program(&["sudo", "nvim", "file.rs"], &skip), Some("nvim".into()));
        assert_eq!(extract_program(&["/usr/bin/sudo", "/usr/bin/nvim"], &skip), Some("nvim".into()));
        assert_eq!(extract_program(&["sudo"], &skip), None);
    }

    #[test]
    fn test_tilde_path() {
        let cases = vec![
            ("/home/user", "/home/user", "~"),
            ("/home/user/", "/home/user", "~"),
            ("/home/user/Projects/foo", "/home/user", "~/Projects/foo"),
            ("/etc/config", "/home/user", "/etc/config"),
            ("/home/user", "", "/home/user"),
            ("/home/username/foo", "/home/user", "/home/username/foo"),
            ("/home/user", "/home/user/", "~"),
            ("/home/user/foo", "/home/user/", "~/foo"),
        ];
        for (path, home, expected) in cases {
            assert_eq!(
                tilde_path(path, home),
                expected,
                "tilde_path({:?}, {:?})",
                path,
                home
            );
        }
    }
}
