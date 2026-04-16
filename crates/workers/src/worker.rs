//! Worker loop: polls the job queue, dispatches to handlers, manages lifecycle.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::watch;
use tokio::time;

use crate::handler::JobHandler;
use crate::queue::{self, JobKind};

/// Configuration for a background worker instance.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Unique identifier for this worker instance (e.g. hostname + replica ID).
    pub worker_id: String,
    /// How many jobs to claim per poll cycle.
    pub batch_size: i32,
    /// Seconds a job lock is held before it's considered stale.
    pub lock_duration_seconds: i64,
    /// Interval between poll cycles when the queue is empty.
    pub poll_interval: Duration,
    /// Interval between stale job recovery sweeps.
    pub recovery_interval: Duration,
    /// Retention period (hours) for completed/dead jobs before cleanup.
    pub retention_hours: i64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            worker_id: format!("worker-{}", uuid::Uuid::new_v4()),
            batch_size: 5,
            lock_duration_seconds: 300, // 5 minutes
            poll_interval: Duration::from_secs(5),
            recovery_interval: Duration::from_secs(60),
            retention_hours: 168, // 7 days
        }
    }
}

/// A background worker that processes jobs from the PostgreSQL-backed queue.
pub struct Worker {
    db: PgPool,
    config: WorkerConfig,
    handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
}

impl Worker {
    pub fn new(db: PgPool, config: WorkerConfig) -> Self {
        Self {
            db,
            config,
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for a specific job kind.
    pub fn register_handler(&mut self, kind: JobKind, handler: Arc<dyn JobHandler>) {
        self.handlers.insert(kind, handler);
    }

    /// Run the worker loop until the shutdown signal is received.
    ///
    /// The `shutdown_rx` watch channel should receive `true` when the process
    /// is shutting down (e.g. on SIGTERM).
    pub async fn run(&self, mut shutdown_rx: watch::Receiver<bool>) {
        tracing::info!(
            worker_id = %self.config.worker_id,
            batch_size = self.config.batch_size,
            poll_interval_ms = self.config.poll_interval.as_millis() as u64,
            "Background worker started"
        );

        let mut poll_interval = time::interval(self.config.poll_interval);
        let mut recovery_interval = time::interval(self.config.recovery_interval);
        poll_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        recovery_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    self.poll_and_process().await;
                }
                _ = recovery_interval.tick() => {
                    self.recover_stale().await;
                    self.cleanup_old().await;
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!(
                            worker_id = %self.config.worker_id,
                            "Background worker shutting down"
                        );
                        break;
                    }
                }
            }
        }
    }

    async fn poll_and_process(&self) {
        let jobs = match queue::claim_jobs(
            &self.db,
            &self.config.worker_id,
            self.config.batch_size,
            self.config.lock_duration_seconds,
        )
        .await
        {
            Ok(jobs) => jobs,
            Err(err) => {
                tracing::error!(error = %err, "Failed to claim jobs");
                return;
            }
        };

        for job in jobs {
            let handler = match self.handlers.get(&job.kind) {
                Some(handler) => Arc::clone(handler),
                None => {
                    let msg = format!("No handler registered for job kind {:?}", job.kind);
                    tracing::error!(job_id = %job.id, msg);
                    let _ = queue::fail_job(&self.db, job.id, &msg, job.attempts, job.max_attempts)
                        .await;
                    continue;
                }
            };

            let start = std::time::Instant::now();

            match handler.handle(job.payload.clone()).await {
                Ok(()) => {
                    let elapsed = start.elapsed();
                    tracing::info!(
                        job_id = %job.id,
                        kind = ?job.kind,
                        elapsed_ms = elapsed.as_millis() as u64,
                        "Job completed successfully"
                    );
                    if let Err(err) = queue::complete_job(&self.db, job.id).await {
                        tracing::error!(
                            job_id = %job.id,
                            error = %err,
                            "Failed to mark job as completed"
                        );
                    }
                }
                Err(error) => {
                    let elapsed = start.elapsed();
                    tracing::warn!(
                        job_id = %job.id,
                        kind = ?job.kind,
                        attempt = job.attempts,
                        elapsed_ms = elapsed.as_millis() as u64,
                        error = %error,
                        "Job failed"
                    );
                    if let Err(err) =
                        queue::fail_job(&self.db, job.id, &error, job.attempts, job.max_attempts)
                            .await
                    {
                        tracing::error!(
                            job_id = %job.id,
                            error = %err,
                            "Failed to record job failure"
                        );
                    }
                }
            }
        }
    }

    async fn recover_stale(&self) {
        if let Err(err) = queue::recover_stale_jobs(&self.db).await {
            tracing::error!(error = %err, "Failed to recover stale jobs");
        }
    }

    async fn cleanup_old(&self) {
        if let Err(err) = queue::cleanup_finished_jobs(&self.db, self.config.retention_hours).await
        {
            tracing::error!(error = %err, "Failed to clean up old jobs");
        }
    }
}
