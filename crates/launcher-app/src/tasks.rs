//! Watch channels retain authoritative snapshots for late or slow subscribers.
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
pub type Cancellation = CancellationToken;

use crate::{
    AppError, Result,
    domain::{InstanceId, TaskId},
    downloads::SourceId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStage {
    Metadata,
    Download,
    Verify,
    Install,
    Java,
    Authenticate,
    Launch,
    Cleanup,
}

#[derive(Debug, Clone)]
pub struct TaskFailure {
    pub stage: TaskStage,
    pub resource: Option<String>,
    pub source: Option<SourceId>,
    pub retryable: bool,
    /// Adapter must redact credentials before constructing a user-visible error.
    pub cause: String,
}

#[derive(Debug, Clone, Default)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub source: Option<SourceId>,
}

#[derive(Debug, Clone)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub instance_id: Option<InstanceId>,
    pub state: TaskState,
    pub progress: DownloadProgress,
    pub failure: Option<TaskFailure>,
}

#[derive(Clone)]
pub struct TaskHandle {
    state: watch::Sender<TaskSnapshot>,
    cancellation: CancellationToken,
}

#[derive(Clone)]
pub struct TaskContext {
    handle: TaskHandle,
}

pub enum TaskOutcome {
    Succeeded,
    Failed(TaskFailure),
    Cancelled,
}

impl TaskHandle {
    pub fn new(instance_id: Option<InstanceId>) -> (Self, TaskContext) {
        let (state, _) = watch::channel(TaskSnapshot {
            id: TaskId::new(),
            instance_id,
            state: TaskState::Queued,
            progress: DownloadProgress::default(),
            failure: None,
        });
        let handle = Self {
            state,
            cancellation: CancellationToken::new(),
        };
        let context = TaskContext {
            handle: handle.clone(),
        };
        (handle, context)
    }

    pub fn snapshot(&self) -> TaskSnapshot {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<TaskSnapshot> {
        self.state.subscribe()
    }

    /// Request only. The worker acknowledges cancellation after awaiting child cleanup.
    pub fn request_cancel(&self) {
        self.state.send_if_modified(|snapshot| {
            if snapshot.state.is_terminal() || snapshot.state == TaskState::Cancelling {
                return false;
            }
            snapshot.state = TaskState::Cancelling;
            self.cancellation.cancel();
            true
        });
    }
}

impl TaskContext {
    pub fn child_token(&self) -> CancellationToken {
        self.handle.cancellation.child_token()
    }

    pub fn start(&self) -> Result<()> {
        let mut result = Ok(());
        self.handle.state.send_if_modified(|snapshot| {
            if snapshot.state != TaskState::Queued {
                result = Err(AppError::InvalidTaskTransition(format!(
                    "{:?} -> Running",
                    snapshot.state
                )));
                return false;
            }
            snapshot.state = TaskState::Running;
            true
        });
        result
    }

    /// Watch coalesces intermediate progress; a terminal snapshot cannot be overwritten.
    pub fn progress(&self, progress: DownloadProgress) -> Result<()> {
        let mut result = Ok(());
        self.handle.state.send_if_modified(|snapshot| {
            if !matches!(snapshot.state, TaskState::Running | TaskState::Cancelling) {
                result = Err(AppError::InvalidTaskTransition(
                    "任务未运行，不能更新进度".into(),
                ));
                return false;
            }
            snapshot.progress = progress;
            true
        });
        result
    }

    /// Call only after children, locks and temporary resources have been cleaned up.
    pub fn finish(&self, outcome: TaskOutcome) -> Result<()> {
        let mut result = Ok(());
        self.handle.state.send_if_modified(|snapshot| {
            if !matches!(snapshot.state, TaskState::Running | TaskState::Cancelling) {
                result = Err(AppError::InvalidTaskTransition("任务不能进入终态".into()));
                return false;
            }
            match outcome {
                TaskOutcome::Succeeded => {
                    snapshot.state = if snapshot.state == TaskState::Cancelling {
                        TaskState::Cancelled
                    } else {
                        TaskState::Succeeded
                    };
                }
                TaskOutcome::Failed(failure) => {
                    snapshot.state = TaskState::Failed;
                    snapshot.failure = Some(failure);
                }
                TaskOutcome::Cancelled if snapshot.state == TaskState::Cancelling => {
                    snapshot.state = TaskState::Cancelled;
                }
                TaskOutcome::Cancelled => {
                    result = Err(AppError::InvalidTaskTransition("尚未请求取消".into()));
                    return false;
                }
            }
            true
        });
        result
    }
}
