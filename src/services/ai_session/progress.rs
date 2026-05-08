use std::sync::mpsc;

use crate::domain::ai::AiProvider;

use super::AiProgressEvent;

pub(super) fn emit_progress(
    progress_sender: Option<&mpsc::Sender<AiProgressEvent>>,
    provider: AiProvider,
    stream: &str,
    message: impl Into<String>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    let _ = progress_sender.send(AiProgressEvent {
        timestamp_ms,
        provider: provider.as_str().to_string(),
        stream: stream.to_string(),
        message: message.into(),
    });
}
