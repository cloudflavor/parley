use super::*;
use std::io::ErrorKind;
use tokio::process::Command;

const FILE_SEARCH_MAX_RESULTS: usize = 200;

#[derive(Debug)]
struct FileSearchRun {
    engine: &'static str,
    results: Vec<CodeSearchResult>,
}

impl TuiApp {
    pub(super) fn handle_file_search_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter)
            || (matches!(key.code, KeyCode::Char('f'))
                && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.file_search.focused = false;
            self.status_line = if self.file_search_query().is_some() {
                format!("file filter active: {}", self.file_search.query.trim())
            } else {
                "file filter cleared".into()
            };
            return Ok(());
        }

        apply_single_line_edit_key(
            &mut self.file_search.query,
            &mut self.file_search.cursor_col,
            key,
        );

        self.constrain_active_file_to_visible_list();
        self.constrain_selection();
        self.status_line = if self.file_search_query().is_some() {
            format!("file filter: {}", self.file_search.query.trim())
        } else {
            "file filter cleared".into()
        };
        Ok(())
    }

    pub(super) async fn handle_command_prompt_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            if let Some(prompt) = self.command_prompt.take() {
                let _ = prompt;
                self.status_line = "command cancelled".into();
            } else {
                self.status_line = "command cancelled".into();
            }
            return Ok(());
        }
        if matches!(key.code, KeyCode::Enter) {
            return self.run_command_prompt().await;
        }

        let Some(prompt) = self.command_prompt.as_mut() else {
            return Ok(());
        };

        apply_single_line_edit_key(&mut prompt.value, &mut prompt.cursor_col, key);

        Ok(())
    }

    async fn run_command_prompt(&mut self) -> Result<()> {
        let Some(prompt) = self.command_prompt.take() else {
            return Ok(());
        };

        match prompt.mode {
            CommandPromptMode::GotoLine => self.goto_line_from_prompt(&prompt.value),
            CommandPromptMode::SearchCurrentFile => {
                self.search_current_file_from_prompt(&prompt.value).await
            }
        }
    }

    fn goto_line_from_prompt(&mut self, input: &str) -> Result<()> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            self.status_line = "goto line expects a number".into();
            return Ok(());
        }

        let Ok(target) = trimmed.parse::<u32>() else {
            self.status_line = format!("invalid line number: {trimmed}");
            return Ok(());
        };

        if self.goto_line_number(target) {
            self.status_line = format!("jumped to line {target}");
        } else {
            self.status_line = format!("line {target} not found in current diff file");
        }
        Ok(())
    }

    pub(super) fn goto_line_number(&mut self, target: u32) -> bool {
        if target == 0 {
            return false;
        }
        self.ensure_row_cache();

        if let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| row.new_line == Some(target))
        {
            self.set_active_line_index(row_index);
            return true;
        }

        if let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| row.old_line == Some(target))
        {
            self.set_active_line_index(row_index);
            return true;
        }

        false
    }

    async fn search_current_file_from_prompt(&mut self, input: &str) -> Result<()> {
        let query = input.trim();
        if query.is_empty() {
            self.search_query = None;
            self.status_line = "file search expects text".into();
            return Ok(());
        }

        let Some(path) = self.current_file().map(|file| file.path.clone()) else {
            self.search_query = None;
            self.status_line = "no current file to search".into();
            return Ok(());
        };

        self.search_query = Some(query.to_string());
        let run = run_file_search(&path, query).await?;
        if run.results.is_empty() {
            if self.find_search_match(query, true) {
                self.status_line = format!("search match in rendered diff: {query}");
            } else {
                self.search_query = None;
                self.status_line = format!("no matches in {path} via {}", run.engine);
            }
            return Ok(());
        }

        let active_line = self
            .current_rows()
            .get(self.active_line_index())
            .and_then(|row| row.new_line.or(row.old_line))
            .unwrap_or(0);
        let target = run
            .results
            .iter()
            .find(|result| result.line > active_line)
            .or_else(|| run.results.first());
        let Some(target) = target else {
            return Ok(());
        };

        if self.goto_line_number(target.line) {
            self.status_line = format!("search match: {query} via {}", run.engine);
        } else if self.find_search_match(query, true) {
            self.status_line = format!("search match in rendered diff: {query}");
        } else {
            self.status_line = format!(
                "match in {path}:{} via {} is outside current diff view",
                target.line, run.engine
            );
        }
        Ok(())
    }

    pub(super) fn jump_search(&mut self, forward: bool) {
        let Some(query) = self.search_query.clone() else {
            self.status_line = "no active search (use /text)".into();
            return;
        };

        if self.find_search_match(&query, forward) {
            self.status_line = format!("search match: {query}");
        } else if self.current_rows_contain_query(&query) {
            self.status_line = format!("no further match for: {query}");
        } else {
            self.search_query = None;
            self.status_line = format!("search cleared (no matches): {query}");
        }
    }

    fn current_rows_contain_query(&mut self, query: &str) -> bool {
        self.ensure_row_cache();
        let needle = query.to_lowercase();
        self.current_rows()
            .iter()
            .any(|row| row.raw.to_lowercase().contains(&needle))
    }

    fn find_search_match(&mut self, query: &str, forward: bool) -> bool {
        self.ensure_row_cache();
        let rows = self.current_rows();
        let query_lower = query.to_lowercase();
        if !rows.is_empty() {
            let len = rows.len();
            let mut index = self.active_line_index();

            for _ in 0..len {
                index = if forward {
                    (index + 1) % len
                } else {
                    (index + len - 1) % len
                };

                let haystack = rows[index].raw.to_lowercase();
                if haystack.contains(&query_lower) {
                    self.set_active_line_index(index);
                    return true;
                }
            }
        }

        false
    }
}

async fn run_file_search(path: &str, query: &str) -> Result<FileSearchRun> {
    match run_rg_file_search(path, query).await {
        Ok(results) => Ok(FileSearchRun {
            engine: "rg",
            results,
        }),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io_error| io_error.kind() == ErrorKind::NotFound) =>
        {
            Ok(FileSearchRun {
                engine: "grep",
                results: run_grep_file_search(path, query).await?,
            })
        }
        Err(error) => Err(error),
    }
}

async fn run_rg_file_search(path: &str, query: &str) -> Result<Vec<CodeSearchResult>> {
    let output = Command::new("rg")
        .args([
            "--line-number",
            "--column",
            "--color",
            "never",
            "--smart-case",
            "--with-filename",
            "--",
            query,
            path,
        ])
        .output()
        .await?;

    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("rg file search failed: {}", stderr.trim());
    }

    Ok(parse_file_rg_output(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

async fn run_grep_file_search(path: &str, query: &str) -> Result<Vec<CodeSearchResult>> {
    let output = Command::new("grep")
        .args(["-nIH", "-e", query, "--", path])
        .output()
        .await?;
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("grep file search failed: {}", stderr.trim());
    }

    Ok(parse_file_grep_output(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_file_rg_output(output: &str) -> Vec<CodeSearchResult> {
    output
        .lines()
        .filter_map(parse_file_rg_output_line)
        .take(FILE_SEARCH_MAX_RESULTS)
        .collect()
}

fn parse_file_grep_output(output: &str) -> Vec<CodeSearchResult> {
    output
        .lines()
        .filter_map(parse_file_grep_output_line)
        .take(FILE_SEARCH_MAX_RESULTS)
        .collect()
}

fn parse_file_rg_output_line(line: &str) -> Option<CodeSearchResult> {
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

fn parse_file_grep_output_line(line: &str) -> Option<CodeSearchResult> {
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
    fn parse_file_rg_output_line_reads_path_line_column_and_text() {
        let parsed = parse_file_rg_output_line("src/main.rs:12:4:let query = search();")
            .expect("rg file output should parse");

        assert_eq!(parsed.path, "src/main.rs");
        assert_eq!(parsed.line, 12);
        assert_eq!(parsed.column, 4);
        assert_eq!(parsed.text, "let query = search();");
    }

    #[test]
    fn parse_file_grep_output_line_defaults_column_to_one() {
        let parsed = parse_file_grep_output_line("src/main.rs:12:let query = search();")
            .expect("grep file output should parse");

        assert_eq!(parsed.path, "src/main.rs");
        assert_eq!(parsed.line, 12);
        assert_eq!(parsed.column, 1);
        assert_eq!(parsed.text, "let query = search();");
    }
}
