//! Shared catalog loading helpers for theme and language managers.

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::{is_supported_config_file, read_json_or_jsonc};

/// Common interface for built-in configuration catalog identifiers.
pub trait ConfigCatalog {
    fn builtin_ids() -> &'static [&'static str];
}

/// Scans a config directory for `.json` / `.jsonc` files and parses each entry.
pub(crate) fn scan_json_config_dir<T, F>(
    dir: &Path,
    item_label: &str,
    mut parse: F,
) -> Result<Vec<T>>
where
    F: FnMut(&Path, Value) -> Result<T>,
{
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut loaded = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_file() || !is_supported_config_file(&path) {
            continue;
        }
        match read_json_or_jsonc(&path).and_then(|value| parse(&path, value)) {
            Ok(item) => loaded.push(item),
            Err(err) => eprintln!(
                "skipping custom {item_label} config '{}': {err}",
                path.display()
            ),
        }
    }
    Ok(loaded)
}

/// Creates `dest_dir` if needed and writes normalized JSON to `dest_dir/file_name`.
pub(crate) fn persist_normalized_json_config(
    dest_dir: &Path,
    file_name: &str,
    normalized: &Value,
) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;
    std::fs::write(
        dest_dir.join(file_name),
        serde_json::to_string_pretty(normalized)?,
    )?;
    Ok(())
}

/// Replaces an item with the same id or appends it.
pub(crate) fn upsert_by_id<T, IdFn>(items: &mut Vec<T>, item: T, id_of: IdFn)
where
    IdFn: Fn(&T) -> &str,
{
    let id = id_of(&item);
    if let Some(existing) = items.iter_mut().find(|existing| id_of(existing) == id) {
        *existing = item;
    } else {
        items.push(item);
    }
}

/// Returns true when `id` matches a built-in catalog identifier.
pub(crate) fn is_builtin_id(id: &str, builtin_ids: &[&str]) -> bool {
    builtin_ids.contains(&id)
}
