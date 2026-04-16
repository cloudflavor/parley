use anyhow::{Context, Result};
use git2::{ErrorCode, Repository};

const FALLBACK_REVIEW_NAME: &str = "review";
const DETACHED_PREFIX: &str = "detached-";

pub fn resolve_tui_review_name(explicit: Option<&str>) -> Result<String> {
    if let Some(value) = explicit {
        return Ok(normalize_review_name(value));
    }

    let repo = Repository::discover(".").context("failed to discover git repository")?;
    let raw = detect_head_name(&repo).unwrap_or_else(|| FALLBACK_REVIEW_NAME.to_string());
    Ok(normalize_review_name(&raw))
}

fn detect_head_name(repo: &Repository) -> Option<String> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(error) if error.code() == ErrorCode::UnbornBranch => {
            return Some(FALLBACK_REVIEW_NAME.to_string());
        }
        Err(_) => return None,
    };

    if let Some(shorthand) = head.shorthand()
        && shorthand != "HEAD"
    {
        return Some(shorthand.to_string());
    }

    if repo.head_detached().unwrap_or(false) {
        if let Some(oid) = head.target() {
            let oid = oid.to_string();
            let short = oid.chars().take(12).collect::<String>();
            return Some(format!("{DETACHED_PREFIX}{short}"));
        }
        return Some(format!("{DETACHED_PREFIX}unknown"));
    }

    None
}

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

    let trimmed = output
        .trim_matches(|ch| matches!(ch, '_' | '.'))
        .to_string();
    if trimmed.is_empty() {
        return FALLBACK_REVIEW_NAME.to_string();
    }

    trimmed
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
    fn normalize_review_name_should_fallback_when_empty() {
        assert_eq!(normalize_review_name("////"), "review");
    }
}
