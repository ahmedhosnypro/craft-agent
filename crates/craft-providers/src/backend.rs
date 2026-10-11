//! Backend enum dispatch for the provider adapters.

use craft_core::error::ProviderError;
use craft_core::provider::{ModelEventStream, Provider};
use craft_core::types::ModelRequest;

use crate::anthropic::AnthropicProvider;
use crate::gemini::GeminiProvider;
use crate::openai::OpenAiProvider;

/// One mounted dialect per variant; the runtime dispatches through this
/// enum and never knows which wire it is talking to.
pub enum Backend {
    OpenAi(OpenAiProvider),
    Anthropic(AnthropicProvider),
    Gemini(GeminiProvider),
}

impl Provider for Backend {
    async fn stream(&self, request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        match self {
            Backend::OpenAi(provider) => provider.stream(request).await,
            Backend::Anthropic(provider) => provider.stream(request).await,
            Backend::Gemini(provider) => provider.stream(request).await,
        }
    }
}
