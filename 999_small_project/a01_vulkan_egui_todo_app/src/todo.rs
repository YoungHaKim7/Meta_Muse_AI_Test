use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u64,
    pub text: String,
    pub done: bool,
    pub created_at: DateTime<Local>,
}

impl TodoItem {
    pub fn new(id: u64, text: String) -> Self {
        Self {
            id,
            text,
            done: false,
            created_at: Local::now(),
        }
    }
}

#[derive(Debug, Default)]
pub struct TodoList {
    pub items: Vec<TodoItem>,
    next_id: u64,
}

impl TodoList {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }
    pub fn add(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        self.items.push(TodoItem::new(self.next_id, text));
        self.next_id += 1;
    }
    pub fn toggle(&mut self, idx: usize) {
        if let Some(item) = self.items.get_mut(idx) {
            item.done = !item.done;
        }
    }
    pub fn remove(&mut self, idx: usize) {
        if idx < self.items.len() {
            self.items.remove(idx);
        }
    }
    pub fn stats(&self) -> (usize, usize) {
        (
            self.items.len(),
            self.items.iter().filter(|i| i.done).count(),
        )
    }
    pub fn load_from_file(path: &std::path::Path) -> Self {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(items) = serde_json::from_str::<Vec<TodoItem>>(&data) {
                let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                return Self { items, next_id };
            }
        }
        Self::new()
    }
    pub fn save_to_file(&self, path: &std::path::Path) {
        if let Ok(json) = serde_json::to_string_pretty(&self.items) {
            let _ = std::fs::write(path, json);
        }
    }
}
