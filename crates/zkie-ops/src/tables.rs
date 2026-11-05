//! P1 Phase 1: `tables.meta` sidecar parser skeleton.
//!
//! The sidecar is a JSON document distributed with the model:
//!
//! ```json
//! {
//!   "version": 1,
//!   "tables": [
//!     { "table_id": 7, "kind": "file", "length": 7340032, "arity": 23,
//!       "file_path": "tables/rsqrt_table_i32.bin", "digest": [1,2,3,4,5,6,7,8] },
//!     { "table_id": 9, "kind": "compute", "length": 2097152, "arity": 21,
//!       "file_path": null, "digest": null }
//!   ]
//! }
//! ```
//!
//! Phase 1 only parses and validates the schema. The verifier does not consume
//! the registry yet (lookup_roots/static_table_commits stay unchanged), and the
//! registry is carried in `TrustedModelManifest::a_tables` without transcript
//! absorption. Phase 2 wires loading/digest verification into the verifier.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::compose::{ATableEntry, ATableKind};

/// Current sidecar schema version.
pub const TABLES_META_VERSION: u32 = 1;

#[derive(Debug)]
pub enum TableMetaError {
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedVersion(u32),
    DuplicateTable(usize),
    InvalidEntry(String),
}

impl fmt::Display for TableMetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableMetaError::Io(e) => write!(f, "tables.meta I/O error: {e}"),
            TableMetaError::Json(e) => write!(f, "tables.meta JSON error: {e}"),
            TableMetaError::UnsupportedVersion(v) => {
                write!(f, "unsupported tables.meta version {v}")
            }
            TableMetaError::DuplicateTable(id) => {
                write!(f, "duplicate tables.meta entry for table {id}")
            }
            TableMetaError::InvalidEntry(msg) => write!(f, "invalid tables.meta entry: {msg}"),
        }
    }
}

impl std::error::Error for TableMetaError {}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TablesMetaFile {
    version: u32,
    tables: Vec<TableMetaEntry>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TableMetaEntry {
    table_id: usize,
    kind: ATableKind,
    length: usize,
    arity: usize,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    digest: Option<[u32; 8]>,
}

/// Parse a sidecar from text. Duplicate ids, malformed entries and unknown
/// fields are rejected; an empty `tables` list is valid.
pub fn parse_tables_meta_str(text: &str)
    -> Result<BTreeMap<usize, ATableEntry>, TableMetaError>
{
    let file: TablesMetaFile = serde_json::from_str(text)
        .map_err(TableMetaError::Json)?;
    if file.version != TABLES_META_VERSION {
        return Err(TableMetaError::UnsupportedVersion(file.version));
    }
    let mut out = BTreeMap::new();
    for e in file.tables {
        if e.length == 0 || e.arity == 0 || e.arity >= usize::BITS as usize {
            return Err(TableMetaError::InvalidEntry(format!(
                "table {} has invalid length/arity ({}/{})", e.table_id, e.length, e.arity)));
        }
        let padded = 1usize.checked_shl(e.arity as u32)
            .ok_or_else(|| TableMetaError::InvalidEntry(
                format!("table {} arity {} overflows", e.table_id, e.arity)))?;
        if e.length > padded {
            return Err(TableMetaError::InvalidEntry(format!(
                "table {} length {} exceeds padded arity {}", e.table_id, e.length, padded)));
        }
        match e.kind {
            ATableKind::File => {
                let path = e.file_path.as_deref().ok_or_else(|| {
                    TableMetaError::InvalidEntry(format!(
                        "file table {} has no file_path", e.table_id))
                })?;
                if path.is_empty() || path.starts_with('/')
                    || path.split('/').any(|c| c == "..")
                {
                    return Err(TableMetaError::InvalidEntry(format!(
                        "file table {} has an unsafe file_path", e.table_id)));
                }
            }
            ATableKind::Compute => {
                if e.file_path.is_some() {
                    return Err(TableMetaError::InvalidEntry(format!(
                        "compute table {} must not carry file_path", e.table_id)));
                }
            }
        }
        let entry = ATableEntry {
            kind: e.kind,
            length: e.length,
            arity: e.arity,
            file_path: e.file_path,
            digest: e.digest,
        };
        if out.insert(e.table_id, entry).is_some() {
            return Err(TableMetaError::DuplicateTable(e.table_id));
        }
    }
    Ok(out)
}

/// Load `<model_dir>/tables.meta`. A missing file is the current empty-registry
/// behavior; malformed text or schema errors fail closed.
pub fn load_tables_meta(path: &Path)
    -> Result<BTreeMap<usize, ATableEntry>, TableMetaError>
{
    match std::fs::read_to_string(path) {
        Ok(text) => parse_tables_meta_str(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(TableMetaError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zkie-tables-meta-{}-{name}.json", std::process::id()))
    }

    #[test]
    fn parses_compute_and_file_entries() {
        let text = r#"{
            "version": 1,
            "tables": [
                {"table_id": 7, "kind": "file", "length": 7340032, "arity": 23,
                 "file_path": "tables/rsqrt_table_i32.bin",
                 "digest": [1,2,3,4,5,6,7,8]},
                {"table_id": 9, "kind": "compute", "length": 2097152, "arity": 21,
                 "file_path": null, "digest": null}
            ]
        }"#;
        let map = parse_tables_meta_str(text).expect("valid sidecar");
        assert_eq!(map.len(), 2);
        let f = &map[&7];
        assert_eq!(f.kind, ATableKind::File);
        assert_eq!(f.length, 7340032);
        assert_eq!(f.arity, 23);
        assert_eq!(f.digest, Some([1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(map[&9].kind, ATableKind::Compute);
        assert_eq!(map[&9].digest, None);
    }

    #[test]
    fn missing_sidecar_is_empty_registry() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert!(load_tables_meta(&path).expect("missing file is empty").is_empty());
    }

    #[test]
    fn bad_json_is_rejected() {
        let path = temp_path("bad");
        std::fs::write(&path, "{not json").unwrap();
        assert!(matches!(load_tables_meta(&path), Err(TableMetaError::Json(_))));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn duplicate_table_ids_are_rejected() {
        let text = r#"{"version":1,"tables":[
            {"table_id":3,"kind":"compute","length":32,"arity":5,
             "file_path":null,"digest":null},
            {"table_id":3,"kind":"compute","length":32,"arity":5,
             "file_path":null,"digest":null}
        ]}"#;
        assert!(matches!(parse_tables_meta_str(text),
            Err(TableMetaError::DuplicateTable(3))));
    }

    #[test]
    fn file_entry_requires_safe_path() {
        let no_path = r#"{"version":1,"tables":[
            {"table_id":1,"kind":"file","length":32,"arity":5,
             "file_path":null,"digest":null}
        ]}"#;
        assert!(matches!(parse_tables_meta_str(no_path),
            Err(TableMetaError::InvalidEntry(_))));
        let escaping = r#"{"version":1,"tables":[
            {"table_id":1,"kind":"file","length":32,"arity":5,
             "file_path":"../secret.bin","digest":null}
        ]}"#;
        assert!(matches!(parse_tables_meta_str(escaping),
            Err(TableMetaError::InvalidEntry(_))));
    }

    #[test]
    fn unknown_version_and_fields_are_rejected() {
        let v2 = r#"{"version":2,"tables":[]}"#;
        assert!(matches!(parse_tables_meta_str(v2),
            Err(TableMetaError::UnsupportedVersion(2))));
        let unknown = r#"{"version":1,"tables":[],"extra":1}"#;
        assert!(matches!(parse_tables_meta_str(unknown),
            Err(TableMetaError::Json(_))));
    }
}
