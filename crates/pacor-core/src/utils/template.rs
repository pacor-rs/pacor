use std::collections::HashMap;

use anyhow::{Result, anyhow};

/// Perform simple placeholder replacement in a template string.
///
/// Placeholders must be in the format `{{key}}`.
///
/// # Errors
/// Returns an error if any `{{...}}` placeholder remains unreplaced in the output,
/// which usually means a key was missing from `variables`. Empty values are
/// allowed and render as an empty string (e.g. an unset publisher).
pub fn render_template(template: &str, variables: &HashMap<&str, &str>) -> Result<String> {
    let mut result = template.to_string();

    for (key, value) in variables {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }

    let unreplaced = find_unreplaced_placeholders(&result);
    if !unreplaced.is_empty() {
        return Err(anyhow!(
            "unreplaced placeholders remaining in template: {}",
            unreplaced.join(", ")
        ));
    }

    Ok(result)
}

fn find_unreplaced_placeholders(rendered: &str) -> Vec<String> {
    let mut unreplaced = Vec::new();
    let mut chars = rendered.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next();
            let mut key = String::new();
            let mut found_end = false;
            while let Some(kc) = chars.next() {
                if kc == '}' && chars.peek() == Some(&'}') {
                    chars.next();
                    found_end = true;
                    break;
                }
                key.push(kc);
            }
            if found_end && !key.is_empty() {
                unreplaced.push(key);
            }
        }
    }

    unreplaced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_simple_replacement() {
        let mut vars = HashMap::new();
        vars.insert("NAME", "world");
        let result = render_template("Hello {{NAME}}!", &vars).unwrap();
        assert_eq!(result, "Hello world!");
    }

    #[test]
    fn test_render_multiple_replacements() {
        let mut vars = HashMap::new();
        vars.insert("A", "1");
        vars.insert("B", "2");
        let result = render_template("{{A}}+{{B}}=3", &vars).unwrap();
        assert_eq!(result, "1+2=3");
    }

    #[test]
    fn test_render_repeated_placeholder() {
        let mut vars = HashMap::new();
        vars.insert("X", "ok");
        let result = render_template("{{X}} and {{X}}", &vars).unwrap();
        assert_eq!(result, "ok and ok");
    }

    #[test]
    fn test_render_empty_value_is_allowed() {
        let mut vars = HashMap::new();
        vars.insert("K", "");
        let result = render_template("[{{K}}]", &vars).unwrap();
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_render_unreplaced_placeholder_error() {
        let vars = HashMap::new();
        let result = render_template("{{UNKNOWN}}", &vars);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unreplaced"));
    }

    #[test]
    fn test_render_no_placeholders() {
        let vars = HashMap::new();
        let result = render_template("plain text", &vars).unwrap();
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_render_partial_unreplaced() {
        let mut vars = HashMap::new();
        vars.insert("A", "1");
        let result = render_template("{{A}} {{MISSING}}", &vars);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("MISSING"));
    }

    #[test]
    fn test_find_unreplaced_empty() {
        assert!(find_unreplaced_placeholders("no placeholders").is_empty());
    }

    #[test]
    fn test_find_unreplaced_single() {
        let result = find_unreplaced_placeholders("{{FOO}}");
        assert_eq!(result, vec!["FOO"]);
    }

    #[test]
    fn test_find_unreplaced_multiple() {
        let result = find_unreplaced_placeholders("{{A}}x{{B}}");
        assert_eq!(result, vec!["A", "B"]);
    }

    #[test]
    fn test_find_unreplaced_ignores_single_braces() {
        let result = find_unreplaced_placeholders("{not} a {{REAL}}");
        assert_eq!(result, vec!["REAL"]);
    }
}
