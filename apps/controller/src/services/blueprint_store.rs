use std::{collections::HashMap, path::PathBuf, sync::Arc};

use tokio::sync::RwLock;

use crate::domain::types::Blueprint;

#[derive(Clone)]
pub struct BlueprintStore {
    data_dir: Arc<PathBuf>,
    items: Arc<RwLock<HashMap<String, Blueprint>>>,
}

impl BlueprintStore {
    pub async fn new(data_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        tokio::fs::create_dir_all(&data_dir).await?;
        let store = Self {
            data_dir: Arc::new(data_dir),
            items: Arc::new(RwLock::new(HashMap::new())),
        };
        store.load_from_disk().await?;
        Ok(store)
    }

    pub async fn list(&self) -> Vec<Blueprint> {
        let mut items = self
            .items
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.id.cmp(&right.id));
        items
    }

    pub async fn get(&self, id: &str) -> Option<Blueprint> {
        self.items.read().await.get(id).cloned()
    }

    pub async fn first_by_tag(&self, tag: &str) -> Option<Blueprint> {
        self.list()
            .await
            .into_iter()
            .find(|item| item.tags.iter().any(|value| value == tag))
    }

    pub async fn save(
        &self,
        blueprint: Blueprint,
    ) -> Result<Blueprint, Box<dyn std::error::Error + Send + Sync>> {
        validate_blueprint_id(&blueprint.id)?;

        let file_path = self.file_path(&blueprint.id);
        let json = serde_json::to_string_pretty(&blueprint)?;
        tokio::fs::write(file_path, json).await?;
        self.items
            .write()
            .await
            .insert(blueprint.id.clone(), blueprint.clone());
        Ok(blueprint)
    }

    pub async fn delete(&self, id: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        validate_blueprint_id(id)?;

        let removed = self.items.write().await.remove(id).is_some();
        let file_path = self.file_path(id);
        match tokio::fs::remove_file(file_path).await {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(removed),
            Err(error) => Err(error.into()),
        }
    }

    async fn load_from_disk(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut dir = tokio::fs::read_dir(self.data_dir.as_ref()).await?;
        let mut loaded = HashMap::new();

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let content = tokio::fs::read_to_string(&path).await?;
            let blueprint = serde_json::from_str::<Blueprint>(&content)?;
            loaded.insert(blueprint.id.clone(), blueprint);
        }

        *self.items.write().await = loaded;
        Ok(())
    }

    fn file_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(format!("{}.json", safe_id(id)))
    }
}

fn validate_blueprint_id(id: &str) -> Result<(), std::io::Error> {
    if id.is_empty() || safe_id(id) != id {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "蓝图 ID 只能使用英文字母、数字、横线和下划线",
        ));
    }

    Ok(())
}

fn safe_id(id: &str) -> String {
    id.chars()
        .filter(|value| value.is_ascii_alphanumeric() || *value == '-' || *value == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{BlueprintBlock, BlueprintSize, MaterialCount};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(name: &str) -> PathBuf {
        let number = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blockwright-blueprint-store-{name}-{}-{number}",
            std::process::id()
        ))
    }

    fn blueprint(id: &str, tags: &[&str]) -> Blueprint {
        Blueprint {
            id: id.to_string(),
            name: format!("蓝图 {id}"),
            description: "测试蓝图".to_string(),
            size: BlueprintSize {
                width: 1,
                height: 1,
                depth: 1,
            },
            materials: vec![MaterialCount {
                material: "minecraft:oak_planks".to_string(),
                count: 1,
            }],
            blocks: vec![BlueprintBlock {
                x: 0,
                y: 0,
                z: 0,
                material: "minecraft:oak_planks".to_string(),
            }],
            tags: tags.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn saves_loads_and_lists_blueprints_by_id() {
        let data_dir = temp_dir("sorted");
        let store = BlueprintStore::new(data_dir.clone()).await.unwrap();
        store.save(blueprint("z-house", &["house"])).await.unwrap();
        store.save(blueprint("a-house", &["house"])).await.unwrap();

        let reloaded = BlueprintStore::new(data_dir).await.unwrap();
        let ids = reloaded
            .list()
            .await
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["a-house", "z-house"]);
        assert!(reloaded.get("a-house").await.is_some());
    }

    #[tokio::test]
    async fn finds_first_blueprint_by_tag_deterministically() {
        let store = BlueprintStore::new(temp_dir("tag")).await.unwrap();
        store.save(blueprint("z-house", &["house"])).await.unwrap();
        store.save(blueprint("a-house", &["house"])).await.unwrap();

        let item = store.first_by_tag("house").await.unwrap();

        assert_eq!(item.id, "a-house");
    }

    #[tokio::test]
    async fn rejects_empty_or_unsafe_blueprint_ids() {
        let store = BlueprintStore::new(temp_dir("safe-id")).await.unwrap();

        let empty_result = store.save(blueprint("", &[])).await;
        let unsafe_result = store.save(blueprint("../house", &[])).await;

        assert!(empty_result.is_err());
        assert!(unsafe_result.is_err());
    }

    #[tokio::test]
    async fn deletes_blueprint_from_memory_and_disk() {
        let data_dir = temp_dir("delete");
        let store = BlueprintStore::new(data_dir.clone()).await.unwrap();
        store
            .save(blueprint("old-house", &["house"]))
            .await
            .unwrap();

        assert!(store.delete("old-house").await.unwrap());
        assert!(store.get("old-house").await.is_none());

        let reloaded = BlueprintStore::new(data_dir).await.unwrap();
        assert!(reloaded.get("old-house").await.is_none());
    }
}
