use super::*;
use crate::git::diff::{DiffSource, load_root_directory_file};
use anyhow::{Context, Result};
use std::io::ErrorKind;
use tokio::process::Command;

const CODE_SEARCH_MAX_RESULTS: usize = 200;
const GREP_FILE_CHUNK_SIZE: usize = 200;

#[derive(Debug)]
struct CodeSearchRun {
    engine: &'static str,
    results: Vec<CodeSearchResult>,
}

impl TuiApp {
    pub(super) async fn open_code_search(&mut self) -> Result<()> {
        self.dismiss_ai_progress_popup();
        self.code_search = Some(CodeSearchState {
            query: String::new(),
            cursor_col: 0,
            results: Vec::new(),
            selected_index: 0,
            scroll: 0,
            engine: None,
            message: "type to search with rg; grep fallback if rg is unavailable".into(),
        });
        self.status_line = "code search opened".into();
        Ok(())
    }

    pub(super) async fn handle_code_search_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.code_search = None;
            self.status_line = "code search closed".into();
            return Ok(());
        }

        if matches!(key.code, KeyCode::Enter) {
            return self.open_selected_code_search_result().await;
        }

        let mut should_refresh = false;
        if let Some(search) = self.code_search.as_mut() {
            match key.code {
                KeyCode::Up => {
                    search.selected_index = search.selected_index.saturating_sub(1);
                }
                KeyCode::Down => {
                    let max_index = search.results.len().saturating_sub(1);
                    search.selected_index = (search.selected_index + 1).min(max_index);
                }
                KeyCode::PageUp => {
                    search.selected_index = search.selected_index.saturating_sub(8);
                }
                KeyCode::PageDown => {
                    let max_index = search.results.len().saturating_sub(1);
                    search.selected_index = (search.selected_index + 8).min(max_index);
                }
                KeyCode::Home => {
                    search.cursor_col = 0;
                }
                KeyCode::End => {
                    search.cursor_col = search.query.chars().count();
                }
                KeyCode::Left => {
                    search.cursor_col = search.cursor_col.saturating_sub(1);
                }
                KeyCode::Right => {
                    search.cursor_col = (search.cursor_col + 1).min(search.query.chars().count());
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    search.cursor_col = 0;
                }
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    search.cursor_col = search.query.chars().count();
                }
                KeyCode::Backspace if search.cursor_col > 0 => {
                    remove_char_at(&mut search.query, search.cursor_col - 1);
                    search.cursor_col -= 1;
                    should_refresh = true;
                }
                KeyCode::Delete if search.cursor_col < search.query.chars().count() => {
                    remove_char_at(&mut search.query, search.cursor_col);
                    should_refresh = true;
                }
                KeyCode::Char(ch)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    insert_char_at(&mut search.query, search.cursor_col, ch);
                    search.cursor_col += 1;
                    should_refresh = true;
                }
                _ => {}
            }
        }

        if should_refresh {
            self.refresh_code_search().await?;
        }
        self.constrain_code_search_selection();
        Ok(())
    }

    async fn refresh_code_search(&mut self) -> Result<()> {
        let Some(search) = self.code_search.as_mut() else {
            return Ok(());
        };
        let query = search.query.trim().to_string();
        search.selected_index = 0;
        search.scroll = 0;
        if query.is_empty() {
            search.results.clear();
            search.engine = None;
            search.message = "type to search with rg; grep fallback if rg is unavailable".into();
            self.search_query = None;
            self.status_line = "code search ready".into();
            return Ok(());
        }

        search.message = format!("searching for {query}");
        self.search_query = Some(query.clone());
        let run = run_code_search(&query).await?;
        let result_count = run.results.len();
        search.results = run.results;
        search.engine = Some(run.engine);
        search.message = if result_count == 0 {
            format!("no matches via {}", run.engine)
        } else if result_count >= CODE_SEARCH_MAX_RESULTS {
            format!(
                "showing first {CODE_SEARCH_MAX_RESULTS} matches via {}",
                run.engine
            )
        } else {
            format!("{result_count} match(es) via {}", run.engine)
        };
        self.status_line = search.message.clone();
        Ok(())
    }

    pub(super) fn constrain_code_search_selection(&mut self) {
        let Some(search) = self.code_search.as_mut() else {
            return;
        };
        if search.results.is_empty() {
            search.selected_index = 0;
            search.scroll = 0;
            return;
        }
        search.selected_index = search
            .selected_index
            .min(search.results.len().saturating_sub(1));
        if search.selected_index < search.scroll {
            search.scroll = search.selected_index;
        } else if search.selected_index >= search.scroll.saturating_add(10) {
            search.scroll = search.selected_index.saturating_sub(9);
        }
    }

    pub(super) async fn open_selected_code_search_result(&mut self) -> Result<()> {
        let Some(selected_index) = self
            .code_search
            .as_ref()
            .map(|search| search.selected_index)
        else {
            self.status_line = "no code search result selected".into();
            return Ok(());
        };
        self.open_code_search_result_at_index(selected_index).await
    }

    pub(super) async fn open_code_search_result_at_index(
        &mut self,
        result_index: usize,
    ) -> Result<()> {
        let Some(result) = self
            .code_search
            .as_ref()
            .and_then(|search| search.results.get(result_index))
            .cloned()
        else {
            self.status_line = "no code search result selected".into();
            return Ok(());
        };

        let file_index = if let Some(index) = self
            .diff
            .files
            .iter()
            .position(|file| file.path == result.path)
        {
            index
        } else if let Some(file) = load_root_directory_file(&self.config, result.path.clone())
            .await
            .with_context(|| format!("failed to load {}", result.path))?
        {
            self.diff.files.push(file);
            self.invalidate_visible_file_indices_cache();
            let index = self.diff.files.len().saturating_sub(1);
            self.root_hydrated_files.insert(index);
            index
        } else {
            self.status_line = format!("search result is not reviewable: {}", result.path);
            return Ok(());
        };

        self.hydrate_code_search_file_if_needed(file_index, &result.path)
            .await?;
        self.select_file(file_index);
        self.ensure_row_cache_for_file(file_index);
        let opened = self.goto_line_number(result.line);
        self.code_search = None;
        self.status_line = if opened {
            format!("opened {}:{}", result.path, result.line)
        } else {
            format!("opened {} (line {} not in view)", result.path, result.line)
        };
        Ok(())
    }

    async fn hydrate_code_search_file_if_needed(
        &mut self,
        file_index: usize,
        path: &str,
    ) -> Result<()> {
        if !matches!(self.diff_source, DiffSource::RootDirectory)
            || self.root_hydrated_files.contains(&file_index)
            || self
                .diff
                .files
                .get(file_index)
                .is_some_and(|file| !file.hunks.is_empty())
        {
            return Ok(());
        }

        if let Some(file) = load_root_directory_file(&self.config, path.to_string())
            .await
            .with_context(|| format!("failed to load {path}"))?
            && let Some(slot) = self.diff.files.get_mut(file_index)
        {
            *slot = file;
            self.root_hydrated_files.insert(file_index);
            self.row_cache.remove(&file_index);
            self.clear_diff_render_cache_for_file(file_index);
        }
        Ok(())
    }
}

async fn run_code_search(query: &str) -> Result<CodeSearchRun> {
    match run_rg_search(query).await {
        Ok(results) => Ok(CodeSearchRun {
            engine: "rg",
            results,
        }),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io_error| io_error.kind() == ErrorKind::NotFound) =>
        {
            Ok(CodeSearchRun {
                engine: "grep",
                results: run_grep_search(query).await?,
            })
        }
        Err(error) => Err(error),
    }
}

async fn run_rg_search(query: &str) -> Result<Vec<CodeSearchResult>> {
    let output = Command::new("rg")
        .args([
            "--line-number",
            "--column",
            "--color",
            "never",
            "--smart-case",
            "--glob",
            "!worktrees/**",
            "--",
            query,
        ])
        .output()
        .await?;

    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("rg search failed: {}", stderr.trim());
    }

    Ok(parse_rg_output(&String::from_utf8_lossy(&output.stdout)))
}

async fn run_grep_search(query: &str) -> Result<Vec<CodeSearchResult>> {
    let files_output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .output()
        .await
        .context("failed to list git-tracked and unignored files")?;
    if !files_output.status.success() {
        let stderr = String::from_utf8_lossy(&files_output.stderr);
        anyhow::bail!("git ls-files failed for grep fallback: {}", stderr.trim());
    }

    let files = String::from_utf8_lossy(&files_output.stdout)
        .lines()
        .filter(|path| !path.starts_with("worktrees/"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut results = Vec::new();
    for chunk in files.chunks(GREP_FILE_CHUNK_SIZE) {
        if results.len() >= CODE_SEARCH_MAX_RESULTS {
            break;
        }
        let output = Command::new("grep")
            .args(["-nI", "-e", query, "--"])
            .args(chunk)
            .output()
            .await?;
        if !output.status.success() && output.status.code() != Some(1) {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("grep fallback failed: {}", stderr.trim());
        }
        results.extend(parse_grep_output(&String::from_utf8_lossy(&output.stdout)));
        results.truncate(CODE_SEARCH_MAX_RESULTS);
    }
    Ok(results)
}

fn parse_rg_output(output: &str) -> Vec<CodeSearchResult> {
    output
        .lines()
        .filter_map(parse_rg_output_line)
        .take(CODE_SEARCH_MAX_RESULTS)
        .collect()
}

fn parse_grep_output(output: &str) -> Vec<CodeSearchResult> {
    output
        .lines()
        .filter_map(parse_grep_output_line)
        .take(CODE_SEARCH_MAX_RESULTS)
        .collect()
}

fn parse_rg_output_line(line: &str) -> Option<CodeSearchResult> {
    let (path, rest) = line.split_once(':')?;
    let (line_number, rest) = rest.split_once(':')?;
    let (column, text) = rest.split_once(':')?;
    Some(CodeSearchResult {
        path: path.to_string(),
        line: line_number.parse().ok()?,
        column: column.parse().ok()?,
        text: text.to_string(),
    })
}

fn parse_grep_output_line(line: &str) -> Option<CodeSearchResult> {
    let (path, rest) = line.split_once(':')?;
    let (line_number, text) = rest.split_once(':')?;
    Some(CodeSearchResult {
        path: path.to_string(),
        line: line_number.parse().ok()?,
        column: 1,
        text: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rg_output_line_reads_path_line_column_and_text() {
        let parsed = parse_rg_output_line("src/main.rs:12:4:let query = search();")
            .expect("rg output should parse");

        assert_eq!(parsed.path, "src/main.rs");
        assert_eq!(parsed.line, 12);
        assert_eq!(parsed.column, 4);
        assert_eq!(parsed.text, "let query = search();");
    }

    #[test]
    fn parse_grep_output_line_defaults_column_to_one() {
        let parsed = parse_grep_output_line("src/main.rs:12:let query = search();")
            .expect("grep output should parse");

        assert_eq!(parsed.path, "src/main.rs");
        assert_eq!(parsed.line, 12);
        assert_eq!(parsed.column, 1);
        assert_eq!(parsed.text, "let query = search();");
    }
}
