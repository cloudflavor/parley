use anyhow::{Context, Result};
use git2::{Repository, Sort};

#[derive(Debug, Clone)]
pub struct CommitSummary {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
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
