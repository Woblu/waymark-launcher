//! Everything the core reports while it works. The desktop app forwards these
//! to the UI and the CLI prints them.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::crash::CrashReport;

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    TaskStarted {
        id: String,
        title: String,
    },
    TaskProgress {
        id: String,
        done: u64,
        total: u64,
        detail: String,
    },
    TaskFinished {
        id: String,
    },
    TaskFailed {
        id: String,
        message: String,
    },
    GameStarted {
        instance: String,
        pid: u32,
    },
    GameLog {
        instance: String,
        line: String,
        error: bool,
    },
    GameExited {
        instance: String,
        code: Option<i32>,
        played_secs: u64,
        crash: Option<CrashReport>,
    },
    ServerStopped {
        instance: String,
    },
}

pub type Emitter = Arc<dyn Fn(Event) + Send + Sync>;

pub fn silent() -> Emitter {
    Arc::new(|_| {})
}

/// A unit of work shown in the UI. Progress is throttled so a download of
/// thousands of small files can't flood the IPC bridge and stall rendering.
pub struct Task {
    id: String,
    emit: Emitter,
    last: Mutex<Instant>,
}

const PROGRESS_INTERVAL: Duration = Duration::from_millis(66);

impl Task {
    pub fn start(emit: &Emitter, title: impl Into<String>) -> Self {
        let id = uuid::Uuid::new_v4().simple().to_string();
        emit(Event::TaskStarted {
            id: id.clone(),
            title: title.into(),
        });
        Task {
            id,
            emit: emit.clone(),
            last: Mutex::new(Instant::now() - PROGRESS_INTERVAL),
        }
    }

    pub fn progress(&self, done: u64, total: u64, detail: &str) {
        let mut last = self.last.lock().expect("progress clock poisoned");
        if done < total && last.elapsed() < PROGRESS_INTERVAL {
            return;
        }
        *last = Instant::now();
        (self.emit)(Event::TaskProgress {
            id: self.id.clone(),
            done,
            total,
            detail: detail.to_string(),
        });
    }

    pub fn finish(self) {
        (self.emit)(Event::TaskFinished { id: self.id });
    }

    pub fn fail(self, message: impl Into<String>) {
        (self.emit)(Event::TaskFailed {
            id: self.id,
            message: message.into(),
        });
    }
}
