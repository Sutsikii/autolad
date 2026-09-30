//! Background jobs (renders). Long work must not block a tool call, and must be
//! cancellable: a job is a spawned task, and cancelling aborts it, which drops the
//! render future and thereby kills ffmpeg.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::task::AbortHandle;

use crate::error::EngineError;

/// How long to wait after aborting before deleting the partial output, so the
/// killed ffmpeg has released its file handle.
const CANCEL_CLEANUP_DELAY: Duration = Duration::from_millis(400);

/// Progress is stored as thousandths so it fits an atomic integer.
const PROGRESS_SCALE: f64 = 1000.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    Running,
    Done { size_bytes: u64 },
    Failed { error: String },
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub id: String,
    pub output: PathBuf,
    /// Fraction in `[0, 1]`.
    pub progress: f64,
    pub elapsed_secs: f64,
    #[serde(flatten)]
    pub state: JobState,
}

struct Inner {
    state: JobState,
    finished_at: Option<Instant>,
}

struct Job {
    id: String,
    output: PathBuf,
    started: Instant,
    progress: AtomicU32,
    inner: Mutex<Inner>,
    abort: Mutex<Option<AbortHandle>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Job {
    fn finish(&self, state: JobState) {
        let mut inner = lock(&self.inner);
        // A cancel that raced with completion wins: don't overwrite a terminal state.
        if inner.state == JobState::Running {
            inner.state = state;
            inner.finished_at = Some(Instant::now());
        }
    }

    fn status(&self) -> JobStatus {
        let inner = lock(&self.inner);
        let end = inner.finished_at.unwrap_or_else(Instant::now);
        let progress = match inner.state {
            JobState::Done { .. } => 1.0,
            _ => f64::from(self.progress.load(Ordering::Relaxed)) / PROGRESS_SCALE,
        };
        JobStatus {
            id: self.id.clone(),
            output: self.output.clone(),
            progress,
            elapsed_secs: end.duration_since(self.started).as_secs_f64(),
            state: inner.state.clone(),
        }
    }
}

#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
}

pub type ProgressSink = Arc<dyn Fn(f64) + Send + Sync>;

impl Jobs {
    /// Starts `work` in the background and returns its job id.
    /// `work` receives a sink to report progress in `[0, 1]`.
    pub fn spawn<F, Fut>(&self, output: PathBuf, work: F) -> String
    where
        F: FnOnce(ProgressSink) -> Fut,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let id = format!("render-{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
        let job = Arc::new(Job {
            id: id.clone(),
            output: output.clone(),
            started: Instant::now(),
            progress: AtomicU32::new(0),
            inner: Mutex::new(Inner {
                state: JobState::Running,
                finished_at: None,
            }),
            abort: Mutex::new(None),
        });

        let progress_job = job.clone();
        let sink: ProgressSink = Arc::new(move |p: f64| {
            let scaled = (p.clamp(0.0, 1.0) * PROGRESS_SCALE) as u32;
            progress_job.progress.store(scaled, Ordering::Relaxed);
        });

        let fut = work(sink);
        let run_job = job.clone();
        let handle = tokio::spawn(async move {
            let state = match fut.await {
                Ok(()) => JobState::Done {
                    size_bytes: std::fs::metadata(&run_job.output).map_or(0, |m| m.len()),
                },
                Err(error) => JobState::Failed { error },
            };
            run_job.finish(state);
        });
        *lock(&job.abort) = Some(handle.abort_handle());

        lock(&self.jobs).insert(id.clone(), job);
        id
    }

    pub fn status(&self, id: &str) -> Result<JobStatus, EngineError> {
        Ok(self.get(id)?.status())
    }

    /// Stops a running job and removes its partial output. A finished job is left as is.
    pub async fn cancel(&self, id: &str) -> Result<JobStatus, EngineError> {
        let job = self.get(id)?;
        let was_running = lock(&job.inner).state == JobState::Running;
        if was_running {
            if let Some(handle) = lock(&job.abort).take() {
                handle.abort();
            }
            job.finish(JobState::Cancelled);
            tokio::time::sleep(CANCEL_CLEANUP_DELAY).await;
            // Best effort: the file may not exist yet, or ffmpeg may still be exiting.
            let _ = tokio::fs::remove_file(&job.output).await;
        }
        Ok(job.status())
    }

    fn get(&self, id: &str) -> Result<Arc<Job>, EngineError> {
        lock(&self.jobs)
            .get(id)
            .cloned()
            .ok_or_else(|| EngineError::UnknownJob(id.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn wait_until_finished(jobs: &Jobs, id: &str) -> JobStatus {
        for _ in 0..200 {
            let status = jobs.status(id).unwrap();
            if status.state != JobState::Running {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("job {id} never finished");
    }

    #[tokio::test]
    async fn successful_job_reports_done_with_full_progress() {
        let jobs = Jobs::default();
        let id = jobs.spawn(PathBuf::from("none.mp4"), |progress| async move {
            progress(0.5);
            Ok(())
        });
        let status = wait_until_finished(&jobs, &id).await;
        assert_eq!(status.state, JobState::Done { size_bytes: 0 });
        assert_eq!(status.progress, 1.0);
    }

    #[tokio::test]
    async fn failing_job_keeps_its_error() {
        let jobs = Jobs::default();
        let id = jobs.spawn(PathBuf::from("none.mp4"), |_| async { Err("boom".into()) });
        let status = wait_until_finished(&jobs, &id).await;
        assert_eq!(
            status.state,
            JobState::Failed {
                error: "boom".into()
            }
        );
    }

    #[tokio::test]
    async fn progress_is_visible_while_running() {
        let jobs = Jobs::default();
        let id = jobs.spawn(PathBuf::from("none.mp4"), |progress| async move {
            progress(0.25);
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(())
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let status = jobs.status(&id).unwrap();
        assert_eq!(status.state, JobState::Running);
        assert_eq!(status.progress, 0.25);
        jobs.cancel(&id).await.unwrap();
    }

    #[tokio::test]
    async fn cancel_stops_the_work_and_deletes_partial_output() {
        let out = std::env::temp_dir().join(format!("autolad-job-{}.mp4", std::process::id()));
        std::fs::write(&out, b"partial").unwrap();

        let jobs = Jobs::default();
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = finished.clone();
        let id = jobs.spawn(out.clone(), |_| async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            flag.store(true, Ordering::Relaxed);
            Ok(())
        });

        let status = jobs.cancel(&id).await.unwrap();
        assert_eq!(status.state, JobState::Cancelled);
        assert!(!out.exists(), "partial output should be removed");
        assert!(!finished.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn cancelling_a_finished_job_changes_nothing() {
        let jobs = Jobs::default();
        let id = jobs.spawn(PathBuf::from("none.mp4"), |_| async { Ok(()) });
        wait_until_finished(&jobs, &id).await;
        let status = jobs.cancel(&id).await.unwrap();
        assert!(matches!(status.state, JobState::Done { .. }));
    }

    #[tokio::test]
    async fn unknown_job_is_an_error() {
        let jobs = Jobs::default();
        assert!(matches!(
            jobs.status("nope"),
            Err(EngineError::UnknownJob(_))
        ));
    }
}
