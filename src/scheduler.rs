use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};
use tokio::select;
use tokio::time::{Duration, sleep, Instant};
use tokio_util::sync::CancellationToken;

type TaskId = u64;
type TaskData = (CancellationToken, Instant);

#[derive(Clone)]
pub struct Scheduler {
    tasks: Arc<RwLock<HashMap<TaskId, TaskData>>>,
    task_duration: Duration,
    start_token: Arc<RwLock<CancellationToken>>,
}

impl Scheduler {
    pub fn new(task_duration: Duration) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            task_duration,
            start_token: Arc::new(RwLock::new(CancellationToken::new())),
        }
    }

    pub fn add_task<F, Fut>(&self, task_id: TaskId, task: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let old_task = self.cancel_task(task_id);
        let cancel_token = CancellationToken::new();
        let token_clone = cancel_token.clone();
        let timestamp = Instant::now();
        {
            let mut tasks = self.tasks.write().unwrap();
            tasks.insert(task_id, (token_clone, timestamp));
        }
        let start_token = self.start_token.read().unwrap().clone();

        tokio::spawn(Self::task_wrapper(
            task(),
            self.task_duration,
            // Control tokens
            cancel_token,
            start_token,
            // For self removal
            self.tasks.clone(),
            task_id,
            timestamp,
        ));
        if old_task { tracing::info!("Task updated: {task_id}");
        } else { tracing::info!("Added task: {task_id}"); }
    }

    pub fn cancel_task(&self, task_id: TaskId) -> bool {
        let mut tasks = self.tasks.write().unwrap();
        if let Some((token, _)) = tasks.remove(&task_id) {
            token.cancel();
            return true;
        }
        false
    }

    pub fn complete_all(&self) {
        let mut start_token = self.start_token.write().unwrap();
        start_token.cancel();
        {
            let mut tasks = self.tasks.write().unwrap();
            tasks.clear();
        }
        tracing::info!("Completion of all tasks...");
        *start_token = CancellationToken::new();
    }

    async fn task_wrapper<F>(
        task: F,
        task_duration: Duration,
        cancel_token: CancellationToken,
        start_token: CancellationToken,
        tasks: Arc<RwLock<HashMap<TaskId, TaskData>>>,
        task_id: TaskId,
        task_timestamp: Instant,
    )
    where
        F: Future<Output = ()> + Send + 'static,
    {
        select! {
            _ = cancel_token.cancelled() => {}
            _ = start_token.cancelled() => { task.await; }
            _ = sleep(task_duration) => { task.await; }
        }
        let mut tasks = tasks.write().unwrap();
        if let Some((_, timestamp)) = tasks.get(&task_id) {
            // Task is required to delete its id after completion,
            // but when adding a task, the old task should not undo the new one
            if *timestamp == task_timestamp {
                tasks.remove(&task_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_task() {
        let scheduler = Scheduler::new(Duration::from_secs(2));
        let task_id = 1;
        let counter = Arc::new(RwLock::new(0));
        let counter_clone = Arc::clone(&counter);
        scheduler.add_task(task_id, move || async move {
            let mut count = counter_clone.write().unwrap();
            *count += 1;
        });
        assert_eq!(*counter.read().unwrap(), 0);
        sleep(Duration::from_secs(3)).await;
        assert_eq!(*counter.read().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let scheduler = Scheduler::new(Duration::from_secs(2));
        let task_id = 1;

        assert_eq!(scheduler.cancel_task(task_id), false);
        scheduler.add_task(task_id, || async {
            /* Something to do */
        });
        assert_eq!(scheduler.cancel_task(task_id), true);
    }

    #[tokio::test]
    async fn test_duplicate_task() {
        let scheduler = Scheduler::new(Duration::from_secs(2));
        let counter = Arc::new(RwLock::new(0));
        let task_id = 1;

        for _ in 0..3 {
            let counter_clone = Arc::clone(&counter);
            scheduler.add_task(task_id, move || async move {
                let mut count = counter_clone.write().unwrap();
                *count += 1;
            });
        }
        sleep(Duration::from_secs(3)).await;
        assert_eq!(*counter.read().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_complete_all() {
        let scheduler = Scheduler::new(Duration::from_secs(10));
        let counter = Arc::new(RwLock::new(0));
        let task_ids = vec![1, 2, 3];

        for task_id in task_ids.iter() {
            let counter_clone = Arc::clone(&counter);
            scheduler.add_task(*task_id, move || async move {
                let mut count = counter_clone.write().unwrap();
                *count += 1;
            });
        }
        scheduler.complete_all();
        for task_id in task_ids {
            assert_eq!(scheduler.cancel_task(task_id), false);
        }
        sleep(Duration::from_secs(3)).await;
        let final_count = *counter.read().unwrap();
        assert_eq!(final_count, 3);
    }
}
