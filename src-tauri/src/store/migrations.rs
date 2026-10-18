use rusqlite::Connection;

use crate::error::Result;

/// Ordered list of migrations. The index + 1 is the schema version stored in `user_version`.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_init.sql"),
    include_str!("migrations/0002_unsubscribe.sql"),
    include_str!("migrations/0003_aliases.sql"),
    include_str!("migrations/0004_settings.sql"),
];

pub fn apply(conn: &mut Connection) -> Result<()> {
    let current: usize = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
        tracing::info!(version = i + 1, "applied database migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn).unwrap();
        apply(&mut conn).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
        // FTS5 must be compiled in.
        conn.execute(
            "INSERT INTO messages_fts(rowid, subject, sender, recipients, preview, body) VALUES (1, 'hello', '', '', '', '')",
            [],
        )
        .unwrap();
    }
}
