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
        self.items
            .read()
            .await
            .values()
            .find(|item| item.tags.iter().any(|value| value == tag))
            .cloned()
    }

    pub async fn save(
        &self,
        blueprint: Blueprint,
    ) -> Result<Blueprint, Box<dyn std::error::Error + Send + Sync>> {
        let file_path = self.file_path(&blueprint.id);
        let json = serde_json::to_string_pretty(&blueprint)?;
        tokio::fs::write(file_path, json).await?;
        self.items
            .write()
            .await
            .insert(blueprint.id.clone(), blueprint.clone());
        Ok(blueprint)
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

fn safe_id(id: &str) -> String {
    id.chars()
        .filter(|value| value.is_ascii_alphanumeric() || *value == '-' || *value == '_')
        .collect()
}
