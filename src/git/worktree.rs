use anyhow::{Context, Result, anyhow};
use git2::Repository;
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// Shared repository context that accounts for git worktrees.
#[derive(Debug, Clone)]
pub struct RepositoryContext {
    /// The worktree path that git operations should run against.
    pub selected_worktree: PathBuf,
    /// The main/original worktree path (where `.parley` lives for normal repos).
    pub main_worktree: Option<PathBuf>,
    /// The common git directory (where `commondir` points for worktrees).
    pub common_git_dir: PathBuf,
    /// The resolved canonical storage root.
    pub storage_root: PathBuf,
    /// Identity of the currently selected worktree.
    pub current_worktree_name: Option<String>,
}

/// Information about a single git worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head_summary: Option<String>,
    pub is_current: bool,
}

/// Discover repository context from the current working directory.
pub async fn discover_from_cwd() -> Result<RepositoryContext> {
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    discover(&cwd).await
}

/// Discover repository context starting from a given directory.
pub async fn discover(start_dir: impl AsRef<Path>) -> Result<RepositoryContext> {
    let start_dir = start_dir.as_ref().to_path_buf();
    spawn_blocking(move || discover_sync(&start_dir))
        .await
        .context("failed to join repository discovery task")?
}

fn discover_sync(start_dir: &Path) -> Result<RepositoryContext> {
    let repo = Repository::discover(start_dir).context("failed to discover git repository")?;
    let workdir = repo.workdir().map(Path::to_path_buf);
    let common_git_dir = repo.commondir().to_path_buf();

    let main_worktree = resolve_main_worktree(&common_git_dir, workdir.as_ref());

    let selected_worktree = workdir.clone().unwrap_or_else(|| start_dir.to_path_buf());

    let storage_root = resolve_storage_root(main_worktree.as_ref(), &common_git_dir)?;
    let current_worktree_name = detect_current_worktree_name(&repo, workdir.as_ref())?;

    Ok(RepositoryContext {
        selected_worktree,
        main_worktree,
        common_git_dir,
        storage_root,
        current_worktree_name,
    })
}

fn resolve_main_worktree(common_git_dir: &Path, workdir: Option<&PathBuf>) -> Option<PathBuf> {
    let wd = workdir?;
    let canonical_common = std::fs::canonicalize(common_git_dir).ok();
    let canonical_wd = std::fs::canonicalize(wd).ok();

    if let (Some(common), Some(wd_canon)) = (canonical_common.as_deref(), canonical_wd.as_deref()) {
        let wd_git = wd_canon.join(".git");
        let wd_git_canon = std::fs::canonicalize(&wd_git).ok();
        if wd_git_canon.as_deref() == Some(common) {
            return canonical_wd.clone();
        }
    }

    if let Some(parent) = common_git_dir.parent()
        && parent.is_dir()
    {
        return Some(parent.to_path_buf());
    }
    canonical_wd.clone()
}

fn resolve_storage_root(main_worktree: Option<&PathBuf>, common_git_dir: &Path) -> Result<PathBuf> {
    if let Some(wd) = main_worktree {
        return Ok(wd.join(".parley"));
    }
    Ok(common_git_dir.join("parley"))
}

fn detect_current_worktree_name(
    repo: &Repository,
    workdir: Option<&PathBuf>,
) -> Result<Option<String>> {
    let current_path = std::env::current_dir().ok();

    if let Some(wd) = workdir
        && let Some(current) = current_path.as_deref()
    {
        let canonical_current = std::fs::canonicalize(current).ok();
        let canonical_wd = std::fs::canonicalize(wd).ok();
        if canonical_current != canonical_wd {
            let worktrees = repo.worktrees()?;
            for name in worktrees.iter().flatten() {
                if let Ok(wt) = repo.find_worktree(name)
                    && let Ok(wt_path) = std::fs::canonicalize(wt.path())
                    && Some(wt_path) == canonical_current
                {
                    return Ok(Some(name.to_string()));
                }
            }
            return Ok(current.file_name().map(|n| n.to_string_lossy().to_string()));
        }
    }

    Ok(None)
}

/// List all worktrees for the repository containing `start_dir`.
pub async fn list_worktrees(start_dir: impl AsRef<Path>) -> Result<Vec<WorktreeInfo>> {
    let start_dir = start_dir.as_ref().to_path_buf();
    spawn_blocking(move || list_worktrees_sync(&start_dir))
        .await
        .context("failed to join worktree listing task")?
}

fn list_worktrees_sync(start_dir: &Path) -> Result<Vec<WorktreeInfo>> {
    let repo = Repository::discover(start_dir).context("failed to discover git repository")?;
    let current_path = std::env::current_dir().and_then(std::fs::canonicalize).ok();

    let mut result = Vec::new();

    if let Some(workdir) = repo.workdir() {
        let canonical_wd = std::fs::canonicalize(workdir).ok();
        let is_current = current_path
            .as_ref()
            .and_then(|cp| canonical_wd.as_ref().map(|wd| cp == wd))
            .unwrap_or(false);
        let head_summary = repo.head().ok().and_then(|head| {
            let oid = head.target()?;
            head.shorthand()
                .map(|s| format!("{s} ({:.7})", oid.to_string()))
        });
        result.push(WorktreeInfo {
            name: "main".to_string(),
            path: workdir.to_path_buf(),
            branch: repo
                .head()
                .ok()
                .and_then(|h| h.shorthand().map(str::to_string)),
            head_summary,
            is_current,
        });
    }

    let worktrees = repo.worktrees()?;
    for name in worktrees.iter().flatten() {
        let Ok(wt) = repo.find_worktree(name) else {
            continue;
        };
        let path = wt.path().to_path_buf();
        let canonical_path = std::fs::canonicalize(&path).ok();
        let is_current = current_path
            .as_ref()
            .and_then(|cp| canonical_path.as_ref().map(|p| cp == p))
            .unwrap_or(false);

        let (branch, head_summary) = read_worktree_head(&path);

        result.push(WorktreeInfo {
            name: name.to_string(),
            path,
            branch,
            head_summary,
            is_current,
        });
    }

    Ok(result)
}

fn read_worktree_head(path: &Path) -> (Option<String>, Option<String>) {
    let head_path = path.join(".git").join("HEAD");
    if !head_path.exists() {
        let git_file = path.join(".git");
        if let Ok(content) = std::fs::read_to_string(&git_file) {
            let git_dir = content.trim().strip_prefix("gitdir: ").map(PathBuf::from);
            if let Some(git_dir) = git_dir {
                return parse_head_file(&git_dir.join("HEAD"));
            }
        }
    }
    parse_head_file(&head_path)
}

fn parse_head_file(path: &Path) -> (Option<String>, Option<String>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let trimmed = content.trim();
    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
        return (Some(branch.to_string()), Some(branch.to_string()));
    }
    let short = if trimmed.len() > 7 {
        &trimmed[..7]
    } else {
        trimmed
    };
    (None, Some(format!("detached {short}")))
}

/// Resolve a worktree selection by name or path against the repository.
pub async fn resolve_worktree(
    start_dir: impl AsRef<Path>,
    name_or_path: &str,
) -> Result<Option<PathBuf>> {
    let start_dir = start_dir.as_ref().to_path_buf();
    let name = name_or_path.to_string();
    spawn_blocking(move || resolve_worktree_sync(&start_dir, &name))
        .await
        .context("failed to join worktree resolution task")?
}

fn resolve_worktree_sync(start_dir: &Path, name_or_path: &str) -> Result<Option<PathBuf>> {
    let repo = Repository::discover(start_dir).context("failed to discover git repository")?;

    let worktrees = repo.worktrees()?;
    for name in worktrees.iter().flatten() {
        if name == name_or_path
            && let Ok(wt) = repo.find_worktree(name)
        {
            return Ok(Some(wt.path().to_path_buf()));
        }
    }

    let candidate = Path::new(name_or_path);
    if candidate.is_absolute() && candidate.is_dir() {
        return Ok(Some(candidate.to_path_buf()));
    }

    let relative = start_dir.join(candidate);
    if relative.is_dir() {
        return Ok(Some(relative.canonicalize().unwrap_or(relative)));
    }

    for name in worktrees.iter().flatten() {
        let Ok(wt) = repo.find_worktree(name) else {
            continue;
        };
        let wt_path = wt.path();
        if let Some(file_name) = wt_path.file_name()
            && file_name == name_or_path
        {
            return Ok(Some(wt_path.to_path_buf()));
        }
    }

    Ok(None)
}

/// Build a `RepositoryContext` with an explicit worktree selection.
pub async fn discover_with_worktree(
    start_dir: impl AsRef<Path>,
    worktree: Option<&str>,
) -> Result<RepositoryContext> {
    let mut ctx = discover(&start_dir).await?;

    if let Some(wt_name) = worktree {
        let Some(wt_path) = resolve_worktree(&start_dir, wt_name).await? else {
            return Err(anyhow!("worktree '{wt_name}' not found"));
        };
        ctx.selected_worktree = wt_path;
        ctx.current_worktree_name = Some(wt_name.to_string());
    }

    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Repository;
    use tempfile::tempdir;

    #[tokio::test]
    async fn discover_normal_repo_has_main_worktree() -> Result<()> {
        let tmp = tempdir()?;
        Repository::init(tmp.path())?;
        let ctx = discover(tmp.path()).await?;
        assert_eq!(ctx.selected_worktree, tmp.path());
        assert_eq!(ctx.main_worktree.as_deref(), Some(tmp.path()));
        assert_eq!(ctx.storage_root, tmp.path().join(".parley"));
        Ok(())
    }

    #[tokio::test]
    async fn resolve_worktree_returns_none_for_unknown() -> Result<()> {
        let tmp = tempdir()?;
        Repository::init(tmp.path())?;
        let result = resolve_worktree(tmp.path(), "nonexistent").await?;
        assert!(result.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn resolve_worktree_by_absolute_path() -> Result<()> {
        let tmp = tempdir()?;
        Repository::init(tmp.path())?;
        let result = resolve_worktree(tmp.path(), tmp.path().to_str().unwrap()).await?;
        assert_eq!(result, Some(tmp.path().to_path_buf()));
        Ok(())
    }
}
