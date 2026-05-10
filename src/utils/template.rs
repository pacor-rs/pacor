use std::collections::HashMap;

use anyhow::{Result, anyhow};

/// Perform simple placeholder replacement in a template string.
///
/// Placeholders must be in the format `{{key}}`.
pub(crate) fn render_template(template: &str, variables: &HashMap<&str, &str>) -> Result<String> {
    let mut result = template.to_string();

    for (key, value) in variables {
        if value.is_empty() {
            return Err(anyhow!("placeholder value is empty: {}", key));
        }

        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }

    Ok(result)
}
