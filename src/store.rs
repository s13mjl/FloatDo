//! 任务数据存储 —— 与旧版数据格式完全兼容。
//! 字段：id / title / completed / created_at，JSON 数组，UTF-8，两空格缩进。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub completed: bool,
    #[serde(rename = "created_at")]
    pub created_at: String,
}

pub struct TaskStore {
    file_path: PathBuf,
    tasks: Vec<Task>,
}

impl TaskStore {
    /// 任务文件放在 exe 同目录下。
    pub fn new() -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let file_path = if dir.as_os_str().is_empty() {
            PathBuf::from("tasks.txt")
        } else {
            dir.join("tasks.txt")
        };
        let mut store = TaskStore {
            file_path,
            tasks: Vec::new(),
        };
        store.load();
        store
    }

    pub fn load(&mut self) {
        let text = match fs::read_to_string(&self.file_path) {
            Ok(t) => t,
            Err(_) => {
                self.tasks = Vec::new();
                return;
            }
        };
        self.tasks = serde_json::from_str(&text).unwrap_or_default();
    }

    pub fn save(&self) -> bool {
        match serde_json::to_string_pretty(&self.tasks) {
            Ok(text) => fs::write(&self.file_path, text).is_ok(),
            Err(_) => false,
        }
    }

    pub fn add(&mut self, title: &str) -> Task {
        let now = chrono::Local::now();
        let task = Task {
            id: now.format("%Y%m%d%H%M%S%.6f").to_string(),
            title: title.to_string(),
            completed: false,
            created_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        self.tasks.push(task.clone());
        self.save();
        task
    }

    pub fn toggle(&mut self, id: &str) -> bool {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.completed = !t.completed;
            self.save();
            return true;
        }
        false
    }

    pub fn delete(&mut self, id: &str) {
        self.tasks.retain(|t| t.id != id);
        self.save();
    }

    pub fn all(&self) -> &[Task] {
        &self.tasks
    }
}
