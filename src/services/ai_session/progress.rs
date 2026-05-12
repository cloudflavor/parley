use super::AiProgressEvent;
use crate::domain::ai::AiProvider;
use crate::utils::cast::u128_to_u64_saturating;
use tokio::sync::mpsc;

pub(super) fn emit_progress(
    progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    provider: AiProvider,
    stream: &str,
    message: impl Into<String>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u128_to_u64_saturating(elapsed.as_millis()))
        .unwrap_or(0);
    let _ = progress_sender.send(AiProgressEvent {
        timestamp_ms,
        provider: provider.as_str().to_string(),
        stream: stream.to_string(),
        message: message.into(),
    });
}
