//! One jscpd run across the whole clone set, feeding the recorded-report
//! parser; the program is injectable so corpora drive fixture scripts.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bundle::Bundle;
use crate::error::DigestError;
use crate::jscpd::{DuplicateCandidate, read_jscpd_report};

/// jscpd's file reporters write `<output>/jscpd-report.json`.
pub const REPORT_FILE_NAME: &str = "jscpd-report.json";

pub struct JscpdRunner {
    program: PathBuf,
}

impl Default for JscpdRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JscpdRunner {
    pub fn new() -> Self {
        JscpdRunner {
            program: PathBuf::from("jscpd"),
        }
    }

    /// Tests point this at a fake jscpd script.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        JscpdRunner {
            program: program.into(),
        }
    }
}

/// One run over the clone set: Rust mode, `**/target/**` ignored, json
/// report only, exit code passed through untouched. A missing binary, a
/// non-zero exit, or an unusable report becomes an error row and no
/// candidates — never a fatal Err.
pub fn run_jscpd(
    runner: &JscpdRunner,
    scan_root: &Path,
    report_dir: &Path,
    bundle: &mut Bundle,
) -> Result<Vec<DuplicateCandidate>, DigestError> {
    let output = Command::new(&runner.program)
        .arg(scan_root)
        .arg("--format")
        .arg("rust")
        .arg("--ignore")
        .arg("**/target/**")
        .arg("--reporters")
        .arg("json")
        .arg("--output")
        .arg(report_dir)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => {
            bundle.push_error("bundle", "jscpd", &err.to_string());
            return Ok(Vec::new());
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bundle.push_error("bundle", "jscpd", &stderr);
        return Ok(Vec::new());
    }
    match read_jscpd_report(&report_dir.join(REPORT_FILE_NAME)) {
        Ok(candidates) => Ok(candidates),
        Err(err) => {
            bundle.push_error("bundle", "jscpd", &err.to_string());
            Ok(Vec::new())
        }
    }
}
