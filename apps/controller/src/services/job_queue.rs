use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use tokio::sync::RwLock;

use crate::domain::types::{ChatAttachment, GameAction, GameJob};

#[derive(Clone, Default)]
pub struct JobQueue {
    next_id: Arc<AtomicU64>,
    items: Arc<RwLock<VecDeque<GameJob>>>,
    statuses: Arc<RwLock<HashMap<String, JobQueueStatus>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobQueuePhase {
    Pending,
    Claimed,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub struct JobQueueStatus {
    pub phase: JobQueuePhase,
    pub job: Option<GameJob>,
    pub message: Option<String>,
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
        self.statuses.write().await.insert(
            job.id.clone(),
            JobQueueStatus {
                phase: JobQueuePhase::Pending,
                job: Some(job.clone()),
                message: None,
            },
        );
        job
    }

    pub async fn pop_next(&self, server_id: &str) -> Option<GameJob> {
        let mut items = self.items.write().await;
        let index = items.iter().position(|item| item.server_id == server_id)?;
        let job = items.remove(index)?;
        self.statuses.write().await.insert(
            job.id.clone(),
            JobQueueStatus {
                phase: JobQueuePhase::Claimed,
                job: Some(job.clone()),
                message: None,
            },
        );
        Some(job)
    }

    pub async fn merge_pending_scan_job(
        &self,
        server_id: &str,
        target_player: Option<&str>,
        summary: String,
        actions: &[GameAction],
    ) -> Option<GameJob> {
        let (new_text, new_attachments) = scan_action_payload(actions)?;
        let mut items = self.items.write().await;
        let job = items.iter_mut().find(|job| {
            job.server_id == server_id
                && same_target_player(job.target_player.as_deref(), target_player)
                && scan_action_payload(&job.actions).is_some()
        })?;

        job.summary = summary;
        update_scan_action(&mut job.actions, new_text, new_attachments);
        self.statuses.write().await.insert(
            job.id.clone(),
            JobQueueStatus {
                phase: JobQueuePhase::Pending,
                job: Some(job.clone()),
                message: None,
            },
        );
        Some(job.clone())
    }

    pub async fn status(&self, job_id: &str) -> Option<JobQueueStatus> {
        self.statuses.read().await.get(job_id).cloned()
    }

    pub async fn mark_result(&self, job_id: &str, ok: bool, message: Option<String>) {
        let mut statuses = self.statuses.write().await;
        let entry = statuses
            .entry(job_id.to_string())
            .or_insert_with(|| JobQueueStatus {
                phase: JobQueuePhase::Claimed,
                job: None,
                message: None,
            });
        entry.phase = if ok {
            JobQueuePhase::Succeeded
        } else {
            JobQueuePhase::Failed
        };
        entry.message = message;
    }

    pub fn reserve_job_id(&self) -> String {
        let number = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("hm-job-{number}")
    }
}

fn scan_action_payload(actions: &[GameAction]) -> Option<(&str, &[ChatAttachment])> {
    actions.iter().find_map(|action| match action {
        GameAction::ScanNearbyAndPlan { text, attachments } => {
            Some((text.as_str(), attachments.as_slice()))
        }
        _ => None,
    })
}

fn update_scan_action(
    actions: &mut [GameAction],
    new_text: &str,
    new_attachments: &[ChatAttachment],
) {
    for action in actions {
        if let GameAction::ScanNearbyAndPlan { text, attachments } = action {
            *text = merge_scan_text(text, new_text);
            merge_attachments(attachments, new_attachments);
            return;
        }
    }
}

fn merge_scan_text(existing: &str, new_text: &str) -> String {
    let existing = existing.trim();
    let new_text = new_text.trim();
    if new_text.is_empty() || existing.contains(new_text) {
        existing.to_string()
    } else if existing.is_empty() || new_text.contains(existing) {
        new_text.to_string()
    } else {
        format!("{existing}\n最新补充：{new_text}")
    }
}

fn merge_attachments(existing: &mut Vec<ChatAttachment>, new_attachments: &[ChatAttachment]) {
    for attachment in new_attachments {
        if !existing.contains(attachment) {
            existing.push(attachment.clone());
        }
    }
}

fn same_target_player(left: Option<&str>, right: Option<&str>) -> bool {
    left.unwrap_or("")
        .trim()
        .eq_ignore_ascii_case(right.unwrap_or("").trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_action(message: &str) -> Vec<GameAction> {
        vec![GameAction::Chat {
            message: message.to_string(),
        }]
    }

    fn scan_action(text: &str) -> Vec<GameAction> {
        vec![GameAction::ScanNearbyAndPlan {
            text: text.to_string(),
            attachments: Vec::new(),
        }]
    }

    fn image_attachment(path: &str) -> ChatAttachment {
        ChatAttachment {
            kind: crate::domain::types::ChatAttachmentKind::Image,
            source: crate::domain::types::ChatAttachmentSource::LocalPath {
                path: path.to_string(),
            },
            file_name: Some("reference.png".to_string()),
            mime_type: Some("image/png".to_string()),
        }
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
        assert_eq!(
            queue.status("hm-job-1").await.unwrap().phase,
            JobQueuePhase::Pending
        );
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
        assert_eq!(
            queue.status("hm-job-2").await.unwrap().phase,
            JobQueuePhase::Claimed
        );
        assert_eq!(queue.pop_next("server-a").await.unwrap().summary, "A1");
        assert_eq!(queue.pop_next("server-a").await.unwrap().summary, "A2");
        assert!(queue.pop_next("server-a").await.is_none());
    }

    #[tokio::test]
    async fn merge_pending_scan_job_keeps_one_job_and_latest_requirements() {
        let queue = JobQueue::default();
        let first = queue
            .enqueue(
                "hmcl-lan".to_string(),
                Some("Charles".to_string()),
                "改造现有建筑".to_string(),
                vec![GameAction::ScanNearbyAndPlan {
                    text: "按参考图放大摩天轮".to_string(),
                    attachments: vec![image_attachment("/tmp/first.png")],
                }],
            )
            .await;
        let merged = queue
            .merge_pending_scan_job(
                "hmcl-lan",
                Some("charles"),
                "继续优化现有建筑".to_string(),
                &[GameAction::ScanNearbyAndPlan {
                    text: "每个箱子都更复杂，不要做小模型".to_string(),
                    attachments: vec![image_attachment("/tmp/second.png")],
                }],
            )
            .await
            .unwrap();

        assert_eq!(merged.id, first.id);
        let job = queue.pop_next("hmcl-lan").await.unwrap();
        assert_eq!(job.summary, "继续优化现有建筑");
        assert!(queue.pop_next("hmcl-lan").await.is_none());
        assert!(matches!(
            &job.actions[0],
            GameAction::ScanNearbyAndPlan { text, attachments }
                if text.contains("按参考图放大摩天轮")
                    && text.contains("每个箱子都更复杂")
                    && attachments.len() == 2
        ));
    }

    #[tokio::test]
    async fn merge_pending_scan_job_ignores_different_target_player() {
        let queue = JobQueue::default();
        queue
            .enqueue(
                "hmcl-lan".to_string(),
                Some("Charles".to_string()),
                "改造现有建筑".to_string(),
                scan_action("改造 A"),
            )
            .await;

        assert!(queue
            .merge_pending_scan_job(
                "hmcl-lan",
                Some("Alex"),
                "改造现有建筑".to_string(),
                &scan_action("改造 B"),
            )
            .await
            .is_none());
    }

    #[tokio::test]
    async fn status_tracks_claimed_and_result_phases() {
        let queue = JobQueue::default();
        let job = queue
            .enqueue(
                "hmcl-lan".to_string(),
                None,
                "建造小屋".to_string(),
                chat_action("one"),
            )
            .await;

        assert_eq!(
            queue.status(&job.id).await.unwrap().phase,
            JobQueuePhase::Pending
        );
        assert_eq!(queue.pop_next("hmcl-lan").await.unwrap().id, job.id);
        assert_eq!(
            queue.status(&job.id).await.unwrap().phase,
            JobQueuePhase::Claimed
        );

        queue
            .mark_result(&job.id, false, Some("执行失败".to_string()))
            .await;
        let status = queue.status(&job.id).await.unwrap();
        assert_eq!(status.phase, JobQueuePhase::Failed);
        assert_eq!(status.message.as_deref(), Some("执行失败"));
    }
}
