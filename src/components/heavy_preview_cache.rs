//! Process-wide memo for heavy preview artifacts (Mermaid PNG / LaTeX SVG).
//!
//! Virtualization already skips mounting off-window blocks. This cache avoids
//! repeating disk/parse/raster work on every paint of a mounted preview.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, OnceLock};

const MEMO_CAP: usize = 128;

pub(crate) struct HeavyPreviewMemo<V> {
    inner: OnceLock<Mutex<HashMap<u64, V>>>,
}

impl<V: Clone> HeavyPreviewMemo<V> {
    pub(crate) const fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    fn map(&self) -> &Mutex<HashMap<u64, V>> {
        self.inner.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(crate) fn get(&self, key: u64) -> Option<V> {
        self.map()
            .lock()
            .ok()
            .and_then(|guard| guard.get(&key).cloned())
    }

    pub(crate) fn insert(&self, key: u64, value: V) {
        let Ok(mut guard) = self.map().lock() else {
            return;
        };
        if guard.len() >= MEMO_CAP && !guard.contains_key(&key) {
            // Drop an arbitrary entry; order is not important for a soft cap.
            if let Some(evict) = guard.keys().next().copied() {
                guard.remove(&evict);
            }
        }
        guard.insert(key, value);
    }
}

pub(crate) fn hash_key(parts: impl Hash) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut hasher = DefaultHasher::new();
    parts.hash(&mut hasher);
    hasher.finish()
}
