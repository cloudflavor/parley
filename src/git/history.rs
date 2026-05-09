use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use git2::{Commit, DiffOptions, Repository, Sort};

#[derive(Debug, Clone)]
pub struct CommitSummary {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeatmapEntry {
    pub path: String,
    pub commits: usize,
    pub changes: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Default)]
struct FileHeatmapStats {
    commits: usize,
    insertions: usize,
    deletions: usize,
}

/// # Errors
///
/// Returns an error when the git repository cannot be found or its commit history cannot be read.
pub fn recent_commits(limit: usize) -> Result<Vec<CommitSummary>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let repo = Repository::discover(".").context("failed to locate git repository")?;
    let mut revwalk = repo.revwalk().context("failed to create git revwalk")?;
    revwalk
        .set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .context("failed to configure git revwalk sorting")?;
    revwalk
        .push_head()
        .context("failed to start git revwalk from HEAD")?;

    let mut commits = Vec::with_capacity(limit);
    for oid_result in revwalk.take(limit) {
        let oid = oid_result.context("failed to walk git history")?;
        let commit = repo
            .find_commit(oid)
            .with_context(|| format!("failed to load commit {oid}"))?;
        let summary = commit
            .summary()
            .unwrap_or("(no commit message)")
            .to_string();
        let oid_text = oid.to_string();
        let short_oid: String = oid_text.chars().take(12).collect();
        commits.push(CommitSummary {
            oid: oid_text,
            short_oid,
            summary,
        });
    }

    Ok(commits)
}

/// # Errors
///
/// Returns an error when the git repository cannot be found or commit diffs cannot be read.
pub fn file_heatmap() -> Result<Vec<FileHeatmapEntry>> {
    let repo = Repository::discover(".").context("failed to locate git repository")?;
    let mut revwalk = repo.revwalk().context("failed to create git revwalk")?;
    revwalk
        .set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .context("failed to configure git revwalk sorting")?;
    revwalk
        .push_head()
        .context("failed to start git revwalk from HEAD")?;

    let mut stats: HashMap<String, FileHeatmapStats> = HashMap::new();
    for oid_result in revwalk {
        let oid = oid_result.context("failed to walk git history")?;
        let commit = repo
            .find_commit(oid)
            .with_context(|| format!("failed to load commit {oid}"))?;
        collect_commit_file_heat(&repo, &commit, &mut stats)?;
    }

    let mut entries = stats
        .into_iter()
        .map(|(path, stats)| FileHeatmapEntry {
            path,
            commits: stats.commits,
            changes: stats.insertions + stats.deletions,
            insertions: stats.insertions,
            deletions: stats.deletions,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .changes
            .cmp(&left.changes)
            .then_with(|| right.commits.cmp(&left.commits))
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(entries)
}

fn collect_commit_file_heat(
    repo: &Repository,
    commit: &Commit<'_>,
    stats: &mut HashMap<String, FileHeatmapStats>,
) -> Result<()> {
    let new_tree = commit.tree().context("failed to read commit tree")?;
    let old_tree = if commit.parent_count() == 0 {
        None
    } else {
        Some(
            commit
                .parent(0)
                .context("failed to read first parent")?
                .tree()
                .context("failed to read parent tree")?,
        )
    };
    let mut options = DiffOptions::new();
    options.context_lines(0).include_typechange(true);
    let diff = repo
        .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut options))
        .context("failed to diff commit")?;

    let mut touched_paths = Vec::new();
    let mut line_changes = Vec::new();
    diff.foreach(
        &mut |delta, _progress| {
            if let Some(path) = delta_path(&delta) {
                touched_paths.push(path);
            }
            true
        },
        None,
        None,
        Some(&mut |delta, _hunk, line| {
            if let Some(path) = delta_path(&delta) {
                match line.origin() {
                    '+' => line_changes.push((path, true)),
                    '-' => line_changes.push((path, false)),
                    _ => {}
                }
            }
            true
        }),
    )
    .context("failed to walk commit diff")?;

    let mut touched = HashSet::new();
    for path in touched_paths {
        touched.insert(path);
    }
    for (path, insertion) in line_changes {
        touched.insert(path.clone());
        let entry = stats.entry(path).or_default();
        if insertion {
            entry.insertions += 1;
        } else {
            entry.deletions += 1;
        }
    }
    for path in touched {
        let entry = stats.entry(path).or_default();
        entry.commits += 1;
    }
    Ok(())
}

fn delta_path(delta: &git2::DiffDelta<'_>) -> Option<String> {
    delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .map(normalize_git_path)
}

fn normalize_git_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use anyhow::Result;
    use git2::{Oid, Signature};
    use tempfile::tempdir;

    use super::{file_heatmap, normalize_git_path};

    #[test]
    fn normalize_git_path_uses_forward_slashes() {
        assert_eq!(
            normalize_git_path(Path::new("src/lib.rs")),
            "src/lib.rs".to_string()
        );
    }

    #[test]
    fn file_heatmap_orders_files_by_line_churn() -> Result<()> {
        let temp = tempdir()?;
        let repo = git2::Repository::init(temp.path())?;
        commit_file(&repo, temp.path(), "src/hot.rs", "fn one() {}\n", "hot one")?;
        commit_file(&repo, temp.path(), "src/cold.rs", "fn cold() {}\n", "cold")?;
        commit_file(
            &repo,
            temp.path(),
            "src/hot.rs",
            "fn one() {}\nfn two() {}\n",
            "hot two",
        )?;

        let previous_dir = std::env::current_dir()?;
        std::env::set_current_dir(temp.path())?;
        let entries = file_heatmap();
        std::env::set_current_dir(previous_dir)?;
        let entries = entries?;

        assert_eq!(entries[0].path, "src/hot.rs");
        assert_eq!(entries[0].commits, 2);
        assert!(entries[0].changes >= entries[1].changes);
        Ok(())
    }

    fn commit_file(
        repo: &git2::Repository,
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
        let oid = repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parent_refs,
        )?;
        Ok(oid)
    }
}
