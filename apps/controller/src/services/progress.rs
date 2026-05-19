use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProgressSnapshot {
    pub id: String,
    pub sequence: u64,
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub message: String,
    pub done: bool,
    pub updated_at_millis: u128,
}

#[derive(Clone, Default)]
pub struct ProgressStore {
    items: Arc<Mutex<HashMap<String, ProgressSnapshot>>>,
}

impl ProgressStore {
    pub fn start(&self, id: &str, phase: impl Into<String>, detail: Option<String>) {
        self.write(id, phase.into(), detail, false);
    }

    pub fn record(&self, id: &str, phase: impl Into<String>, detail: Option<String>) {
        self.write(id, phase.into(), detail, false);
    }

    pub fn finish(&self, id: &str, phase: impl Into<String>, detail: Option<String>) {
        self.write(id, phase.into(), detail, true);
    }

    pub fn get(&self, id: &str) -> Option<ProgressSnapshot> {
        self.items.lock().ok()?.get(id).cloned()
    }

    fn write(&self, id: &str, phase: String, detail: Option<String>, done: bool) {
        let Ok(mut items) = self.items.lock() else {
            return;
        };
        let sequence = items
            .get(id)
            .map(|item| item.sequence.saturating_add(1))
            .unwrap_or(1);
        let message = match detail.as_deref().filter(|value| !value.is_empty()) {
            Some(detail) => format!("{phase}：{detail}"),
            None => phase.clone(),
        };
        items.insert(
            id.to_string(),
            ProgressSnapshot {
                id: id.to_string(),
                sequence,
                phase,
                detail,
                message,
                done,
                updated_at_millis: timestamp_millis(),
            },
        );
    }
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_store_keeps_latest_snapshot_with_sequence() {
        let store = ProgressStore::default();

        store.start("req-1", "开始处理", None);
        store.record("req-1", "调用工具", Some("rg".to_string()));

        let snapshot = store.get("req-1").unwrap();
        assert_eq!(snapshot.sequence, 2);
        assert_eq!(snapshot.message, "调用工具：rg");
        assert!(!snapshot.done);
    }
}
