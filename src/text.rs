use std::collections::HashMap;

pub fn apply_replacements(text: &str, replacements: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    for (from, to) in replacements {
        let mut i = 0;
        while let Some(pos) = result[i..].to_lowercase().find(&from.to_lowercase()) {
            let abs_pos = i + pos;
            result.replace_range(abs_pos..abs_pos + from.len(), to);
            i = abs_pos + to.len();
        }
    }
    result
}
