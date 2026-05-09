#[must_use]
pub fn normalize_review_name(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut previous_was_separator = false;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            output.push(ch);
            previous_was_separator = false;
            continue;
        }

        if !previous_was_separator && !output.is_empty() {
            output.push('_');
            previous_was_separator = true;
        }
    }

    output
        .trim_matches(|ch| matches!(ch, '_' | '.'))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_review_name;

    #[test]
    fn normalize_review_name_should_replace_invalid_chars() {
        assert_eq!(
            normalize_review_name("feature/ai-review"),
            "feature_ai-review"
        );
    }

    #[test]
    fn normalize_review_name_should_collapse_separator_runs() {
        assert_eq!(normalize_review_name("foo///bar"), "foo_bar");
    }

    #[test]
    fn normalize_review_name_should_trim_leading_and_trailing_markers() {
        assert_eq!(normalize_review_name("__foo.bar__"), "foo.bar");
    }

    #[test]
    fn normalize_review_name_should_return_empty_when_no_name_remains() {
        assert_eq!(normalize_review_name("////"), "");
    }
}
