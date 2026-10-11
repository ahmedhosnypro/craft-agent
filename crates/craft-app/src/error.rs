//! Typed CLI errors: one actionable line per failure, never a panic.

use std::path::PathBuf;

use craft_core::error::ProviderError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{var} is required but not set")]
    MissingEnv { var: &'static str },
    #[error("CRAFT_AGENT_PROVIDER must be anthropic, openai, or gemini; got `{value}`")]
    InvalidProvider { value: String },
    #[error("provider `{name}` failed to initialize: {source}")]
    ProviderInit {
        name: &'static str,
        source: ProviderError,
    },
    #[error("could not read the prompt from stdin: {source}")]
    PromptStdin { source: std::io::Error },
    #[error("could not read the prompt file `{path}`: {source}")]
    PromptFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("prompt exceeds the {max}-byte cap")]
    PromptTooLarge { max: usize },
    #[error("prompt is not valid UTF-8")]
    PromptNotUtf8,
    #[error("could not start the async runtime: {source}")]
    AsyncRuntime { source: std::io::Error },
    #[error("could not resolve the workspace root: {source}")]
    WorkspaceRoot { source: std::io::Error },
    #[error("could not prepare the session directory `{path}`: {source}")]
    SessionDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid --since window `{value}`; expected <n><s|m|h|d>, at most 366d")]
    InvalidWindow { value: String },
    #[error("--org must be a non-empty organization name")]
    EmptyOrg,
    #[error("the curated repos list at `{path}` has no entries")]
    EmptyRepoList { path: PathBuf },
    #[error("--repos must name at least one repository")]
    EmptyReposFlag,
    #[error("could not read the curated repos list `{path}`: {source}")]
    CuratedRepos {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the digest run failed: {source}")]
    Digest { source: craft_digest::DigestError },
}
