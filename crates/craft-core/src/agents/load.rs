//! Discovery and loading: the closest `.craft-agent/agents` between
//! the start directory and the git root, fail-soft directory scans,
//! and a fail-closed single-file loader.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::parse;
use super::validate;
use super::{AgentCatalog, AgentFile, AgentFileError, SkippedFile, file_stem};

/// Project-scope agents directory, relative to any directory from
/// the start up to the git root.
pub const AGENTS_DIR: &str = ".craft-agent/agents";
/// Hostile-input bound: one agent file may not exceed this size.
pub const FILE_BYTE_CAP: u64 = 65_536;
/// Levels walked up from the start directory, inclusive.
const DISCOVERY_DEPTH: usize = 8;

fn io_err(path: &Path, source: std::io::Error) -> AgentFileError {
    AgentFileError::Io {
        path: path.to_path_buf(),
        message: source.to_string(),
    }
}

/// The closest agents directory from `start` walking up, at most
/// [`DISCOVERY_DEPTH`] levels; the git root is the last candidate.
pub fn discover_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    for _ in 0..DISCOVERY_DEPTH {
        let candidate = dir.join(AGENTS_DIR);
        if candidate.is_dir() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            return None;
        }
        if !dir.pop() {
            return None;
        }
    }
    None
}

/// Reads and parses one file; the entry key is the parsed name when
/// the frontmatter yielded one, else the file stem — the identity a
/// `--agent` lookup can reach.
fn load_entry(path: &Path) -> (String, Result<AgentFile, AgentFileError>) {
    let parsed = match read_parsed(path) {
        Ok(parsed) => parsed,
        Err(error) => return (file_stem(path), Err(error)),
    };
    let key = parsed
        .entries
        .iter()
        .find(|entry| entry.key == "name")
        .map(|entry| entry.value.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| file_stem(path));
    (key, validate::build(&parsed, path))
}

/// Loads one agent file, fail-closed: any parse or rule violation is
/// a typed error naming the file and the rule.
pub fn load_file(path: &Path) -> Result<AgentFile, AgentFileError> {
    load_entry(path).1
}

fn read_parsed(path: &Path) -> Result<parse::Parsed, AgentFileError> {
    let file = File::open(path).map_err(|source| io_err(path, source))?;
    let mut bytes = Vec::new();
    file.take(FILE_BYTE_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_err(path, source))?;
    if bytes.len() as u64 > FILE_BYTE_CAP {
        return Err(AgentFileError::BodyLength {
            path: path.to_path_buf(),
            line: 1,
            message: format!("file is {} bytes (cap {FILE_BYTE_CAP})", bytes.len()),
        });
    }
    let source = String::from_utf8(bytes).map_err(|_| AgentFileError::Frontmatter {
        path: path.to_path_buf(),
        line: 1,
        message: String::from("file is not valid UTF-8"),
    })?;
    parse::parse(&source, path)
}

/// Scans one agents directory, fail-soft per file: every invalid file
/// is skipped into `skipped` with its error; a broken directory read
/// is a typed error.
pub fn load_dir(dir: &Path) -> Result<AgentCatalog, AgentFileError> {
    let reader = std::fs::read_dir(dir).map_err(|source| io_err(dir, source))?;
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in reader {
        let entry = entry.map_err(|source| io_err(dir, source))?;
        let path = entry.path();
        let is_file = entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false);
        if is_file && path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    files.sort();
    let mut catalog = AgentCatalog::default();
    for path in &files {
        let (key, result) = load_entry(path);
        match result {
            Ok(agent) => catalog.agents.push(agent),
            Err(error) => catalog.skipped.push(SkippedFile {
                key,
                stem: file_stem(path),
                error,
            }),
        }
    }
    Ok(catalog)
}

/// The catalog for the closest agents directory from `start`; no
/// directory found is an empty catalog, not an error.
pub fn load_catalog(start: &Path) -> Result<AgentCatalog, AgentFileError> {
    match discover_dir(start) {
        Some(dir) => load_dir(&dir),
        None => Ok(AgentCatalog::default()),
    }
}
