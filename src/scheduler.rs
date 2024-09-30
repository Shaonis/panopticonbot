use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};
use tokio::select;
use tokio::time::{Duration, sleep, Instant};
use tokio_util::sync::CancellationToken;

type TaskId = u64;
type TaskData = (CancellationToken, Instant);

/// A `Scheduler` for managing tasks with a configurable timeout.
/// Tasks are added and can be cancelled or automatically removed after a certain duration.
/// The scheduler uses cancellation tokens to manage task execution.
///
/// # Fields
///
/// * `tasks` - A map of task IDs to task data.
/// * `task_duration` - Time after which the task will start execution.
/// * `start_token` - A token used to control the startup of all tasks.
#[derive(Clone)]
pub struct Scheduler {
    tasks: Arc<RwLock<HashMap<TaskId, TaskData>>>,
    task_duration: Duration,
    start_token: CancellationToken,
}

impl Scheduler {
    /// Creates a new `Scheduler` with a specified task duration.
    ///
    /// # Arguments
    ///
    /// * `task_duration` - The duration each task is allowed to run before completion.
    pub fn new(task_duration: Duration) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            task_duration,
            start_token: CancellationToken::new(),
        }
    }

    /// Adds a new task to the scheduler.
    /// If a task with the same ID already exists, it will be cancelled and replaced.
    ///
    /// # Arguments
    ///
    /// * `task_id` - The ID of the task.
    /// * `task` - A closure that returns a `Future`, representing the task logic.
    pub fn add_task<F, Fut>(&self, task_id: TaskId, task: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let old_task = self.cancel_task(task_id);
        // Create a new cancellation token for this task
        let cancel_token = CancellationToken::new();
        let token_clone = cancel_token.clone();
        // Is needed in a task to delete one's token after completion
        let timestamp = Instant::now();
        {
            let mut tasks = self.tasks.write().unwrap();
            tasks.insert(task_id, (token_clone, timestamp));
        }
        tokio::spawn(Self::task_wrapper(
            task(),
            self.task_duration,
            // Control tokens
            cancel_token,
            self.start_token.clone(),
            // For cleanup
            self.tasks.clone(),
            task_id,
            timestamp,
        ));

        if old_task {
            tracing::info!("Task updated: {task_id}");
        } else {
            tracing::info!("Added task: {task_id}");
        }
    }

    /// Cancels a task by its ID.
    /// Returns `true` if the task was successfully cancelled, `false` if no such task exists.
    pub fn cancel_task(&self, task_id: TaskId) -> bool {
        let tasks = self.tasks.read().unwrap();
        if let Some((token, _)) = tasks.get(&task_id) {
            token.cancel();
            return true;
        }
        false
    }

    /// Completes all tasks, canceling the shared start_token and replacing it with a new one.
    /// Waits until all tasks are either completed or canceled.
    pub async fn complete_all(&mut self) {
        // Canceling the start token gives all tasks a signal to start executing
        self.start_token.cancel();
        self.start_token = CancellationToken::new();
        tracing::info!("Completion of all tasks...");

        // Wait until the task map is empty, indicating all tasks have finished
        loop {
            { // Additional scope to not take away map access permanently
                let tasks = self.tasks.read().unwrap();
                if tasks.is_empty() { break; }
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Internal function to wrap task execution logic.
    /// Handles cancellation and ensures task cleanup after execution.
    ///
    /// # Arguments
    ///
    /// * `task` - The task to execute.
    /// * `task_duration` - The duration after which the task is forcefully completed.
    /// * `cancel_token` - Token to cancel this specific task.
    /// * `start_token` - The token to start this task.
    /// * `tasks` - Shared reference to the task map.
    /// * `task_id` - The ID of the task.
    /// * `task_timestamp` - The timestamp of when the task was added.
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
            _ = cancel_token.cancelled() => {},
            _ = start_token.cancelled() => { task.await; },
            _ = sleep(task_duration) => { task.await; },
        }
        
        // Task is required to delete its id after completion
        let mut tasks = tasks.write().unwrap();
        if let Some((_, timestamp)) = tasks.get(&task_id) {
            // When adding a task, the old task should not cancel the new task,
            // the old task may not have time to complete
            // before the cancel_token is replaced with the new one
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
        let mut scheduler = Scheduler::new(Duration::from_secs(10));
        let counter = Arc::new(RwLock::new(0));
        let task_ids = vec![1, 2, 3];

        for task_id in task_ids.iter() {
            let counter_clone = Arc::clone(&counter);
            scheduler.add_task(*task_id, move || async move {
                let mut count = counter_clone.write().unwrap();
                *count += 1;
            });
        }
        scheduler.complete_all().await;
        for task_id in task_ids {
            assert_eq!(scheduler.cancel_task(task_id), false);
        }
        let final_count = *counter.read().unwrap();
        assert_eq!(final_count, 3);
    }
}
