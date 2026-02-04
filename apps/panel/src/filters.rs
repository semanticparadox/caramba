use askama::Result;
pub use askama::filters::*; // Re-export built-in filters so Askama can find them


pub fn format_bytes(s: &i64) -> Result<String> {
    let bytes = *s as u64;
    let res = if bytes < 1024 { format!("{} B", bytes) }
    else if bytes < 1024 * 1024 { format!("{:.1} KB", bytes as f64 / 1024.0) }
    else if bytes < 1024 * 1024 * 1024 { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
    else { format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0)) };
    Ok(res)
}

// Overload or handling for u64 if needed? Askama usually passes reference.
// The SQLx queries return i64 for count/sum usually.
// admin.rs uses `bytes: u64` in its local helper.
// I'll add another one or generic? 
// Askama filters are functions.
// I can define `format_bytes_i64` or just expect i64 since DB uses i64.
