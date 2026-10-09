use std::sync::Arc;

use async_trait::async_trait;
use warpui::{Entity, SingletonEntity};

use crate::server::server_api::TranscribeError;
use crate::server::team_scope::RequestTeamScope;

/// Interface for transcribing voice input.
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait Transcriber: Send + Sync {
    /// Transcribe the given base64 encoded wav file into text.
    ///
    /// `language` is an optional ISO-639-1 code (e.g. `"en"`). When `None`, the
    /// transcription provider auto-detects the spoken language.
    /// This is expected to be async and called off the main thread.
    async fn transcribe(
        &self,
        wav_base64: String,
        language: Option<String>,
        team_scope: RequestTeamScope,
    ) -> Result<String, TranscribeError>;
}

/// A voice transcriber that is enabled or disabled.
///
/// This is a singleton model that the app can decide to enable or disable.
/// The editor does expect that it will exist as a singleton fetchable from app context
/// either way though, and depending on whether the optional transcriber is set,
/// the editor considers transcriber to be enabled or disabled.
///
/// We set it up this way to avoid the editor having a direct dependency on any server api.
pub struct VoiceTranscriber {
    /// The transcriber to use. If `None`, the transcriber is disabled.
    #[cfg_attr(not(feature = "voice_input"), allow(dead_code))]
    transcriber: Option<Arc<dyn Transcriber>>,
}

impl VoiceTranscriber {
    pub fn new(transcriber: Arc<dyn Transcriber>) -> Self {
        Self {
            transcriber: Some(transcriber),
        }
    }

    /// Returns the transcriber if one is set.
    pub fn transcriber(&self) -> Option<&Arc<dyn Transcriber>> {
        self.transcriber.as_ref()
    }
}

impl Entity for VoiceTranscriber {
    type Event = ();
}

impl SingletonEntity for VoiceTranscriber {}

/// Local voice transcriber for WarpOss.
///
/// Serves as an in-process local transcriber placeholder that returns
/// a clear error indicating the local speech model or runtime engine has not been installed yet,
/// without calling any remote server.
#[derive(Debug, Default)]
pub struct LocalVoiceTranscriber;

impl LocalVoiceTranscriber {
    pub fn new() -> Self {
        Self
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl Transcriber for LocalVoiceTranscriber {
    async fn transcribe(
        &self,
        _wav_base64: String,
        _language: Option<String>,
        _team_scope: RequestTeamScope,
    ) -> Result<String, TranscribeError> {
        Err(TranscribeError::Other(anyhow::anyhow!(
            "Local voice transcription engine is not installed yet"
        )))
    }
}

impl Entity for LocalVoiceTranscriber {
    type Event = ();
}

impl SingletonEntity for LocalVoiceTranscriber {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspaces::user_workspaces::team_workspace_settings::TeamlessScopeForTest;

    #[tokio::test]
    async fn test_local_voice_transcriber_returns_not_installed_error() {
        let transcriber = LocalVoiceTranscriber::new();
        let result = transcriber
            .transcribe(
                "UklGRg==".to_string(),
                None,
                RequestTeamScope::from_scope(&TeamlessScopeForTest),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not installed yet"));
    }
}
