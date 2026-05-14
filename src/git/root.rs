use anyhow::{Context, Result};
use git2::Repository;
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// # Errors
///
/// Returns an error when git repository discovery fails, the repository is bare, or the blocking
/// discovery task cannot be joined.
pub async fn discover_workdir(start_dir: impl AsRef<Path>) -> Result<PathBuf> {
    let start_dir = start_dir.as_ref().to_path_buf();
    spawn_blocking(move || {
        let repo = Repository::discover(&start_dir).context("failed to discover git repository")?;
        let workdir = repo
            .workdir()
            .context("parley requires a non-bare git repository")?;
        Ok::<_, anyhow::Error>(workdir.to_path_buf())
    })
    .await
    .context("failed to join git repository discovery task")?
}

#[cfg(test)]
mod tests {
    use super::discover_workdir;
    use anyhow::Result;
    use git2::Repository;
    use tempfile::tempdir;
    use tokio::fs as tokio_fs;

    #[tokio::test]
    async fn discover_workdir_should_return_repo_root_from_subdirectory() -> Result<()> {
        let tempdir = tempdir()?;
        Repository::init(tempdir.path())?;
        let nested_dir = tempdir.path().join("src").join("nested");
        tokio_fs::create_dir_all(&nested_dir).await?;

        let workdir = discover_workdir(&nested_dir).await?;
        let expected = tokio_fs::canonicalize(tempdir.path()).await?;

        assert_eq!(workdir, expected);
        Ok(())
    }
}
