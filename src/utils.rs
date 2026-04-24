use std::collections::HashSet;

/// Extract the last path component from a full path.
pub fn short_path(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Parse git root from `git rev-parse --show-toplevel` output.
pub fn parse_git_root(stdout: &[u8]) -> Option<String> {
    let root = String::from_utf8_lossy(stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(root)
    }
}

/// Extract program name from a command, skipping wrapper programs (e.g. "sudo").
/// Iterates tokens, strips path prefixes, skips any in the skip set, returns first match.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_path() {
        assert_eq!(short_path("/home/user/Projects/my-project"), "my-project");
        assert_eq!(short_path("/home/user/Projects/my-project/"), "my-project");
        assert_eq!(short_path("/"), "/");
    }

    #[test]
    fn test_parse_git_root() {
        assert_eq!(
            parse_git_root(b"/home/user/project\n"),
            Some("/home/user/project".into())
        );
        assert_eq!(parse_git_root(b""), None);
    }

    #[test]
    fn test_extract_program() {
        let no_skip = HashSet::new();
        assert_eq!(
            extract_program(&["nvim", "src/main.rs"], &no_skip),
            Some("nvim".into())
        );
        assert_eq!(
            extract_program(&["/usr/bin/nvim"], &no_skip),
            Some("nvim".into())
        );
        assert_eq!(
            extract_program(&["cargo", "build", "--release"], &no_skip),
            Some("cargo".into())
        );
        assert_eq!(extract_program(&[], &no_skip), None);
    }

    #[test]
    fn test_extract_program_skips_wrappers() {
        let skip: HashSet<String> = ["sudo".to_string()].into();
        assert_eq!(
            extract_program(&["sudo", "nvim", "file.rs"], &skip),
            Some("nvim".into())
        );
        assert_eq!(
            extract_program(&["/usr/bin/sudo", "/usr/bin/nvim"], &skip),
            Some("nvim".into())
        );
        assert_eq!(extract_program(&["sudo"], &skip), None);
    }
}
