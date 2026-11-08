//! 任务数据存储 —— 与旧版数据格式完全兼容。
//! 字段：id / title / completed / created_at / due_date（可选），JSON 数组，UTF-8，两空格缩进。

use chrono::{Local, NaiveDate};
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
    /// 设定的日期（YYYY-MM-DD）。未设定时为 None：文件里不写该字段，卡片上也不显示。
    #[serde(rename = "due_date", default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
}

/// 今天的日期（YYYY-MM-DD）。未设定日期的任务用它参与排序，随系统日期动态变化。
pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// "2026-11-08" → 卡片上显示的 "26-11-08"（只显示日期，不显示时间）。
pub fn short_date(iso: &str) -> String {
    NaiveDate::parse_from_str(iso, "%Y-%m-%d")
        .map(|d| d.format("%y-%m-%d").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

impl Task {
    /// 卡片右侧显示的日期；未设定日期则不显示。
    pub fn display_date(&self) -> Option<String> {
        self.due_date.as_deref().map(short_date)
    }
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

    pub fn add(&mut self, title: &str, due_date: Option<&str>) -> Task {
        let now = chrono::Local::now();
        let task = Task {
            id: now.format("%Y%m%d%H%M%S%.6f").to_string(),
            title: title.to_string(),
            completed: false,
            created_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            due_date: due_date.map(|d| d.to_string()),
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

    /// 修改已有任务的标题与日期；未找到返回 false。
    pub fn update(&mut self, id: &str, title: &str, due_date: Option<&str>) -> bool {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.title = title.to_string();
            t.due_date = due_date.map(|d| d.to_string());
            self.save();
            return true;
        }
        false
    }

    pub fn delete(&mut self, id: &str) {
        self.tasks.retain(|t| t.id != id);
        self.save();
    }

    pub fn count(&self) -> usize {
        self.tasks.len()
    }

    /// 列表视图：日期早的排在上面，晚的在下面；
    /// 同一日期的任务保持添加先后顺序（稳定排序）；
    /// 未设定日期的任务按「今天」参与排序。
    pub fn all(&self) -> Vec<Task> {
        let today = today();
        let mut tasks = self.tasks.clone();
        tasks.sort_by_cached_key(|t| t.due_date.clone().unwrap_or_else(|| today.clone()));
        tasks
    }
}
