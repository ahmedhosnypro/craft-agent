//! Dry-run emission: render, scrub, write to a caller-supplied writer.

use std::io::Write;

use crate::bundle::Bundle;
use crate::error::{DigestError, bounded};
use crate::render::render_digest;
use crate::scrub::scrub;

/// The typed hook the `--dry-run` CLI flag calls: the scrubbed digest goes
/// to `out`, nothing is written to disk or published.
pub fn emit_dry_run(mut out: impl Write, bundle: &Bundle) -> Result<(), DigestError> {
    let digest = scrub(&render_digest(bundle));
    out.write_all(digest.as_bytes())
        .map_err(|err| DigestError::Io(bounded(&err.to_string())))?;
    Ok(())
}
