use std::collections::HashSet;

pub fn short_path(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
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
