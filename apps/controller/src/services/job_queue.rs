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
        let id = self.reserve_job_id();
        self.enqueue_with_id(id, server_id, target_player, summary, actions)
            .await
    }

    pub async fn enqueue_with_id(
        &self,
        id: String,
        server_id: String,
        target_player: Option<String>,
        summary: String,
        actions: Vec<GameAction>,
    ) -> GameJob {
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

    pub fn reserve_job_id(&self) -> String {
        let number = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("hm-job-{number}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_action(message: &str) -> Vec<GameAction> {
        vec![GameAction::Chat {
            message: message.to_string(),
        }]
    }

    #[tokio::test]
    async fn enqueue_assigns_incrementing_job_ids() {
        let queue = JobQueue::default();

        let first = queue
            .enqueue(
                "local-paper".to_string(),
                Some("Steve".to_string()),
                "第一个任务".to_string(),
                chat_action("one"),
            )
            .await;
        let second = queue
            .enqueue(
                "local-paper".to_string(),
                Some("Alex".to_string()),
                "第二个任务".to_string(),
                chat_action("two"),
            )
            .await;

        assert_eq!(first.id, "hm-job-1");
        assert_eq!(second.id, "hm-job-2");
    }

    #[tokio::test]
    async fn enqueue_with_reserved_id_preserves_id() {
        let queue = JobQueue::default();
        let id = queue.reserve_job_id();

        let job = queue
            .enqueue_with_id(
                id.clone(),
                "local-paper".to_string(),
                None,
                "指定任务".to_string(),
                chat_action("one"),
            )
            .await;

        assert_eq!(job.id, id);
        assert_eq!(queue.pop_next("local-paper").await.unwrap().id, "hm-job-1");
    }

    #[tokio::test]
    async fn pop_next_returns_first_matching_server_without_losing_others() {
        let queue = JobQueue::default();
        queue
            .enqueue(
                "server-a".to_string(),
                None,
                "A1".to_string(),
                chat_action("a1"),
            )
            .await;
        queue
            .enqueue(
                "server-b".to_string(),
                None,
                "B1".to_string(),
                chat_action("b1"),
            )
            .await;
        queue
            .enqueue(
                "server-a".to_string(),
                None,
                "A2".to_string(),
                chat_action("a2"),
            )
            .await;

        assert_eq!(queue.pop_next("server-b").await.unwrap().summary, "B1");
        assert_eq!(queue.pop_next("server-a").await.unwrap().summary, "A1");
        assert_eq!(queue.pop_next("server-a").await.unwrap().summary, "A2");
        assert!(queue.pop_next("server-a").await.is_none());
    }
}
