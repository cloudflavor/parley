use anyhow::{Context, Result, anyhow};
use git2::{DiffFormat, DiffOptions, Repository};

use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk, DiffLine, DiffLineKind};

pub async fn load_git_diff_head() -> Result<DiffDocument> {
    let text = tokio::task::spawn_blocking(load_diff_text)
        .await
        .context("failed to join git2 diff worker")??;

    parse_unified_diff(&text)
}

fn load_diff_text() -> Result<String> {
    let repo = Repository::discover(".").context("failed to discover git repository")?;
    let head_tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());

    let mut diff_opts = DiffOptions::new();
    diff_opts
        .context_lines(3)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true)
        .include_typechange(true);
    let diff = repo
        .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut diff_opts))
        .context("failed to compute repository diff")?;

    let mut patch_bytes = Vec::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        match line.origin() {
            '+' | '-' | ' ' => patch_bytes.push(line.origin() as u8),
            _ => {}
        }
        patch_bytes.extend_from_slice(line.content());
        true
    })
    .context("failed to render patch text")?;

    String::from_utf8(patch_bytes).context("git2 patch output is not utf-8")
}

pub fn parse_unified_diff(text: &str) -> Result<DiffDocument> {
    let mut files = Vec::new();

    let mut current_file: Option<DiffFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_cursor: u32 = 0;
    let mut new_cursor: u32 = 0;

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            if let Some(hunk) = current_hunk.take()
                && let Some(file) = current_file.as_mut()
            {
                file.hunks.push(hunk);
            }
            if let Some(file) = current_file.take() {
                files.push(file);
            }
            current_file = Some(DiffFile {
                path: String::new(),
                header_lines: vec![line.to_string()],
                hunks: Vec::new(),
            });
            continue;
        }

        if line.starts_with("@@") {
            if current_file.is_none() {
                current_file = Some(DiffFile {
                    path: String::new(),
                    header_lines: Vec::new(),
                    hunks: Vec::new(),
                });
            }

            if let Some(hunk) = current_hunk.take()
                && let Some(file) = current_file.as_mut()
            {
                file.hunks.push(hunk);
            }

            let (old_start, old_count, new_start, new_count) = parse_hunk_header(line)?;
            old_cursor = old_start;
            new_cursor = new_start;

            let mut hunk = DiffHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                header: line.to_string(),
                lines: Vec::new(),
            };
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::HunkHeader,
                old_line: None,
                new_line: None,
                raw: line.to_string(),
                code: line.to_string(),
            });
            current_hunk = Some(hunk);
            continue;
        }

        if let Some(rest) = line.strip_prefix("+++ b/") {
            if let Some(file) = current_file.as_mut() {
                file.path = rest.to_string();
                file.header_lines.push(line.to_string());
            }
            continue;
        }

        if let Some(file) = current_file.as_mut()
            && current_hunk.is_none()
        {
            file.header_lines.push(line.to_string());
            continue;
        }

        if let Some(hunk) = current_hunk.as_mut() {
            let parsed = if let Some(code) = line.strip_prefix('+') {
                let line_value = DiffLine {
                    kind: DiffLineKind::Added,
                    old_line: None,
                    new_line: Some(new_cursor),
                    raw: line.to_string(),
                    code: code.to_string(),
                };
                new_cursor += 1;
                line_value
            } else if let Some(code) = line.strip_prefix('-') {
                let line_value = DiffLine {
                    kind: DiffLineKind::Removed,
                    old_line: Some(old_cursor),
                    new_line: None,
                    raw: line.to_string(),
                    code: code.to_string(),
                };
                old_cursor += 1;
                line_value
            } else if let Some(code) = line.strip_prefix(' ') {
                let line_value = DiffLine {
                    kind: DiffLineKind::Context,
                    old_line: Some(old_cursor),
                    new_line: Some(new_cursor),
                    raw: line.to_string(),
                    code: code.to_string(),
                };
                old_cursor += 1;
                new_cursor += 1;
                line_value
            } else {
                DiffLine {
                    kind: DiffLineKind::Meta,
                    old_line: None,
                    new_line: None,
                    raw: line.to_string(),
                    code: line.to_string(),
                }
            };

            hunk.lines.push(parsed);
        }
    }

    if let Some(hunk) = current_hunk.take()
        && let Some(file) = current_file.as_mut()
    {
        file.hunks.push(hunk);
    }

    if let Some(file) = current_file.take() {
        files.push(file);
    }

    Ok(DiffDocument { files })
}

fn parse_hunk_header(line: &str) -> Result<(u32, u32, u32, u32)> {
    let Some(rest) = line.strip_prefix("@@ -") else {
        return Err(anyhow!("invalid hunk header format: {line}"));
    };
    let Some((left, right_tail)) = rest.split_once(" +") else {
        return Err(anyhow!("invalid hunk header body: {line}"));
    };
    let Some((right, _tail)) = right_tail.split_once(" @@") else {
        return Err(anyhow!("invalid hunk header end: {line}"));
    };

    let (old_start, old_count) = parse_range(left)?;
    let (new_start, new_count) = parse_range(right)?;
    Ok((old_start, old_count, new_start, new_count))
}

fn parse_range(value: &str) -> Result<(u32, u32)> {
    if let Some((start, count)) = value.split_once(',') {
        Ok((start.parse()?, count.parse()?))
    } else {
        Ok((value.parse()?, 1))
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::diff::DiffLineKind;

    use super::parse_unified_diff;

    #[test]
    fn parse_unified_diff_should_parse_added_and_removed_lines_with_numbers() {
        let input = "diff --git a/src/lib.rs b/src/lib.rs\nindex 123..456 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,3 @@\n fn a() {}\n-fn b() {}\n+fn b() {\"x\";}\n+fn c() {}\n";

        let doc = parse_unified_diff(input).expect("diff should parse");

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, "src/lib.rs");
        assert!(
            doc.files[0]
                .header_lines
                .iter()
                .any(|line| line.starts_with("index "))
        );
        assert_eq!(doc.files[0].hunks.len(), 1);
        let hunk = &doc.files[0].hunks[0];
        assert_eq!(hunk.lines[0].kind, DiffLineKind::HunkHeader);
        assert_eq!(hunk.lines[2].kind, DiffLineKind::Removed);
        assert_eq!(hunk.lines[2].old_line, Some(2));
        assert_eq!(hunk.lines[2].new_line, None);
        assert_eq!(hunk.lines[3].kind, DiffLineKind::Added);
        assert_eq!(hunk.lines[3].old_line, None);
        assert_eq!(hunk.lines[3].new_line, Some(2));
    }
}
