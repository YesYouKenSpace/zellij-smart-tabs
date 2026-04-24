use minijinja::{Environment, Value};

/// Render a tab name from a format string and a context value.
pub fn render(format: &str, context: &Value) -> String {
    let env = Environment::new();
    env.render_str(format, context).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::context;

    #[test]
    fn test_simple_variable() {
        let ctx = context! { short_dir => "my-project" };
        assert_eq!(render("{{ short_dir }}", &ctx), "my-project");
    }

    #[test]
    fn test_nested_pane_access() {
        let panes = vec![
            serde_json::json!({"short_dir": "my-project", "program": "nvim"}),
            serde_json::json!({"short_dir": "other-dir"}),
        ];
        let ctx = context! {
            short_dir => "my-project",
            pane => Value::from_serialize(&panes)
        };
        assert_eq!(render("{{ pane[0].program }}", &ctx), "nvim");
        assert_eq!(render("{{ pane[1].short_dir }}", &ctx), "other-dir");
        assert_eq!(render("{{ pane[-1].short_dir }}", &ctx), "other-dir");
    }

    #[test]
    fn test_conditional() {
        let ctx = context! { short_dir => "my-project", short_git_root => "my-repo" };
        let format =
            "{% if short_git_root %}{{ short_git_root }}{% else %}{{ short_dir }}{% endif %}";
        assert_eq!(render(format, &ctx), "my-repo");
    }

    #[test]
    fn test_undefined_is_falsy() {
        let ctx = context! { short_dir => "my-project" };
        let format =
            "{% if short_git_root %}{{ short_git_root }}{% else %}{{ short_dir }}{% endif %}";
        assert_eq!(render(format, &ctx), "my-project");
    }

    #[test]
    fn test_array_pane_access() {
        let panes = vec![
            serde_json::json!({"short_dir": "first", "program": "nvim"}),
            serde_json::json!({"short_dir": "second"}),
            serde_json::json!({"short_dir": "third"}),
        ];
        let ctx = context! {
            short_dir => "first",
            pane => Value::from_serialize(&panes)
        };
        // Positive indexing
        assert_eq!(render("{{ pane[0].short_dir }}", &ctx), "first");
        assert_eq!(render("{{ pane[1].short_dir }}", &ctx), "second");
        assert_eq!(render("{{ pane[0].program }}", &ctx), "nvim");
        // Negative indexing
        assert_eq!(render("{{ pane[-1].short_dir }}", &ctx), "third");
        assert_eq!(render("{{ pane[-2].short_dir }}", &ctx), "second");
        // Out of bounds
        assert_eq!(render("{{ pane[99].short_dir }}", &ctx), "");
    }

    #[test]
    fn test_undefined_pane_index() {
        let panes = vec![serde_json::json!({"short_dir": "my-project"})];
        let ctx = context! {
            short_dir => "my-project",
            pane => Value::from_serialize(&panes)
        };
        assert_eq!(render("{{ pane[5].short_dir }}", &ctx), "");
    }
}
