use crate::services::logging_service::LoggingService;
use crate::services::redis_service::RedisService;
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedLogEntry {
    pub source: String,
    pub level: String,
    pub category: String,
    pub message: String,
    pub node_label: Option<String>,
    pub created_at: String,
    pub sort_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedLogPayload {
    pub logs: Vec<UnifiedLogEntry>,
    pub categories: Vec<String>,
}

pub struct UnifiedLogService;

impl UnifiedLogService {
    pub async fn get_logs(
        pool: &PgPool,
        redis: &RedisService,
        source: &str,
        category: Option<&str>,
        node_filter: Option<(i64, String)>,
        limit: usize,
        offset: usize,
    ) -> Result<UnifiedLogPayload> {
        let cache_key = format!(
            "unified-logs:filters:v1:{}:{}:{}:{}:{}",
            source,
            category.unwrap_or(""),
            node_filter.as_ref().map(|(id, _)| *id).unwrap_or(0),
            limit,
            offset
        );

        if let Some(cached) = redis.get(&cache_key).await? {
            if let Ok(payload) = serde_json::from_str::<UnifiedLogPayload>(&cached) {
                return Ok(payload);
            }
        }

        let mut logs = Vec::new();
        let normalized_source = source.trim().to_ascii_lowercase();

        if normalized_source.is_empty()
            || normalized_source == "all"
            || normalized_source == "system"
        {
            let system_logs = LoggingService::get_logs(
                pool,
                limit as i64,
                offset as i64,
                category.map(str::to_string),
            )
            .await?;
            logs.extend(system_logs.into_iter().map(|log| UnifiedLogEntry {
                source: "system".to_string(),
                level: log.action.clone(),
                category: log.action,
                message: log.details.unwrap_or_default(),
                node_label: None,
                created_at: log.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                sort_ts: log.created_at.timestamp(),
            }));
        }

        if normalized_source.is_empty() || normalized_source == "all" || normalized_source == "bot"
        {
            logs.extend(read_flat_log_file("bot", "bot.log", limit));
        }

        if (normalized_source.is_empty()
            || normalized_source == "all"
            || normalized_source == "node")
            && node_filter.is_some()
        {
            let (node_id, node_name) = node_filter.unwrap();
            if let Some(serialized) = redis.get(&format!("node_logs:{}", node_id)).await? {
                let parsed: HashMap<String, String> =
                    serde_json::from_str(&serialized).unwrap_or_default();
                for (service_name, content) in parsed {
                    for line in content.lines().rev().take(limit) {
                        if line.trim().is_empty() {
                            continue;
                        }
                        logs.push(parse_external_log_line(
                            "node",
                            line,
                            Some(format!("{} · {}", node_name, service_name)),
                        ));
                    }
                }
            }
        }

        logs.sort_by(|left, right| right.sort_ts.cmp(&left.sort_ts));
        if logs.len() > limit {
            logs.truncate(limit);
        }

        let categories = LoggingService::get_categories(pool)
            .await
            .unwrap_or_default();
        let payload = UnifiedLogPayload { logs, categories };
        let serialized = serde_json::to_string(&payload)?;
        let _ = redis.set(&cache_key, &serialized, 45).await;
        Ok(payload)
    }
}

fn read_flat_log_file(source: &str, path: &str, limit: usize) -> Vec<UnifiedLogEntry> {
    if !Path::new(path).exists() {
        return Vec::new();
    }

    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    content
        .lines()
        .rev()
        .take(limit)
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse_external_log_line(source, line, None))
        .collect()
}

fn parse_external_log_line(
    source: &str,
    line: &str,
    node_label: Option<String>,
) -> UnifiedLogEntry {
    let timestamp = parse_line_timestamp(line).unwrap_or_else(Utc::now);
    let level = if line.contains("ERROR") || line.contains("Error") {
        "Error"
    } else if line.contains("WARN") || line.contains("Warning") {
        "Warning"
    } else if line.contains("INFO") {
        "Info"
    } else {
        "Log"
    };

    UnifiedLogEntry {
        source: source.to_string(),
        level: level.to_string(),
        category: source.to_ascii_uppercase(),
        message: line.to_string(),
        node_label,
        created_at: timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
        sort_ts: timestamp.timestamp(),
    }
}

fn parse_line_timestamp(line: &str) -> Option<DateTime<Utc>> {
    for token in line.split_whitespace().take(2) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(token) {
            return Some(dt.with_timezone(&Utc));
        }
    }

    let prefix = line.get(0..19)?;
    let naive = NaiveDateTime::parse_from_str(prefix, "%Y-%m-%d %H:%M:%S").ok()?;
    Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

#[cfg(test)]
mod tests {
    use super::{parse_external_log_line, parse_line_timestamp};

    #[test]
    fn parses_plain_timestamp_prefix() {
        let parsed = parse_line_timestamp("2026-03-15 12:34:56 INFO boot ok");
        assert!(parsed.is_some());
    }

    #[test]
    fn infers_error_level_for_external_line() {
        let entry = parse_external_log_line("bot", "2026-03-15 12:34:56 ERROR failed", None);
        assert_eq!(entry.level, "Error");
        assert_eq!(entry.source, "bot");
    }
}
