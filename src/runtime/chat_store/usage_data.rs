//! What `/cost` reads from the usage table, read-only: the folded rows and
//! the task keys. No DDL runs here; a column an older file lacks reads NULL.

use rusqlite::Connection;

use super::worker::usage_report;
use crate::core::ai::chat::dashboard::UsageData;

/// The aggregates over the timing columns of the report: the wall time of the
/// worker records, and how many have one. A file whose `turn_usage` has no
/// `wall_ms` yet reads NULL and 0, so the rest of `/cost` still shows.
pub(super) fn wall_sql(conn: &Connection) -> rusqlite::Result<&'static str> {
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('turn_usage') WHERE name = 'wall_ms')",
        [],
        |r| r.get(0),
    )?;
    Ok(if present {
        "SUM(CASE WHEN role = 'worker' THEN wall_ms END),
         COUNT(CASE WHEN role = 'worker' THEN wall_ms END)"
    } else {
        "NULL, 0"
    })
}

/// The rows of the report and the distinct task keys by day. A store without
/// the table has no usage; nothing is created.
pub(super) fn usage_data(
    conn: &Connection,
    conversation: Option<i64>,
    at: i64,
) -> rusqlite::Result<UsageData> {
    let rows = usage_report(conn, conversation, at)?;
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'turn_usage')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(UsageData::default());
    }
    // A task is a conversation (`c<id>`) or, for a worker without one, its
    // orchestrator run (`r<run>`, the part of `<run>.<worker>` before the dot).
    // A deleted chat has neither: its tokens count, no task does.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT
                MAX(0, CAST(julianday(date(?1, 'unixepoch', 'localtime'))
                      - julianday(date(ts, 'unixepoch', 'localtime')) AS INTEGER)) AS day,
                CASE WHEN conversation_id IS NOT NULL THEN 'c' || conversation_id
                     WHEN role = 'worker' AND instr(worker, '.') > 0
                     THEN 'r' || substr(worker, 1, instr(worker, '.') - 1) END AS key
         FROM turn_usage WHERE key IS NOT NULL ORDER BY day, key",
    )?;
    let tasks = stmt
        .query_map([at], |r| {
            let day: i64 = r.get(0)?;
            Ok((u32::try_from(day).unwrap_or(u32::MAX), r.get(1)?))
        })?
        .collect::<Result<_, _>>()?;
    Ok(UsageData { rows, tasks })
}

#[cfg(test)]
mod tests;
