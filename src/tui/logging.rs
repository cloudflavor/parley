use anyhow::{Context, Result};
use std::io;
use std::io::Write;
use std::path::Path;
use tokio::fs::File;
use tokio::fs::OpenOptions;
use tokio::fs::create_dir_all;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Clone)]
struct FileMakeWriter {
    sender: UnboundedSender<Vec<u8>>,
}

struct FileWriter {
    sender: UnboundedSender<Vec<u8>>,
    buffer: Vec<u8>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FileMakeWriter {
    type Writer = FileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        FileWriter {
            sender: self.sender.clone(),
            buffer: Vec::new(),
        }
    }
}

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let bytes = std::mem::take(&mut self.buffer);
        self.sender
            .send(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "log writer task stopped"))
    }
}

impl Drop for FileWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// # Errors
///
/// Returns an error when the log directory cannot be created or the log file cannot be opened.
/// Unknown log level values are mapped to `INFO` for tracing initialization.
pub async fn init_file_tracing(log_path: &Path, log_level: &str) -> Result<()> {
    let file = open_log_file(log_path).await?;
    let (sender, receiver) = mpsc::unbounded_channel();
    let _log_writer_task = tokio::spawn(write_log_entries(file, receiver));
    let make_writer = FileMakeWriter { sender };

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

async fn write_log_entries(mut file: File, mut receiver: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(entry) = receiver.recv().await {
        if file.write_all(&entry).await.is_err() {
            break;
        }

        if file.flush().await.is_err() {
            break;
        }
    }
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

    Ok(file)
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
    use super::open_log_file;
    use anyhow::Result;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn open_log_file_creates_parent_and_appends() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let log_path = temp_dir.path().join("nested").join("parley.log");

        let mut file = open_log_file(&log_path).await?;
        file.write_all(b"first").await?;
        file.flush().await?;
        drop(file);

        let mut file = open_log_file(&log_path).await?;
        file.write_all(b" second").await?;
        file.flush().await?;
        drop(file);

        let contents = tokio::fs::read_to_string(log_path).await?;
        assert_eq!(contents, "first second");

        Ok(())
    }
}
