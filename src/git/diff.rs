use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use git2::{Commit, DiffFormat, DiffOptions, Repository};
use tracing::{debug, info};

use crate::domain::config::AppConfig;
use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk, DiffLine, DiffLineKind};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiffSource {
    #[default]
    WorkingTree,
    RootDirectory,
    Commit {
        rev: String,
    },
    Range {
        base: String,
        head: String,
    },
}

impl DiffSource {
    #[must_use]
    pub fn working_tree() -> Self {
        Self::WorkingTree
    }
}

/// # Errors
///
/// Returns an error when the git repository cannot be discovered, the requested revision cannot be
/// resolved, the diff cannot be rendered, or the rendered patch cannot be parsed.
pub async fn load_git_diff(config: &AppConfig, source: &DiffSource) -> Result<DiffDocument> {
    debug!(?source, "loading git diff");
    let config = config.clone();
    let source = source.clone();
    let source_for_worker = source.clone();
    let document =
        tokio::task::spawn_blocking(move || load_git_diff_sync(config, source_for_worker))
            .await
            .context("failed to join git2 diff worker")??;
    info!(files = document.files.len(), ?source, "git diff loaded");
    Ok(document)
}

/// # Errors
///
/// Returns an error for the same repository discovery, diff rendering, and parsing failures as
/// [`load_git_diff`].
pub async fn load_git_diff_head(config: &AppConfig) -> Result<DiffDocument> {
    load_git_diff(config, &DiffSource::WorkingTree).await
}

fn load_git_diff_sync(config: AppConfig, source: DiffSource) -> Result<DiffDocument> {
    let repo = Repository::discover(".").context("failed to discover git repository")?;
    load_git_diff_for_repo(&repo, &config, &source)
}

fn load_git_diff_for_repo(
    repo: &Repository,
    config: &AppConfig,
    source: &DiffSource,
) -> Result<DiffDocument> {
    let text = load_diff_text(repo, source)?;
    let mut document = if matches!(source, DiffSource::RootDirectory) {
        load_root_directory_document(repo, config)?
    } else {
        parse_unified_diff(&text)?
    };
    let ignore_repo =
        matches!(source, DiffSource::WorkingTree | DiffSource::RootDirectory).then_some(repo);
    filter_ignored_files(&mut document, config, ignore_repo)?;
    Ok(document)
}

fn load_diff_text(repo: &Repository, source: &DiffSource) -> Result<String> {
    let mut diff_opts = DiffOptions::new();
    diff_opts.context_lines(3).include_typechange(true);

    let diff = match source {
        DiffSource::WorkingTree => {
            configure_worktree_diff_options(&mut diff_opts);
            let head_tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());
            repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut diff_opts))
                .context("failed to compute repository diff")?
        }
        DiffSource::RootDirectory => return Ok(String::new()),
        DiffSource::Commit { rev } => {
            let commit = resolve_commit(repo, rev)?;
            let new_tree = commit.tree().context("failed to read commit tree")?;
            let old_tree = commit
                .parent(0)
                .ok()
                .map(|parent| parent.tree().context("failed to read parent tree"))
                .transpose()?;
            repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut diff_opts))
                .with_context(|| format!("failed to diff commit {rev}"))?
        }
        DiffSource::Range { base, head } => {
            let base_tree = resolve_commit(repo, base)?
                .tree()
                .with_context(|| format!("failed to read base tree for {base}"))?;
            let head_tree = resolve_commit(repo, head)?
                .tree()
                .with_context(|| format!("failed to read head tree for {head}"))?;
            repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut diff_opts))
                .with_context(|| format!("failed to diff range {base}..{head}"))?
        }
    };

    render_diff_text(diff)
}

fn load_root_directory_document(repo: &Repository, config: &AppConfig) -> Result<DiffDocument> {
    let workdir = repo
        .workdir()
        .context("root directory reviews require a non-bare git repository")?;
    let mut paths = tracked_file_paths(repo)?;
    collect_untracked_file_paths(repo, workdir, workdir, config, &mut paths)?;

    let mut files = Vec::new();
    for path in paths {
        if should_ignore_file(path.to_string_lossy().as_ref(), config, Some(repo))? {
            continue;
        }
        if let Some(file) = root_directory_file(workdir, &path)? {
            files.push(file);
        }
    }

    Ok(DiffDocument { files })
}

fn tracked_file_paths(repo: &Repository) -> Result<BTreeSet<PathBuf>> {
    let index = repo.index().context("failed to read git index")?;
    let mut paths = BTreeSet::new();
    for entry in index.iter() {
        let path = std::str::from_utf8(&entry.path).context("git index path is not utf-8")?;
        paths.insert(PathBuf::from(path));
    }
    Ok(paths)
}

fn collect_untracked_file_paths(
    repo: &Repository,
    workdir: &Path,
    dir: &Path,
    config: &AppConfig,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        let relative_path = path
            .strip_prefix(workdir)
            .with_context(|| format!("failed to relativize {}", path.display()))?;

        if should_skip_root_directory_path(relative_path, config) {
            continue;
        }

        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if file_type.is_dir() {
            if repo.status_should_ignore(relative_path).with_context(|| {
                format!("failed to evaluate gitignore rules for {}", path.display())
            })? {
                continue;
            }
            collect_untracked_file_paths(repo, workdir, &path, config, paths)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if repo
            .status_should_ignore(relative_path)
            .with_context(|| format!("failed to evaluate gitignore rules for {}", path.display()))?
        {
            continue;
        }
        paths.insert(relative_path.to_path_buf());
    }
    Ok(())
}

fn root_directory_file(workdir: &Path, relative_path: &Path) -> Result<Option<DiffFile>> {
    let path = workdir.join(relative_path);
    if !path.is_file() {
        return Ok(None);
    }

    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(None);
    }
    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let display_path = normalize_relative_path(relative_path);
    Ok(Some(diff_file_from_content(&display_path, &content)))
}

fn diff_file_from_content(path: &str, content: &str) -> DiffFile {
    let lines = content.lines().collect::<Vec<_>>();
    let line_count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    let mut hunk = DiffHunk {
        old_start: 1,
        old_count: line_count,
        new_start: 1,
        new_count: line_count,
        header: format!("@@ -1,{line_count} +1,{line_count} @@"),
        lines: Vec::with_capacity(lines.len() + 1),
    };
    hunk.lines.push(DiffLine {
        kind: DiffLineKind::HunkHeader,
        old_line: None,
        new_line: None,
        raw: hunk.header.clone(),
        code: hunk.header.clone(),
    });
    for (index, line) in lines.into_iter().enumerate() {
        let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        hunk.lines.push(DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(line_number),
            new_line: Some(line_number),
            raw: format!(" {line}"),
            code: line.to_string(),
        });
    }

    DiffFile {
        path: path.to_string(),
        header_lines: vec![format!("file {path}")],
        hunks: vec![hunk],
    }
}

fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn should_skip_root_directory_path(path: &Path, config: &AppConfig) -> bool {
    let mut components = path.components();
    let Some(Component::Normal(first)) = components.next() else {
        return false;
    };
    if first == ".git" || first == "worktrees" {
        return true;
    }
    config.ignore_parley_dir && first == ".parley"
}

fn configure_worktree_diff_options(diff_opts: &mut DiffOptions) {
    diff_opts
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
}

fn render_diff_text(diff: git2::Diff<'_>) -> Result<String> {
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

fn resolve_commit<'repo>(repo: &'repo Repository, rev: &str) -> Result<Commit<'repo>> {
    repo.revparse_single(rev)
        .with_context(|| format!("failed to resolve revision {rev}"))?
        .peel_to_commit()
        .with_context(|| format!("revision {rev} does not resolve to a commit"))
}

/// # Errors
///
/// Returns an error when a hunk header is malformed or line numbers cannot be parsed.
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
                path: parse_diff_git_path(line).unwrap_or_default(),
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

        if let Some(file) = current_file.as_mut()
            && current_hunk.is_none()
        {
            if line.starts_with("+++ ") {
                if let Some(path) = parse_patch_path(line, "+++ ") {
                    file.path = path;
                }
                file.header_lines.push(line.to_string());
                continue;
            }

            if line.starts_with("--- ") {
                if file.path.is_empty()
                    && let Some(path) = parse_patch_path(line, "--- ")
                {
                    file.path = path;
                }
                file.header_lines.push(line.to_string());
                continue;
            }

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

fn filter_ignored_files(
    document: &mut DiffDocument,
    config: &AppConfig,
    repo: Option<&Repository>,
) -> Result<()> {
    if !config.ignore_parley_dir && repo.is_none() {
        return Ok(());
    }

    let mut retained = Vec::with_capacity(document.files.len());
    for file in document.files.drain(..) {
        if should_ignore_file(&file.path, config, repo)? {
            continue;
        }
        retained.push(file);
    }
    document.files = retained;
    Ok(())
}

fn is_parley_internal_path(path: &str) -> bool {
    path == ".parley" || path.starts_with(".parley/")
}

fn should_ignore_file(path: &str, config: &AppConfig, repo: Option<&Repository>) -> Result<bool> {
    if config.ignore_parley_dir && is_parley_internal_path(path) {
        return Ok(true);
    }

    let Some(repo) = repo else {
        return Ok(false);
    };
    repo.status_should_ignore(Path::new(path))
        .with_context(|| format!("failed to evaluate gitignore rules for {path}"))
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

fn parse_patch_path(line: &str, marker: &str) -> Option<String> {
    let raw = line.strip_prefix(marker)?.trim();
    parse_diff_path(raw)
}

fn parse_diff_git_path(line: &str) -> Option<String> {
    let raw = line.strip_prefix("diff --git ")?;
    let (_, right) = split_diff_paths(raw)?;
    parse_diff_path(right)
}

fn split_diff_paths(raw: &str) -> Option<(&str, &str)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if let Some(rest) = raw.strip_prefix('"') {
        let end_left = rest.find('"')?;
        let left = &raw[..=end_left + 1];
        let rest = rest[end_left + 1..].trim_start();
        let rest = rest.strip_prefix('"')?;
        let end_right = rest.find('"')?;
        let right = &rest[..=end_right];
        return Some((left, right));
    }

    let (left, right) = raw.split_once(' ')?;
    Some((left, right.trim_start()))
}

fn parse_diff_path(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw == "/dev/null" {
        return None;
    }

    let unquoted = raw
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(raw);
    let normalized = unquoted
        .strip_prefix("a/")
        .or_else(|| unquoted.strip_prefix("b/"))
        .unwrap_or(unquoted);
    Some(normalized.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;
    use git2::{Oid, Repository, Signature};
    use tempfile::tempdir;

    use crate::domain::{config::AppConfig, diff::DiffLineKind};

    use super::{DiffSource, filter_ignored_files, load_git_diff_for_repo, parse_unified_diff};

    #[test]
    fn parse_unified_diff_should_parse_added_and_removed_lines_with_numbers() -> Result<()> {
        let input = "diff --git a/src/lib.rs b/src/lib.rs\nindex 123..456 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,3 @@\n fn a() {}\n-fn b() {}\n+fn b() {\"x\";}\n+fn c() {}\n";

        let doc = parse_unified_diff(input)?;

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
        Ok(())
    }

    #[test]
    fn parse_unified_diff_should_use_old_path_for_deleted_files() -> Result<()> {
        let input = "diff --git a/src/old.rs b/src/old.rs\nindex 123..456 100644\n--- a/src/old.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-fn old() {}\n";

        let doc = parse_unified_diff(input)?;

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, "src/old.rs");
        Ok(())
    }

    #[test]
    fn parse_unified_diff_should_parse_quoted_paths() -> Result<()> {
        let input = "diff --git \"a/src/with space.rs\" \"b/src/with space.rs\"\nindex 123..456 100644\n--- \"a/src/with space.rs\"\n+++ \"b/src/with space.rs\"\n@@ -1 +1 @@\n-fn before() {}\n+fn after() {}\n";

        let doc = parse_unified_diff(input)?;

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, "src/with space.rs");
        Ok(())
    }

    #[test]
    fn parse_unified_diff_should_use_diff_header_path_for_binary_new_files() -> Result<()> {
        let input = "diff --git a/src-tauri/icons/128x128.png b/src-tauri/icons/128x128.png\nnew file mode 100644\nindex 0000000..6be5e50\nBinary files /dev/null and b/src-tauri/icons/128x128.png differ\n";

        let doc = parse_unified_diff(input)?;

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, "src-tauri/icons/128x128.png");
        assert!(doc.files[0].hunks.is_empty());
        Ok(())
    }

    #[test]
    fn filter_ignored_files_removes_parley_entries_by_default() -> Result<()> {
        let input = "diff --git a/.parley/config.toml b/.parley/config.toml\n--- a/.parley/config.toml\n+++ b/.parley/config.toml\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let mut doc = parse_unified_diff(input)?;

        filter_ignored_files(&mut doc, &AppConfig::default(), None)?;

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, "src/lib.rs");
        Ok(())
    }

    #[test]
    fn filter_ignored_files_can_keep_parley_entries_when_configured() -> Result<()> {
        let input = "diff --git a/.parley/config.toml b/.parley/config.toml\n--- a/.parley/config.toml\n+++ b/.parley/config.toml\n@@ -1 +1 @@\n-old\n+new\n";
        let mut doc = parse_unified_diff(input)?;
        let config = AppConfig {
            ignore_parley_dir: false,
            ..AppConfig::default()
        };

        filter_ignored_files(&mut doc, &config, None)?;

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, ".parley/config.toml");
        Ok(())
    }

    #[test]
    fn filter_ignored_files_removes_gitignored_paths() -> Result<()> {
        let temp = tempdir()?;
        let repo = Repository::init(temp.path())?;
        fs::write(
            temp.path().join(".gitignore"),
            "ignored.txt\nignored-dir/\n",
        )?;
        fs::write(temp.path().join("ignored.txt"), "ignored\n")?;
        fs::create_dir_all(temp.path().join("ignored-dir"))?;
        fs::write(temp.path().join("ignored-dir/file.txt"), "ignored\n")?;
        fs::write(temp.path().join("tracked.txt"), "tracked\n")?;

        let input = "diff --git a/ignored.txt b/ignored.txt\nnew file mode 100644\nindex 0000000..1111111\nBinary files /dev/null and b/ignored.txt differ\ndiff --git a/ignored-dir/file.txt b/ignored-dir/file.txt\nnew file mode 100644\nindex 0000000..2222222\nBinary files /dev/null and b/ignored-dir/file.txt differ\ndiff --git a/tracked.txt b/tracked.txt\nnew file mode 100644\nindex 0000000..3333333\nBinary files /dev/null and b/tracked.txt differ\n";
        let mut doc = parse_unified_diff(input)?;

        filter_ignored_files(&mut doc, &AppConfig::default(), Some(&repo))?;

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, "tracked.txt");
        Ok(())
    }

    #[test]
    fn load_git_diff_for_commit_uses_first_parent_diff() -> Result<()> {
        let temp = tempdir()?;
        let repo = Repository::init(temp.path())?;

        let first = commit_file(&repo, temp.path(), "src/lib.rs", "fn first() {}\n", "first")?;
        let second = commit_file(
            &repo,
            temp.path(),
            "src/lib.rs",
            "fn second() {}\n",
            "second",
        )?;

        let doc = load_git_diff_for_repo(
            &repo,
            &AppConfig::default(),
            &DiffSource::Commit {
                rev: second.to_string(),
            },
        )?;

        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].path, "src/lib.rs");
        let lines = &doc.files[0].hunks[0].lines;
        assert!(lines.iter().any(|line| line.raw == "-fn first() {}"));
        assert!(lines.iter().any(|line| line.raw == "+fn second() {}"));

        let root_doc = load_git_diff_for_repo(
            &repo,
            &AppConfig::default(),
            &DiffSource::Commit {
                rev: first.to_string(),
            },
        )?;

        assert_eq!(root_doc.files.len(), 1);
        assert!(
            root_doc.files[0]
                .hunks
                .iter()
                .flat_map(|hunk| hunk.lines.iter())
                .any(|line| line.raw == "+fn first() {}")
        );
        Ok(())
    }

    #[test]
    fn load_git_diff_for_range_uses_explicit_base_and_head() -> Result<()> {
        let temp = tempdir()?;
        let repo = Repository::init(temp.path())?;

        let base = commit_file(&repo, temp.path(), "src/lib.rs", "fn one() {}\n", "one")?;
        let _middle = commit_file(&repo, temp.path(), "src/lib.rs", "fn two() {}\n", "two")?;
        let head = commit_file(&repo, temp.path(), "src/lib.rs", "fn three() {}\n", "three")?;

        let doc = load_git_diff_for_repo(
            &repo,
            &AppConfig::default(),
            &DiffSource::Range {
                base: base.to_string(),
                head: head.to_string(),
            },
        )?;

        assert_eq!(doc.files.len(), 1);
        let lines = &doc.files[0].hunks[0].lines;
        assert!(lines.iter().any(|line| line.raw == "-fn one() {}"));
        assert!(lines.iter().any(|line| line.raw == "+fn three() {}"));
        assert!(!lines.iter().any(|line| line.raw == "+fn two() {}"));
        Ok(())
    }

    #[test]
    fn load_root_directory_includes_tracked_and_untracked_files() -> Result<()> {
        let temp = tempdir()?;
        let repo = Repository::init(temp.path())?;

        commit_file(&repo, temp.path(), ".gitignore", "ignored.log\n", "ignore")?;
        commit_file(
            &repo,
            temp.path(),
            "src/lib.rs",
            "fn tracked() {}\n",
            "tracked",
        )?;
        fs::write(temp.path().join("src/extra.rs"), "fn untracked() {}\n")?;
        fs::write(temp.path().join("ignored.log"), "ignored\n")?;
        fs::create_dir_all(temp.path().join("worktrees/other/src"))?;
        fs::write(
            temp.path().join("worktrees/other/src/lib.rs"),
            "fn other_worktree() {}\n",
        )?;

        let doc = load_git_diff_for_repo(&repo, &AppConfig::default(), &DiffSource::RootDirectory)?;

        let paths = doc
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec![".gitignore", "src/extra.rs", "src/lib.rs"]);

        let tracked = doc
            .files
            .iter()
            .find(|file| file.path == "src/lib.rs")
            .expect("tracked file should be present");
        let tracked_lines = &tracked.hunks[0].lines;
        assert!(tracked_lines.iter().any(|line| {
            line.kind == DiffLineKind::Context
                && line.old_line == Some(1)
                && line.new_line == Some(1)
                && line.code == "fn tracked() {}"
        }));
        Ok(())
    }

    fn commit_file(
        repo: &Repository,
        root: &std::path::Path,
        relative_path: &str,
        content: &str,
        message: &str,
    ) -> Result<Oid> {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;

        let mut index = repo.index()?;
        index.add_path(std::path::Path::new(relative_path))?;
        index.write()?;

        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let signature = Signature::now("Parley Test", "parley@example.com")?;
        let parents = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| repo.find_commit(oid))
            .transpose()?
            .into_iter()
            .collect::<Vec<_>>();
        let parent_refs = parents.iter().collect::<Vec<_>>();

        Ok(repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parent_refs,
        )?)
    }
}
