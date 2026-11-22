//! What `/cost` reads from the usage table, read-only: the folded rows and
//! the task keys and the hours. No DDL runs here; a column an older file lacks reads NULL.

use rusqlite::Connection;

use super::worker::usage_report;
use crate::core::ai::chat::dashboard::{AgentDay, HourCell, Span, UsageData};

/// The local days before `?1` (a unix time) of the row's `ts`. Rows and tasks
/// share it, so a record falls on the same day in both.
pub(super) const DAY_SQL: &str = "MAX(0, CAST(julianday(date(?1, 'unixepoch', 'localtime'))
                      - julianday(date(ts, 'unixepoch', 'localtime')) AS INTEGER))";

/// The aggregates over the timing columns of the report: the wall time of the
/// chat and worker records (compaction and summary runs are not timed), and
/// how many have one. A file whose `turn_usage` has no
/// `wall_ms` yet reads NULL and 0, so the rest of `/cost` still shows.
pub(super) fn wall_sql(conn: &Connection) -> rusqlite::Result<&'static str> {
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('turn_usage') WHERE name = 'wall_ms')",
        [],
        |r| r.get(0),
    )?;
    Ok(if present {
        "SUM(CASE WHEN role IN ('chat', 'worker') THEN wall_ms END),
         COUNT(CASE WHEN role IN ('chat', 'worker') THEN wall_ms END)"
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
    // A task is a conversation (`c<id>`) or an orchestrator run (`r<run>`, the
    // part of `<run>.<worker>` before the dot); a run launched from a chat is
    // a task of its own, so a worker row yields its run whether or not it is
    // linked. A deleted chat has neither: its tokens count, no task does.
    let mut stmt = conn.prepare(&format!(
        "SELECT {DAY_SQL} AS day, 'c' || conversation_id AS key
           FROM turn_usage WHERE conversation_id IS NOT NULL
         UNION
         SELECT {DAY_SQL}, 'r' || substr(worker, 1, instr(worker, '.') - 1)
           FROM turn_usage WHERE role = 'worker' AND instr(worker, '.') > 1
         ORDER BY day, key",
    ))?;
    let tasks = stmt
        .query_map([at], |r| {
            let day: i64 = r.get(0)?;
            Ok((u32::try_from(day).unwrap_or(u32::MAX), r.get(1)?))
        })?
        .collect::<Result<_, _>>()?;
    // The weekday is Monday first (`(w + 6) % 7`), the hour the local one of
    // the end time `ts`, and a record with no known token still makes its cell.
    let mut stmt = conn.prepare(&format!(
        "SELECT {DAY_SQL} AS day,
                (CAST(strftime('%w', ts, 'unixepoch', 'localtime') AS INTEGER) + 6) % 7,
                CAST(strftime('%H', ts, 'unixepoch', 'localtime') AS INTEGER),
                SUM(COALESCE(input, 0) + COALESCE(output, 0))
           FROM turn_usage GROUP BY 1, 2, 3 ORDER BY 1, 2, 3",
    ))?;
    let hours = stmt
        .query_map([at], |r| {
            let day: i64 = r.get(0)?;
            let tokens: i64 = r.get(3)?;
            Ok(HourCell {
                days_ago: u32::try_from(day).unwrap_or(u32::MAX),
                weekday: r.get(1)?,
                hour: r.get(2)?,
                tokens: u64::try_from(tokens).unwrap_or(0),
            })
        })?
        .collect::<Result<_, _>>()?;
    let wall = wall_sql(conn)?;
    // The worker records of each engine, with their wall time.
    let mut stmt = conn.prepare(&format!(
        "SELECT {DAY_SQL} AS day, engine, COUNT(*), SUM(input), SUM(output),
                MAX(partial OR input IS NULL OR output IS NULL), {wall}
           FROM turn_usage WHERE role = 'worker' GROUP BY 1, 2 ORDER BY 1, 2",
    ))?;
    let count = |v: Option<i64>| v.and_then(|n| u64::try_from(n).ok());
    let agents = stmt
        .query_map([at], |r| {
            Ok(AgentDay {
                days_ago: u32::try_from(r.get::<_, i64>(0)?).unwrap_or(u32::MAX),
                engine: r.get(1)?,
                workers: count(Some(r.get(2)?)).unwrap_or(0),
                input: count(r.get(3)?),
                output: count(r.get(4)?),
                partial: r.get(5)?,
                wall_ms: count(r.get(6)?),
                timed: count(Some(r.get(7)?)).unwrap_or(0),
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(UsageData {
        rows,
        tasks,
        hours,
        agents,
        spans: spans(conn, at)?,
    })
}

/// The worker runs with a start. A file without `started_at` has none, so the
/// rest of `/cost` still shows.
fn spans(conn: &Connection, at: i64) -> rusqlite::Result<Vec<Span>> {
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('turn_usage') WHERE name = 'started_at')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT {DAY_SQL}, started_at, ts FROM turn_usage
          WHERE role = 'worker' AND started_at IS NOT NULL ORDER BY ts, id",
    ))?;
    let spans = stmt.query_map([at], |r| {
        Ok(Span {
            days_ago: u32::try_from(r.get::<_, i64>(0)?).unwrap_or(u32::MAX),
            start: r.get(1)?,
            end: r.get(2)?,
        })
    })?;
    spans.collect()
}

#[cfg(test)]
mod tests;
