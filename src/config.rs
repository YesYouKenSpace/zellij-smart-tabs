use std::collections::{BTreeMap, HashMap, HashSet};
// NOTE: Keep DEFAULT_FORMAT in sync with README.md § Format Gallery "Default" entry
// and test_gallery_formats_render in this file.
const DEFAULT_FORMAT: &str = "{% if short_git_root %}{{ short_git_root }}{% else %}{{ short_dir }}{% endif %}{% if program %}\u{eab6} {{ program }}{% endif %}{% if status %} | {{ status }}{% endif %}";

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

const DEFAULT_SKIP_PROGRAMS: &[&str] = &["sudo"];

pub struct Config {
    pub format: String,
    /// Set when user-provided format failed to compile; contains the error message.
    pub format_error: Option<String>,
    pub poll_interval: f64,
    pub debounce: f64,
    pub debug: bool,
    pub substitutions: Substitutions,
    pub skip_programs: HashSet<String>,
}

impl Config {
    pub fn from_map(map: &BTreeMap<String, String>) -> Self {
        let (format, format_error) = match map.get("format") {
            Some(user_fmt) => match crate::template::validate_format(user_fmt) {
                Ok(()) => (user_fmt.clone(), None),
                Err(e) => (DEFAULT_FORMAT.to_string(), Some(e)),
            },
            None => (DEFAULT_FORMAT.to_string(), None),
        };

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
            format_error,
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
    fn test_invalid_format_falls_back_to_default() {
        let c = config_with(&[("format", "{% if broken")]);
        assert!(c.format_error.is_some());
        assert!(
            c.format.contains("short_git_root"),
            "should fall back to DEFAULT_FORMAT"
        );
    }

    #[test]
    fn test_valid_format_no_error() {
        let c = config_with(&[("format", "{{ short_dir }}")]);
        assert!(c.format_error.is_none());
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
        ]);
        assert_eq!(c.format, "{{ short_dir }} ({{ short_git_root }})");
        assert_eq!(c.poll_interval, 10.0);
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

    /// Verify that all format strings from the README.md "Format Gallery" section
    /// parse and render correctly. If you add or change a format in the gallery,
    /// update this test to match.
    #[test]
    fn test_gallery_formats_render() {
        use crate::template::render;
        use minijinja::{context, Value};

        let ctx = context! {
            short_dir => "my-project",
            short_git_root => "my-repo",
            cwd => "/home/user/Projects/my-project",
            program => "nvim",
            status => "idle",
            pane => vec![
                serde_json::json!({"short_dir": "my-project", "program": "nvim"}),
                serde_json::json!({"short_dir": "docs"}),
            ],
        };
        let ctx_val = Value::from_serialize(&ctx);

        let c = Config::from_map(&BTreeMap::new());

        // NOTE: KEEP IN SYNC with README.md § Format Gallery.
        let gallery = [
            // Default (also DEFAULT_FORMAT)
            (c.format.as_str(), "my-repo"),
            // Minimal
            ("{{ short_dir }}", "my-project"),
            // Full path
            ("{{ cwd }}", "/home/user/Projects/my-project"),
            // Program-first
            ("{% if program %}{{ program }} @ {% endif %}{{ short_dir }}", "nvim @ my-project"),
            // Status indicators
            ("{{ short_dir }}{% if status %} {{ status }}{% endif %}{% if program %} {{ program }}{% endif %}", "my-project"),
            // Bracketed program
            ("{{ short_dir }}{% if program %} [{{ program }}]{% endif %}", "my-project [nvim]"),
            // Multi-pane
            ("{{ short_dir }}{% if pane[1] %} | {{ pane[1].short_dir }}{% endif %}", "my-project | docs"),
        ];

        for (format, expected_substr) in &gallery {
            let result = render(format, &ctx_val);
            assert!(
                !result.is_empty(),
                "gallery format {:?} should render non-empty",
                format
            );
            assert!(
                result.contains(expected_substr),
                "gallery format {:?} rendered {:?}, expected to contain {:?}",
                format,
                result,
                expected_substr
            );
        }

        // Default format should also fallback to short_dir when git root is absent
        let ctx_no_git = context! { short_dir => "fallback-dir" };
        let result = render(&c.format, &Value::from_serialize(&ctx_no_git));
        assert!(
            !result.is_empty(),
            "default format should render without git root"
        );
        assert!(
            result.contains("fallback-dir"),
            "should fallback to short_dir: {}",
            result
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
