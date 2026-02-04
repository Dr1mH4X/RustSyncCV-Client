use crate::runtime::RuntimeHandle;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::runtime::Runtime;

pub struct AppState {
    pub runtime: Arc<Runtime>,
    pub handle: RuntimeHandle,
    pub logs: Mutex<Vec<String>>,
    pub paused: Mutex<bool>,
}

impl AppState {
    pub fn new(runtime: Arc<Runtime>, handle: RuntimeHandle) -> Self {
        Self {
            runtime,
            handle,
            logs: Mutex::new(Vec::new()),
            paused: Mutex::new(true),
        }
    }

    pub fn push_log(&self, line: String) {
        let mut logs = self.logs.lock();
        if logs.len() > 2000 {
            let keep = 2000;
            let remove = logs.len().saturating_sub(keep);
            if remove > 0 {
                logs.drain(0..remove);
            }
        }
        logs.push(line);
    }

    pub fn get_logs(&self) -> Vec<String> {
        self.logs.lock().clone()
    }

    pub fn set_paused(&self, paused: bool) {
        *self.paused.lock() = paused;
    }

    pub fn is_paused(&self) -> bool {
        *self.paused.lock()
    }
}
