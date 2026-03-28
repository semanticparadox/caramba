use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Serialize)]
pub struct TaskHealth {
    pub name: String,
    pub last_run: Option<DateTime<Utc>>,
    pub last_success: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub run_count: u64,
    pub error_count: u64,
}

#[derive(Clone)]
pub struct TaskHealthRegistry {
    tasks: Arc<RwLock<HashMap<String, TaskHealth>>>,
}

impl TaskHealthRegistry {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Записываем успешное выполнение задачи — обновляем счётчики и временные метки.
    pub async fn record_success(&self, name: &str) {
        let mut tasks = self.tasks.write().await;
        let entry = tasks
            .entry(name.to_string())
            .or_insert_with(|| TaskHealth {
                name: name.to_string(),
                last_run: None,
                last_success: None,
                last_error: None,
                run_count: 0,
                error_count: 0,
            });
        entry.last_run = Some(Utc::now());
        entry.last_success = Some(Utc::now());
        entry.run_count += 1;
    }

    /// Записываем ошибку задачи — сохраняем сообщение и обновляем счётчики.
    pub async fn record_error(&self, name: &str, error: &str) {
        let mut tasks = self.tasks.write().await;
        let entry = tasks
            .entry(name.to_string())
            .or_insert_with(|| TaskHealth {
                name: name.to_string(),
                last_run: None,
                last_success: None,
                last_error: None,
                run_count: 0,
                error_count: 0,
            });
        entry.last_run = Some(Utc::now());
        entry.last_error = Some(error.to_string());
        entry.run_count += 1;
        entry.error_count += 1;
    }

    /// Возвращает срез текущего состояния всех зарегистрированных задач.
    pub async fn get_all(&self) -> Vec<TaskHealth> {
        let tasks = self.tasks.read().await;
        tasks.values().cloned().collect()
    }
}
