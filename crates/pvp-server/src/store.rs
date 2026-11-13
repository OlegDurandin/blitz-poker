//! SQLite persistence: open rooms and tables (with the authoritative match
//! inside) and the head-to-head score of each pair of players.
//!
//! A room is written after every change, so a restart or deploy resumes
//! every match from its last accepted action.

use crate::player::PlayerId;
use crate::room::{Room, Score, Status};
use crate::table_room::{TableRoom, TableStatus};
use poker_core::MatchOutcome;
use rusqlite::{Connection, OptionalExtension, params};

pub struct Store {
    pub(crate) conn: Connection,
    pub(crate) rewards: crate::progress::Rewards,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS rooms (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    data TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS rooms_status ON rooms (status);
CREATE TABLE IF NOT EXISTS duels (
    a INTEGER NOT NULL,
    b INTEGER NOT NULL,
    a_wins INTEGER NOT NULL DEFAULT 0,
    b_wins INTEGER NOT NULL DEFAULT 0,
    draws INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (a, b)
);
CREATE TABLE IF NOT EXISTS tables (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    data TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tables_status ON tables (status);

";

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Waiting => "waiting",
        Status::Ready => "ready",
        Status::Playing => "playing",
        Status::Finished => "finished",
        Status::Closed => "closed",
    }
}

impl Store {
    pub fn set_rewards(&mut self, rewards: crate::progress::Rewards) {
        self.rewards = rewards;
    }
    /// `path` may be `:memory:` for tests.
    pub fn open(path: &str) -> rusqlite::Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 3 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(SCHEMA)?;
        if version < 3 {
            tx.execute_batch(crate::progress::SCHEMA)?;
            tx.execute_batch("INSERT OR IGNORE INTO progress_commits SELECT json_extract(data, '$.game.match_id') FROM rooms WHERE json_valid(data) AND json_extract(data, '$.game.m.result') IS NOT NULL AND json_extract(data, '$.game.match_id') IS NOT NULL;
             INSERT OR IGNORE INTO progress_commits SELECT json_extract(data, '$.game.match_id') FROM tables WHERE json_valid(data) AND json_extract(data, '$.game.t.result') IS NOT NULL AND json_extract(data, '$.game.match_id') IS NOT NULL;
             PRAGMA user_version=3;")?;
        }
        tx.commit()?;
        Ok(Store {
            conn,
            rewards: Default::default(),
        })
    }

    pub fn save_room(&self, room: &Room, now: u64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let data = serde_json::to_string(room).expect("room serializes");
        tx.execute(
            "INSERT INTO rooms (id, status, data, updated_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET status = ?2, data = ?3, updated_ms = ?4",
            params![room.id, status_name(room.status), data, now as i64],
        )?;
        crate::progress::room(&tx, room, now, &self.rewards)?;
        tx.commit()?;
        crate::progress::publish_events(&self.conn);
        Ok(())
    }

    /// Rooms that are not closed. Unreadable rows are skipped and logged.
    pub fn open_rooms(&self) -> rusqlite::Result<Vec<Room>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM rooms WHERE status != 'closed'")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut rooms = Vec::new();
        for row in rows {
            let (id, data) = row?;
            match serde_json::from_str::<Room>(&data) {
                Ok(room) => rooms.push(room),
                Err(e) => tracing::error!(room = %crate::ids::log_tag(&id), "unreadable room: {e}"),
            }
        }
        Ok(rooms)
    }

    /// A closed room by id, so a late visitor learns the invite is over.
    pub fn room(&self, id: &str) -> rusqlite::Result<Option<Room>> {
        let data: Option<String> = self
            .conn
            .query_row("SELECT data FROM rooms WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(data.and_then(|d| serde_json::from_str(&d).ok()))
    }

    /// Drops closed rooms and tables not touched for `older_than_ms`.
    pub fn prune_closed(&self, now: u64, older_than_ms: u64) -> rusqlite::Result<usize> {
        let before = now.saturating_sub(older_than_ms) as i64;
        let rooms = self.conn.execute(
            "DELETE FROM rooms WHERE status = 'closed' AND updated_ms < ?1",
            [before],
        )?;
        let tables = self.conn.execute(
            "DELETE FROM tables WHERE status = 'closed' AND updated_ms < ?1",
            [before],
        )?;
        Ok(rooms + tables)
    }

    pub fn save_table(&self, table: &TableRoom, now: u64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let data = serde_json::to_string(table).expect("table serializes");
        let status = match table.status {
            TableStatus::Lobby => "lobby",
            TableStatus::Playing => "playing",
            TableStatus::Finished => "finished",
            TableStatus::Closed => "closed",
        };
        tx.execute(
            "INSERT INTO tables (id, status, data, updated_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET status = ?2, data = ?3, updated_ms = ?4",
            params![table.id, status, data, now as i64],
        )?;
        crate::progress::table(&tx, table, now, &self.rewards)?;
        tx.commit()?;
        crate::progress::publish_events(&self.conn);
        Ok(())
    }

    /// Tables that are not closed. Unreadable rows are skipped and logged.
    pub fn open_tables(&self) -> rusqlite::Result<Vec<TableRoom>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM tables WHERE status != 'closed'")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut tables = Vec::new();
        for row in rows {
            let (id, data) = row?;
            match serde_json::from_str::<TableRoom>(&data) {
                Ok(t) => tables.push(t),
                Err(e) => {
                    tracing::error!(table = %crate::ids::log_tag(&id), "unreadable table: {e}")
                }
            }
        }
        Ok(tables)
    }

    /// A table by id, closed ones included.
    pub fn table(&self, id: &str) -> rusqlite::Result<Option<TableRoom>> {
        let data: Option<String> = self
            .conn
            .query_row("SELECT data FROM tables WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(data.and_then(|d| serde_json::from_str(&d).ok()))
    }

    pub fn record_duel(
        &self,
        host: PlayerId,
        guest: PlayerId,
        outcome_for_host: MatchOutcome,
    ) -> rusqlite::Result<()> {
        let (host, guest) = (host.raw(), guest.raw());
        // Pairs are stored once, smaller id first.
        let (a, b, a_won) = if host < guest {
            (host, guest, outcome_for_host == MatchOutcome::Win)
        } else {
            (guest, host, outcome_for_host == MatchOutcome::Loss)
        };
        let draw = outcome_for_host == MatchOutcome::Draw;
        self.conn.execute(
            "INSERT INTO duels (a, b, a_wins, b_wins, draws) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (a, b) DO UPDATE SET
               a_wins = a_wins + ?3, b_wins = b_wins + ?4, draws = draws + ?5",
            params![
                a as i64,
                b as i64,
                (a_won && !draw) as i64,
                (!a_won && !draw) as i64,
                draw as i64
            ],
        )?;
        Ok(())
    }

    /// Score between `host` and `guest`, from the host's side.
    pub fn head_to_head(&self, host: PlayerId, guest: PlayerId) -> rusqlite::Result<Score> {
        let (host, guest) = (host.raw(), guest.raw());
        let (a, b) = (host.min(guest), host.max(guest));
        let row: Option<(i64, i64, i64)> = self
            .conn
            .query_row(
                "SELECT a_wins, b_wins, draws FROM duels WHERE a = ?1 AND b = ?2",
                params![a as i64, b as i64],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (a_wins, b_wins, draws) = row.unwrap_or_default();
        let (host_wins, guest_wins) = if host == a {
            (a_wins, b_wins)
        } else {
            (b_wins, a_wins)
        };
        Ok(Score {
            host: host_wins as u32,
            guest: guest_wins as u32,
            draws: draws as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::Profile;

    fn pid(id: u64) -> PlayerId {
        PlayerId::vk(id).unwrap()
    }

    #[test]
    fn rooms_round_trip_and_scores_are_symmetric() {
        let s = Store::open(":memory:").unwrap();
        let p = Profile {
            id: pid(5),
            name: "A".into(),
            photo: None,
        };
        let room = Room::new("r1".into(), p, "q".into(), 10);
        s.save_room(&room, 10).unwrap();
        assert_eq!(s.open_rooms().unwrap(), vec![room.clone()]);
        assert_eq!(s.room("r1").unwrap(), Some(room));

        s.record_duel(pid(9), pid(5), MatchOutcome::Win).unwrap(); // 9 beat 5
        s.record_duel(pid(5), pid(9), MatchOutcome::Draw).unwrap();
        s.record_duel(pid(5), pid(9), MatchOutcome::Win).unwrap(); // 5 beat 9
        s.record_duel(pid(5), pid(9), MatchOutcome::Win).unwrap();
        assert_eq!(
            s.head_to_head(pid(5), pid(9)).unwrap(),
            Score {
                host: 2,
                guest: 1,
                draws: 1
            }
        );
        assert_eq!(
            s.head_to_head(pid(9), pid(5)).unwrap(),
            Score {
                host: 1,
                guest: 2,
                draws: 1
            }
        );
        assert_eq!(s.head_to_head(pid(1), pid(2)).unwrap(), Score::default());
    }

    #[test]
    fn tables_round_trip_and_closed_ones_are_pruned() {
        let s = Store::open(":memory:").unwrap();
        let p = Profile {
            id: pid(5),
            name: "A".into(),
            photo: None,
        };
        let mut t = TableRoom::new("t1".into(), p, 3, "q".into(), 10).unwrap();
        s.save_table(&t, 10).unwrap();
        assert_eq!(s.open_tables().unwrap(), vec![t.clone()]);
        t.leave(pid(5), 20).unwrap();
        s.save_table(&t, 20).unwrap();
        assert!(s.open_tables().unwrap().is_empty());
        assert_eq!(s.table("t1").unwrap(), Some(t));
        assert_eq!(s.prune_closed(1_000, 100).unwrap(), 1);
        assert_eq!(s.table("t1").unwrap(), None);
    }
}
