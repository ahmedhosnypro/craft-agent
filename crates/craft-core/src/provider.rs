//! The provider seam: one trait every dialect implements.

use core::future::Future;
use std::pin::Pin;

use futures::Stream;

use crate::error::ProviderError;
use crate::events::ModelEvent;
use crate::types::ModelRequest;

/// Streamed events for one turn; every failure is a typed error.
pub type ModelEventStream = Pin<Box<dyn Stream<Item = Result<ModelEvent, ProviderError>> + Send>>;

/// A provider dialect mounted over one endpoint. Implementations take
/// their base URL at construction — production passes the provider's
/// fixed default host, tests mount a lane of their choosing — so the
/// request itself never carries endpoint state.
pub trait Provider {
    fn stream(
        &self,
        request: ModelRequest,
    ) -> impl Future<Output = Result<ModelEventStream, ProviderError>> + Send;
}

/// Boxes an adapter's event stream into the seam's stream type.
pub fn boxed(
    stream: impl Stream<Item = Result<ModelEvent, ProviderError>> + Send + 'static,
) -> ModelEventStream {
    Box::pin(stream)
}
