#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileReference {
    pub raw: String,
    pub path: String,
    pub line: Option<u32>,
    pub start_char: usize,
    pub end_char: usize,
}

#[must_use]
pub fn parse_file_references(input: &str) -> Vec<FileReference> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '['
            && let Some((reference, next_index)) = parse_markdown_reference(&chars, i)
        {
            out.push(reference);
            i = next_index;
            continue;
        }

        if chars[i] != '@' {
            i += 1;
            continue;
        }

        if i > 0 && is_identifier_char(chars[i - 1]) {
            i += 1;
            continue;
        }

        let start = i;
        i += 1;
        let path_start = i;
        while i < chars.len() && is_path_char(chars[i]) {
            i += 1;
        }
        if i == path_start {
            continue;
        }

        let path: String = chars[path_start..i].iter().collect();
        if !path.contains('/') && !path.contains('.') {
            continue;
        }

        let mut line = None;
        let mut end = i;
        if i + 1 < chars.len() && chars[i] == ':' && chars[i + 1].is_ascii_digit() {
            let line_start = i + 1;
            let mut j = line_start;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let line_text: String = chars[line_start..j].iter().collect();
            if let Ok(value) = line_text.parse::<u32>() {
                if value > 0 {
                    line = Some(value);
                    end = j;
                    i = j;
                } else {
                    i = j;
                }
            } else {
                i = j;
            }
        }

        let raw: String = chars[start..end].iter().collect();
        out.push(FileReference {
            raw,
            path,
            line,
            start_char: start,
            end_char: end,
        });
    }
    out
}

fn parse_markdown_reference(chars: &[char], start: usize) -> Option<(FileReference, usize)> {
    let mut close_label = start + 1;
    while close_label < chars.len() && chars[close_label] != ']' {
        close_label += 1;
    }
    if close_label + 2 >= chars.len() || chars[close_label + 1] != '(' {
        return None;
    }

    let mut close_target = close_label + 2;
    while close_target < chars.len() && chars[close_target] != ')' {
        close_target += 1;
    }
    if close_target >= chars.len() {
        return None;
    }

    let target_start = close_label + 2;
    let target: String = chars[target_start..close_target].iter().collect();
    let (path, line) = parse_reference_target(target.trim())?;
    let raw: String = chars[start..=close_target].iter().collect();
    Some((
        FileReference {
            raw,
            path,
            line,
            start_char: start,
            end_char: close_target + 1,
        },
        close_target + 1,
    ))
}

fn parse_reference_target(target: &str) -> Option<(String, Option<u32>)> {
    if target.is_empty() {
        return None;
    }

    let mut path_part = target;
    let mut line = None;
    if let Some((base, anchor)) = target.split_once('#') {
        path_part = base;
        let upper = anchor.to_ascii_uppercase();
        if let Some(raw) = upper.strip_prefix('L')
            && let Ok(value) = raw.parse::<u32>()
            && value > 0
        {
            line = Some(value);
        }
    }

    if line.is_none()
        && let Some((base, raw_line)) = split_path_line_suffix(path_part)
        && let Ok(value) = raw_line.parse::<u32>()
        && value > 0
    {
        path_part = base;
        line = Some(value);
    }

    let path = path_part.trim();
    if path.is_empty() || (!path.contains('/') && !path.contains('.')) {
        return None;
    }
    Some((path.to_string(), line))
}

fn split_path_line_suffix(path: &str) -> Option<(&str, &str)> {
    let (base, line) = path.rsplit_once(':')?;
    if line.chars().all(|ch| ch.is_ascii_digit()) {
        Some((base, line))
    } else {
        None
    }
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/')
}

fn is_path_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/')
}

#[cfg(test)]
mod tests {
    use super::parse_file_references;

    #[test]
    fn parses_path_with_line() {
        let refs = parse_file_references("fix @src/tui/app/input.rs:30 now");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "src/tui/app/input.rs");
        assert_eq!(refs[0].line, Some(30));
    }

    #[test]
    fn parses_markdown_link_reference() {
        let refs = parse_file_references(
            "changed [src/tui/app/input.rs](/workspace/parley/src/tui/app/input.rs#L30)",
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/workspace/parley/src/tui/app/input.rs");
        assert_eq!(refs[0].line, Some(30));
    }

    #[test]
    fn ignores_non_paths() {
        let refs = parse_file_references("@reviewer ping @AI resolved");
        assert!(refs.is_empty());
    }
}
