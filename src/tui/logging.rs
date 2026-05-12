use std::fs::File;
use std::io;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::{Context, Result};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use tokio::fs::OpenOptions;
use tokio::fs::create_dir_all;

#[derive(Clone)]
struct FileMakeWriter {
    file: Arc<Mutex<File>>,
}

struct FileWriter {
    file: Arc<Mutex<File>>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FileMakeWriter {
    type Writer = FileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        FileWriter {
            file: Arc::clone(&self.file),
        }
    }
}

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .file
            .lock()
            .map_err(|_| io::Error::other("log file mutex poisoned"))?;
        guard.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut guard = self
            .file
            .lock()
            .map_err(|_| io::Error::other("log file mutex poisoned"))?;
        guard.flush()
    }
}

/// # Errors
///
/// Returns an error when the log directory cannot be created or the log file cannot be opened.
/// Unknown log level values are mapped to `INFO` for tracing initialization.
pub async fn init_file_tracing(log_path: &Path, log_level: &str) -> Result<()> {
    let file = open_log_file(log_path).await?;
    let make_writer = FileMakeWriter {
        file: Arc::new(Mutex::new(file)),
    };

    let level_filter = parse_level_filter(log_level);
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_thread_names(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_writer(make_writer)
        .with_filter(level_filter);

    let init_result = tracing_subscriber::registry().with(layer).try_init();
    if init_result.is_err() {
        // Tracing may already be initialized by another entrypoint; keep running.
        return Ok(());
    }

    Ok(())
}

async fn open_log_file(log_path: &Path) -> Result<File> {
    if let Some(parent) = log_path.parent() {
        create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create log directory {}", parent.display()))?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await
        .with_context(|| format!("failed to open log file {}", log_path.display()))?;

    Ok(file.into_std().await)
}

fn parse_level_filter(level: &str) -> LevelFilter {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        _ => LevelFilter::INFO,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use anyhow::Result;

    use super::open_log_file;

    #[tokio::test]
    async fn open_log_file_creates_parent_and_appends() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let log_path = temp_dir.path().join("nested").join("parley.log");

        let mut file = open_log_file(&log_path).await?;
        file.write_all(b"first")?;
        drop(file);

        let mut file = open_log_file(&log_path).await?;
        file.write_all(b" second")?;
        drop(file);

        let contents = tokio::fs::read_to_string(log_path).await?;
        assert_eq!(contents, "first second");

        Ok(())
    }
}
