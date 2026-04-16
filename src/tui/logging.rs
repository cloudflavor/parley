use std::{
    fs::{File, OpenOptions, create_dir_all},
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

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

pub fn init_file_tracing(log_path: &Path, log_level: &str) -> Result<()> {
    if let Some(parent) = log_path.parent() {
        create_dir_all(parent)
            .with_context(|| format!("failed to create log directory {}", parent.display()))?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open log file {}", log_path.display()))?;
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

fn parse_level_filter(level: &str) -> LevelFilter {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        _ => LevelFilter::INFO,
    }
}
