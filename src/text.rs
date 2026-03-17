use std::collections::HashMap;

pub fn apply_replacements(text: &str, replacements: &HashMap<String, String>) -> String {
    let mut result = text.to_string();

    for (raw_key, to) in replacements {
        let (substring_mode, from) = match raw_key.strip_prefix('~') {
            Some(rest) => (true, rest),
            None => (false, raw_key.as_str()),
        };

        if from.is_empty() {
            continue;
        }

        let from_lower = from.to_lowercase();
        let mut i = 0usize;

        while i < result.len() {
            let Some(pos) = result[i..].to_lowercase().find(&from_lower) else {
                break;
            };

            let start = i + pos;
            let end = start + from.len();
            if end > result.len() {
                break;
            }

            if substring_mode || is_word_boundary_match(&result, start, end) {
                result.replace_range(start..end, to);
                i = start + to.len();
            } else {
                i = start + 1;
            }
        }
    }

    result
}

fn is_word_boundary_match(text: &str, start: usize, end: usize) -> bool {
    let left_ok = text[..start]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric());

    let right_ok = text[end..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric());

    left_ok && right_ok
}

#[cfg(test)]
mod tests {
    use super::apply_replacements;
    use std::collections::HashMap;

    #[test]
    fn replaces_standalone_word() {
        let mut replacements = HashMap::new();
        replacements.insert("jus".to_string(), "just".to_string());

        let out = apply_replacements("jus", &replacements);
        assert_eq!(out, "just");
    }

    #[test]
    fn does_not_replace_inside_other_words() {
        let mut replacements = HashMap::new();
        replacements.insert("jus".to_string(), "just".to_string());

        let out = apply_replacements("just justt adjust", &replacements);
        assert_eq!(out, "just justt adjust");
    }

    #[test]
    fn handles_justt_and_jus_without_word_corruption() {
        let mut replacements = HashMap::new();
        replacements.insert("jus".to_string(), "just".to_string());
        replacements.insert("justt".to_string(), "just".to_string());

        let out = apply_replacements("jus justt", &replacements);
        assert_eq!(out, "just just");
    }

    #[test]
    fn substring_mode_replaces_inside_words() {
        let mut replacements = HashMap::new();
        replacements.insert("~mpm".to_string(), "npm".to_string());

        let out = apply_replacements("mpmrc", &replacements);
        assert_eq!(out, "npmrc");
    }

    #[test]
    fn substring_mode_replaces_standalone_too() {
        let mut replacements = HashMap::new();
        replacements.insert("~mpm".to_string(), "npm".to_string());

        let out = apply_replacements("use mpm install", &replacements);
        assert_eq!(out, "use npm install");
    }

    #[test]
    fn supports_multi_word_replacement() {
        let mut replacements = HashMap::new();
        replacements.insert("lazy vim".to_string(), "LazyVim".to_string());

        let out = apply_replacements("I use lazy vim.", &replacements);
        assert_eq!(out, "I use LazyVim.");
    }
}
