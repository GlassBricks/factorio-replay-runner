use lazy_static::lazy_static;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

pub type ProcessId = u32;

#[derive(Clone)]
pub struct ProcessManager {
    processes: Arc<Mutex<HashSet<ProcessId>>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn register(&self, id: ProcessId) {
        self.processes.lock().unwrap().insert(id);
    }

    pub fn unregister(&self, id: ProcessId) {
        self.processes.lock().unwrap().remove(&id);
    }

    pub fn kill_all(&self) {
        log::info!("Killing all processes");
        let mut processes = self.processes.lock().unwrap();
        for pid in processes.drain() {
            // Send SIGTERM to the process
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    pub fn process_count(&self) -> usize {
        self.processes.lock().unwrap().len()
    }
}

lazy_static! {
    pub static ref GLOBAL_PROCESS_MANAGER: ProcessManager = ProcessManager::new();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_process_registration_and_cleanup() {
        // Create a local process manager for isolated testing
        let manager = ProcessManager::new();

        // Spawn a simple sleep process
        let mut child = async_process::Command::new("sleep")
            .arg("10")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        let pid = child.id();
        manager.register(pid);

        assert_eq!(manager.process_count(), 1);

        // Kill the process manually and unregister
        child.kill().ok();
        manager.unregister(pid);

        assert_eq!(manager.process_count(), 0);
    }

    #[tokio::test]
    async fn test_kill_all_processes() {
        // Create a local process manager for isolated testing
        let manager = ProcessManager::new();

        // Spawn two sleep processes
        let mut child1 = async_process::Command::new("sleep")
            .arg("10")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn first sleep process");

        let mut child2 = async_process::Command::new("sleep")
            .arg("10")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn second sleep process");

        let pid1 = child1.id();
        let pid2 = child2.id();

        manager.register(pid1);
        manager.register(pid2);

        assert_eq!(manager.process_count(), 2);

        // Kill all processes
        manager.kill_all();

        // Give a moment for processes to terminate
        sleep(Duration::from_millis(100)).await;

        // Check if processes are actually terminated
        assert!(child1.try_status().is_ok_and(|status| status.is_some()));
        assert!(child2.try_status().is_ok_and(|status| status.is_some()));

        // Clean up
        manager.unregister(pid1);
        manager.unregister(pid2);

        assert_eq!(manager.process_count(), 0);
    }
}
