use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use tokio::sync::RwLock;

use crate::domain::types::{GameAction, GameJob};

#[derive(Clone, Default)]
pub struct JobQueue {
    next_id: Arc<AtomicU64>,
    items: Arc<RwLock<VecDeque<GameJob>>>,
}

impl JobQueue {
    pub async fn enqueue(
        &self,
        server_id: String,
        target_player: Option<String>,
        summary: String,
        actions: Vec<GameAction>,
    ) -> GameJob {
        let id = self.next_job_id();
        let job = GameJob {
            id,
            server_id,
            target_player,
            summary,
            actions,
        };
        self.items.write().await.push_back(job.clone());
        job
    }

    pub async fn pop_next(&self, server_id: &str) -> Option<GameJob> {
        let mut items = self.items.write().await;
        let index = items.iter().position(|item| item.server_id == server_id)?;
        items.remove(index)
    }

    fn next_job_id(&self) -> String {
        let number = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("hm-job-{number}")
    }
}
