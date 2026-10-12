//! Run preparation: the result-window knob and the session bootstrap —
//! everything the loop needs before its first turn, with each failure
//! surfacing as the run's typed error detail.

use craft_core::types::ChatMessage;

use crate::runner::RunConfig;
use crate::session::{Session, SessionError};
use crate::trim;

pub(crate) struct Prepared {
    pub(crate) result_window: u32,
    pub(crate) session: Session,
}

/// Resolves `CRAFT_AGENT_RESULT_WINDOW_TURNS` when unset on the config,
/// opens (or starts) the session record, and appends the opening user
/// message; the error string is the run's error detail, never a panic.
pub(crate) fn prepare(config: &RunConfig, prompt: &str) -> Result<Prepared, String> {
    let result_window = match config.result_window_turns {
        Some(window) => window,
        None => trim::result_window_from_env().map_err(|err| err.to_string())?,
    };
    let mut session = open_session(config).map_err(|err| err.to_string())?;
    if let Err(err) = session.append(ChatMessage::User {
        text: String::from(prompt),
        images: Vec::new(),
    }) {
        return Err(err.to_string());
    }
    Ok(Prepared {
        result_window,
        session,
    })
}

fn open_session(config: &RunConfig) -> Result<Session, SessionError> {
    match &config.session_path {
        Some(path) => Session::open(path.clone()),
        None => Ok(Session::in_memory()),
    }
}
