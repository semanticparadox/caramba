use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn line_number(content: &str, byte_idx: usize) -> usize {
    content[..byte_idx].bytes().filter(|b| *b == b'\n').count() + 1
}

fn parse_sql_literal_from_call(content: &str, call_idx: usize) -> Option<(usize, String)> {
    let open_paren_rel = content[call_idx..].find('(')?;
    let mut i = call_idx + open_paren_rel + 1;
    let bytes = content.as_bytes();

    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }

    // Raw string: r"..." or r#"..."# etc.
    if bytes[i] == b'r' {
        let mut j = i + 1;
        let mut hashes = 0usize;
        while j < bytes.len() && bytes[j] == b'#' {
            hashes += 1;
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'"' {
            return None;
        }
        let start = j + 1;
        let mut end_marker = String::from("\"");
        end_marker.push_str(&"#".repeat(hashes));
        let end_rel = content[start..].find(&end_marker)?;
        let end = start + end_rel;
        return Some((i, content[start..end].to_string()));
    }

    // Standard string: "..."
    if bytes[i] == b'"' {
        let start = i + 1;
        let mut j = start;
        let mut escaped = false;
        while j < bytes.len() {
            let b = bytes[j];
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                return Some((i, content[start..j].to_string()));
            }
            j += 1;
        }
    }

    None
}

fn extract_sql_literals(content: &str) -> Vec<(usize, String)> {
    let mut result = Vec::new();
    let mut pos = 0usize;
    while let Some(rel) = content[pos..].find("sqlx::query") {
        let idx = pos + rel;
        if let Some(parsed) = parse_sql_literal_from_call(content, idx) {
            result.push(parsed);
        }
        pos = idx + "sqlx::query".len();
    }
    result
}

#[test]
fn sqlx_queries_must_not_use_sqlite_placeholders() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (byte_idx, sql) in extract_sql_literals(&content) {
            if sql.contains('?') {
                let line = line_number(&content, byte_idx);
                violations.push(format!(
                    "{}:{} contains '?' placeholder in sqlx query literal",
                    file.display(),
                    line
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found SQLite placeholders in SQL literals:\n{}",
        violations.join("\n")
    );
}

#[test]
fn sqlx_queries_must_not_use_sqlite_specific_syntax() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (byte_idx, sql) in extract_sql_literals(&content) {
            let lower = sql.to_lowercase();
            let has_bad = lower.contains("insert or ignore")
                || lower.contains("strftime(")
                || lower.contains("datetime(");
            if has_bad {
                let line = line_number(&content, byte_idx);
                violations.push(format!(
                    "{}:{} contains SQLite-only SQL syntax",
                    file.display(),
                    line
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found SQLite-specific SQL in query literals:\n{}",
        violations.join("\n")
    );
}
