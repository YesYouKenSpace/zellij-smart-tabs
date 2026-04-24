use std::collections::{BTreeMap, HashMap, HashSet};

const DEFAULT_FORMAT: &str = "{% if short_git_root %} {{ short_git_root }}{% else %}{{ short_dir }}{% endif %}{% if program %}{% if program_substituted %} {{ program }}{% else %}({{ program }}){% endif %}{% endif %}{% if screen_status %}{{ screen_status }}{% endif %}";

#[derive(Debug, Clone)]
pub struct Substitutions {
    pub program: HashMap<String, String>,
}

impl Default for Substitutions {
    fn default() -> Self {
        let program = [
            ("bash", "\u{f489}"),
            ("bun", "\u{e76f}"),
            ("cargo", "\u{e7a8}"),
            ("nvim", "\u{e6ae}"),
            ("vim", "\u{e7c5}"),
            ("claude", "\u{f069}"),
            ("codex", "\u{eac4}"),
            ("docker", "\u{f308}"),
            ("docker-compose", "\u{f308}"),
            ("emacs", "\u{e632}"),
            ("fish", "\u{f489}"),
            ("gh", "\u{f09b}"),
            ("git", "\u{e702}"),
            ("node", "\u{f0399}"),
            ("npm", "\u{e71e}"),
            ("opencode", "\u{eac4}"),
            ("pnpm", "\u{e71e}"),
            ("python", "\u{e73c}"),
            ("python3", "\u{e73c}"),
            ("rg", "\u{f002}"),
            ("ripgrep", "\u{f002}"),
            ("rustc", "\u{e7a8}"),
            ("tmux", "\u{f489}"),
            ("uv", "\u{e73c}"),
            ("yarn", "\u{e6a7}"),
            ("go", "\u{e627}"),
            ("kubectl", "⎈"),
            ("k9s", "⎈"),
            ("helm", "⎈"),
            ("zsh", "\u{f489}"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Self { program }
    }
}

const DEFAULT_SKIP_PROGRAMS: &[&str] = &["sudo"];

pub struct Config {
    pub format: String,
    pub poll_interval: f64,
    pub debounce: f64,
    pub debug: bool,
    pub substitutions: Substitutions,
    pub skip_programs: HashSet<String>,
}

impl Config {
    pub fn from_map(map: &BTreeMap<String, String>) -> Self {
        let format = map
            .get("format")
            .cloned()
            .unwrap_or_else(|| DEFAULT_FORMAT.to_string());

        let poll_interval = map
            .get("poll_interval")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(2.0);

        let debounce = map
            .get("debounce")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(0.2);

        let debug = map
            .get("debug")
            .map(|v| v.trim().to_lowercase() == "true")
            .unwrap_or(false);

        let mut substitutions = Substitutions::default();
        if let Some(raw) = map.get("sub") {
            let user_subs = parse_substitutions(raw);
            substitutions.program.extend(user_subs.program);
        }

        let mut skip_programs: HashSet<String> = DEFAULT_SKIP_PROGRAMS
            .iter()
            .map(|s| s.to_string())
            .collect();
        if let Some(raw) = map.get("skip_programs") {
            if let Ok(doc) = raw.parse::<kdl::KdlDocument>() {
                for node in doc.nodes() {
                    skip_programs.insert(node.name().to_string());
                }
            }
        }

        Self {
            format,
            poll_interval,
            debounce,
            debug,
            substitutions,
            skip_programs,
        }
    }
}

/// Parse the substitutions block from the raw KDL children string.
///
/// Zellij passes nested config blocks as a raw string. For example:
/// ```kdl
/// sub {
///     program {
///         nvim "Editor"
///         claude "AI"
///     }
/// }
/// ```
/// arrives as `"program {\n    nvim \"Editor\"\n    claude \"AI\"\n}"`.
/// Parse user substitutions from KDL. Returns only the user-specified overrides,
/// not defaults — the caller merges these on top of `Substitutions::default()`.
fn parse_substitutions(raw: &str) -> Substitutions {
    let mut subs = Substitutions {
        program: HashMap::new(),
    };
    let doc = match raw.parse::<kdl::KdlDocument>() {
        Ok(doc) => doc,
        Err(_) => return subs,
    };

    if let Some(node) = doc.get("program") {
        if let Some(children) = node.children() {
            for child in children.nodes() {
                let key = child.name().to_string();
                if let Some(value) = child.entries().first().and_then(|e| e.value().as_string()) {
                    subs.program.insert(key, value.to_string());
                }
            }
        }
    }

    subs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(entries: &[(&str, &str)]) -> Config {
        let map: BTreeMap<String, String> = entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_map(&map)
    }

    #[test]
    fn test_defaults() {
        let c = Config::from_map(&BTreeMap::new());
        assert!(c.format.contains("short_git_root"));
        assert!(c.format.contains(""));
        assert!(c.format.contains("short_dir"));
        assert!(c.format.contains("program_substituted"));
        assert_eq!(c.poll_interval, 2.0);
        assert_eq!(c.debounce, 0.2);
        assert!(!c.debug);
        // Default substitutions are populated
        let defaults = Substitutions::default();
        assert_eq!(
            c.substitutions.program.get("nvim"),
            defaults.program.get("nvim")
        );
        assert_eq!(
            c.substitutions.program.get("claude"),
            defaults.program.get("claude")
        );
    }

    #[test]
    fn test_common_default_program_substitutions() {
        let defaults = Substitutions::default();
        for program in [
            "bash", "bun", "cargo", "docker", "gh", "git", "kubectl", "node", "npm", "opencode",
            "pnpm", "python", "python3", "rg", "rustc", "tmux", "uv", "yarn", "zsh",
        ] {
            assert!(
                defaults.program.contains_key(program),
                "missing default substitution for {program}"
            );
        }
    }

    #[test]
    fn test_default_format_uses_spaced_substituted_program() {
        let c = Config::from_map(&BTreeMap::new());
        let ctx = minijinja::Value::from_serialize(&serde_json::json!({
            "short_dir": "project",
            "program": "",
            "program_substituted": true,
            "screen_status": "",
        }));

        assert_eq!(crate::template::render(&c.format, &ctx), "project ");
    }

    #[test]
    fn test_default_format_wraps_raw_program_without_space() {
        let c = Config::from_map(&BTreeMap::new());
        let ctx = minijinja::Value::from_serialize(&serde_json::json!({
            "short_dir": "project",
            "program": "custom-cli",
            "program_substituted": false,
            "screen_status": "",
        }));

        assert_eq!(
            crate::template::render(&c.format, &ctx),
            "project(custom-cli)"
        );
    }

    #[test]
    fn test_custom_format() {
        let c = config_with(&[("format", "{{ short_dir }}")]);
        assert_eq!(c.format, "{{ short_dir }}");
    }

    #[test]
    fn test_custom_values() {
        let c = config_with(&[
            ("format", "{{ short_dir }} ({{ short_git_root }})"),
            ("poll_interval", "10"),
            ("debug", "true"),
        ]);
        assert_eq!(c.format, "{{ short_dir }} ({{ short_git_root }})");
        assert_eq!(c.poll_interval, 10.0);
        assert!(c.debug);
    }

    #[test]
    fn test_invalid_poll_interval_uses_default() {
        let c = config_with(&[("poll_interval", "not_a_number")]);
        assert_eq!(c.poll_interval, 2.0);
    }

    #[test]
    fn test_substitutions_parsed() {
        let raw = r#"program {
    nvim "Editor"
    claude "AI"
    opencode "AI"
}"#;
        let c = config_with(&[("sub", raw)]);
        assert_eq!(c.substitutions.program.get("nvim"), Some(&"Editor".into()));
        assert_eq!(c.substitutions.program.get("claude"), Some(&"AI".into()));
        assert_eq!(c.substitutions.program.get("opencode"), Some(&"AI".into()));
    }

    #[test]
    fn test_defaults_present_without_sub_config() {
        let c = config_with(&[("format", "{{ short_dir }}")]);
        let defaults = Substitutions::default();
        assert_eq!(
            c.substitutions.program.get("nvim"),
            defaults.program.get("nvim")
        );
    }

    #[test]
    fn test_user_subs_override_defaults() {
        let raw = r#"program {
    nvim "Editor"
}"#;
        let c = config_with(&[("sub", raw)]);
        assert_eq!(c.substitutions.program.get("nvim"), Some(&"Editor".into()));
        // Defaults still present for non-overridden keys
        let defaults = Substitutions::default();
        assert_eq!(
            c.substitutions.program.get("claude"),
            defaults.program.get("claude")
        );
    }

    #[test]
    fn test_skip_programs_defaults() {
        let c = Config::from_map(&BTreeMap::new());
        assert!(c.skip_programs.contains("sudo"));
    }

    #[test]
    fn test_skip_programs_kdl() {
        let raw = "doas\nnohup";
        let c = config_with(&[("skip_programs", raw)]);
        assert!(c.skip_programs.contains("sudo")); // default preserved
        assert!(c.skip_programs.contains("doas"));
        assert!(c.skip_programs.contains("nohup"));
    }

    #[test]
    fn test_substitutions_invalid_kdl_keeps_defaults() {
        let c = config_with(&[("sub", "not valid kdl {{{")]);
        let defaults = Substitutions::default();
        assert_eq!(
            c.substitutions.program.get("nvim"),
            defaults.program.get("nvim")
        );
    }
}
