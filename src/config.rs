use std::collections::{BTreeMap, HashMap};

const DEFAULT_FORMAT: &str = "{% if short_git_root %}{{ short_git_root }}{% else %}{{ short_dir }}{% endif %}{% if program %}\u{eab6} {{ program }}{% endif %} | {% if status %}{{ status }}{{% else %}} {% endif %}";

#[derive(Debug, Clone)]
pub struct Substitutions {
    pub program: HashMap<String, String>,
    pub status: HashMap<String, String>,
}

impl Default for Substitutions {
    fn default() -> Self {
        let program = [
            ("nvim", "\u{e6ae}"),
            ("vim", "\u{37c5}"),
            ("claude", "\u{f069}"),
            ("node", "\u{f0399}"),
            ("zsh", "\u{f489}"),
            ("go", "\u{e627}"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let status = [
            ("idle", ""),
            ("running", "\u{f110}"),
            ("pending", "\u{f009a}"),
            ("done", "\u{f05d}"),
            ("error", "\u{ea87}"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Self { program, status }
    }
}

pub struct Config {
    pub format: String,
    pub poll_interval: f64,
    pub debounce: f64,
    pub debug: bool,
    pub substitutions: Substitutions,
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
            .unwrap_or(5.0);

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
            substitutions.status.extend(user_subs.status);
        }

        Self {
            format,
            poll_interval,
            debounce,
            debug,
            substitutions,
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
        status: HashMap::new(),
    };
    let doc = match raw.parse::<kdl::KdlDocument>() {
        Ok(doc) => doc,
        Err(_) => return subs,
    };

    for (section_name, map) in [("program", &mut subs.program), ("status", &mut subs.status)] {
        if let Some(node) = doc.get(section_name) {
            if let Some(children) = node.children() {
                for child in children.nodes() {
                    let key = child.name().to_string();
                    if let Some(value) = child.entries().first().and_then(|e| e.value().as_string())
                    {
                        map.insert(key, value.to_string());
                    }
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
        assert!(c.format.contains("short_dir"));
        assert_eq!(c.poll_interval, 5.0);
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
        assert_eq!(c.poll_interval, 5.0);
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
    fn test_substitutions_invalid_kdl_keeps_defaults() {
        let c = config_with(&[("sub", "not valid kdl {{{")]);
        let defaults = Substitutions::default();
        assert_eq!(
            c.substitutions.program.get("nvim"),
            defaults.program.get("nvim")
        );
    }
}
