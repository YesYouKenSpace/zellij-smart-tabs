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
    if root.is_empty() { None } else { Some(root) }
}

/// Extract program name from a pane's terminal_command.
/// Only reliable for command panes — regular terminal panes have terminal_command = None.
pub fn extract_program(terminal_command: Option<&str>) -> Option<String> {
    terminal_command.and_then(parse_program_name)
}

fn parse_program_name(cmd: &str) -> Option<String> {
    let first_token = cmd.split_whitespace().next()?;
    let basename = first_token.rsplit('/').next().unwrap_or(first_token);
    if basename.is_empty() { None } else { Some(basename.to_string()) }
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
        assert_eq!(parse_git_root(b"/home/user/project\n"), Some("/home/user/project".into()));
        assert_eq!(parse_git_root(b""), None);
    }

    #[test]
    fn test_extract_program() {
        assert_eq!(extract_program(Some("nvim src/main.rs")), Some("nvim".into()));
        assert_eq!(extract_program(Some("/usr/bin/nvim")), Some("nvim".into()));
        assert_eq!(extract_program(Some("cargo build --release")), Some("cargo".into()));
        assert_eq!(extract_program(None), None);
    }

    #[test]
    fn test_extract_program_from_cmd_vec() {
        let cmd = vec!["nvim".to_string(), "src/main.rs".to_string()];
        assert_eq!(extract_program(cmd.first().map(|s| s.as_str())), Some("nvim".into()));
        let cmd2 = vec!["/usr/bin/nvim".to_string()];
        assert_eq!(extract_program(cmd2.first().map(|s| s.as_str())), Some("nvim".into()));
        let empty: Vec<String> = vec![];
        assert_eq!(extract_program(empty.first().map(|s| s.as_str())), None);
    }
}
