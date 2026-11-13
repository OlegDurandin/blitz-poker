//! The hub: all rooms, all connections, one lock.
//!
//! Every command runs under a single mutex: it mutates one room, persists it
//! and sends each member their own snapshot. That makes joins, ready and
//! actions atomic without any distributed locking (one server process is
//! enough for private duels). Nothing here awaits while holding the lock.

use crate::auth::Sessions;
use crate::config::Config;
use crate::ids;
use crate::player::PlayerId;
use crate::protocol::{
    ClientMsg, Envelope, MAX_ID_LEN, Outbound, PROTOCOL_VERSION, PublicUser, ServerMsg,
};
use crate::room::{Dealer, Event, OsDealer, Profile, Room, RoomError, Status};
use crate::store::Store;
use crate::table_room::{TableEvent, TableRoom, TableStatus};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_millis() as u64
}

/// Rooms with a guest seated that one user may host at once.
const MAX_HOSTED_ROOMS: usize = 3;
/// Closed rooms stay in memory this long (late visitors see "closed").
const CLOSED_IN_MEMORY_MS: u64 = 3_600_000;
/// Closed rooms stay in the database this long.
const CLOSED_IN_DB_MS: u64 = 24 * 3_600_000;
/// Wrong room codes per window, counted per user *and* per network (IPv4
/// address or IPv6 /64), so new guest identities do not reset the count.
const CODE_MISSES_MAX: u32 = 20;
const CODE_MISSES_WINDOW_MS: u64 = 10 * 60_000;
/// Guest sign-ins per network per window, and server-wide per minute as a
/// backstop on anonymous sessions.
const GUEST_AUTH_PER_CLIENT: u32 = 10;
const GUEST_AUTH_WINDOW_MS: u64 = 10 * 60_000;
const GUEST_AUTH_GLOBAL_PER_MINUTE: u32 = 300;

/// Fixed-window counters keyed by user or network.
#[derive(Default)]
struct Windows(HashMap<String, (u64, u32)>);

impl Windows {
    fn count(&self, key: &str, now: u64, window: u64) -> u32 {
        match self.0.get(key) {
            Some(&(start, n)) if now.saturating_sub(start) <= window => n,
            _ => 0,
        }
    }

    fn add(&mut self, key: &str, now: u64, window: u64) -> u32 {
        let e = self.0.entry(key.to_string()).or_insert((now, 0));
        if now.saturating_sub(e.0) > window {
            *e = (now, 0);
        }
        e.1 += 1;
        e.1
    }

    fn prune(&mut self, now: u64, window: u64) {
        self.0
            .retain(|_, (start, _)| now.saturating_sub(*start) <= window);
    }
}

/// What the socket task should do.
#[derive(Debug)]
pub enum Outgoing {
    Text(String),
    /// Close the socket (e.g. the same account connected elsewhere).
    Close,
}

struct Conn {
    profile: Profile,
    /// Network key for rate limits (IPv4 address or IPv6 /64).
    client: String,
    tx: mpsc::UnboundedSender<Outgoing>,
}

#[derive(Default, Serialize)]
pub struct Metrics {
    pub auth_failures: AtomicU64,
    pub connections_total: AtomicU64,
    pub reconnects: AtomicU64,
    pub rooms_created: AtomicU64,
    pub matches_started: AtomicU64,
    pub matches_finished: AtomicU64,
    pub forfeits: AtomicU64,
    pub actions_accepted: AtomicU64,
    pub actions_rejected: AtomicU64,
    pub persistence_errors: AtomicU64,
    /// Sum and count of how long hosts waited for a friend to join.
    pub wait_for_friend_ms_total: AtomicU64,
    pub wait_for_friend_count: AtomicU64,
    pub tables_created: AtomicU64,
    pub tables_started: AtomicU64,
    pub tables_finished: AtomicU64,
}

struct State {
    rooms: HashMap<String, Room>,
    /// Tables of 2–4 with people and bots (`table_room`).
    tables: HashMap<String, TableRoom>,
    /// (user, request_id) → table id, for idempotent `create_table`.
    table_requests: HashMap<(PlayerId, String), String>,
    conns: HashMap<u64, Conn>,
    /// The one live connection per VK user.
    by_user: HashMap<PlayerId, u64>,
    /// (user, request_id) → room id, for idempotent `create_room`.
    create_requests: HashMap<(PlayerId, String), String>,
    /// Wrong room codes per user (`u:<id>`) and per network (`n:<net>`).
    code_misses: Windows,
    /// Guest sign-ins per network, and the server-wide minute (`*`).
    guest_auths: Windows,
    sessions: Sessions,
    store: Store,
    local_reports: Windows,
    daily_analytics: Windows,
    pending_rooms: std::collections::HashSet<String>,
    pending_tables: std::collections::HashSet<String>,
    next_conn: u64,
}

pub struct Hub {
    pub config: Config,
    state: Mutex<State>,
    seq: AtomicU64,
    pub metrics: Metrics,
}

impl Hub {
    /// Loads open rooms from the store; every match resumes with a grace window.
    pub fn new(config: Config, store: Store) -> Hub {
        let now = now_ms();
        let mut rooms = HashMap::new();
        match store.open_rooms() {
            Ok(list) => {
                for mut room in list {
                    room.restore_after_restart(&config.timings, now);
                    rooms.insert(room.id.clone(), room);
                }
            }
            Err(e) => tracing::error!("could not load rooms: {e}"),
        }
        if !rooms.is_empty() {
            tracing::info!(rooms = rooms.len(), "restored open rooms");
        }
        let create_requests = rooms
            .values()
            .map(|r| ((r.host.profile.id, r.create_request.clone()), r.id.clone()))
            .collect();
        let mut tables = HashMap::new();
        match store.open_tables() {
            Ok(list) => {
                for mut t in list {
                    t.restore_after_restart(&config.timings, now);
                    tables.insert(t.id.clone(), t);
                }
            }
            Err(e) => tracing::error!("could not load tables: {e}"),
        }
        if !tables.is_empty() {
            tracing::info!(tables = tables.len(), "restored open tables");
        }
        let table_requests = tables
            .values()
            .map(|t: &TableRoom| ((t.host, t.create_request.clone()), t.id.clone()))
            .collect();
        Hub {
            config,
            state: Mutex::new(State {
                rooms,
                tables,
                table_requests,
                conns: HashMap::new(),
                by_user: HashMap::new(),
                create_requests,
                code_misses: Windows::default(),
                guest_auths: Windows::default(),
                sessions: Sessions::default(),
                store,
                local_reports: Windows::default(),
                daily_analytics: Windows::default(),
                pending_rooms: Default::default(),
                pending_tables: Default::default(),
                next_conn: 1,
            }),
            seq: AtomicU64::new(0),
            metrics: Metrics::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock would abort the process anyway
        // (panic = "abort" in release); in tests, recover the data.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn encode(&self, msg: &ServerMsg) -> String {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        serde_json::to_string(&Outbound {
            v: PROTOCOL_VERSION,
            server_seq: seq,
            msg,
        })
        .expect("server messages serialize")
    }

    fn send(&self, conn: &Conn, msg: &ServerMsg) {
        let _ = conn.tx.send(Outgoing::Text(self.encode(msg)));
    }

    // ---------- sessions ----------

    pub fn issue_session(&self, profile: Profile) -> (String, String) {
        self.lock().sessions.issue(profile, now_ms())
    }

    /// Counts a guest sign-in from `client` (a network key); false when that
    /// network, or the server as a whole, is over its limit.
    pub fn guest_auth_allowed(&self, client: &str) -> bool {
        let now = now_ms();
        let mut st = self.lock();
        let global = st.guest_auths.add("*", now, 60_000);
        let mine = st
            .guest_auths
            .add(&format!("n:{client}"), now, GUEST_AUTH_WINDOW_MS);
        global <= GUEST_AUTH_GLOBAL_PER_MINUTE && mine <= GUEST_AUTH_PER_CLIENT
    }

    pub fn ticket_for(&self, session: &str) -> Option<String> {
        self.lock().sessions.ticket_for(session, now_ms())
    }

    pub fn redeem_ticket(&self, ticket: &str) -> Option<Profile> {
        self.lock().sessions.redeem(ticket, now_ms())
    }

    pub fn daily_challenges(&self, session: &str) -> Result<serde_json::Value, u16> {
        let st = self.lock();
        let now = now_ms();
        let who = st
            .sessions
            .profile(session, now)
            .filter(|p| !p.id.is_guest())
            .ok_or(401u16)?;
        let tx = st.store.conn.unchecked_transaction().map_err(|_| 503u16)?;
        let id = crate::progress::identity(&tx, &who, now)
            .map_err(|_| 503u16)?
            .ok_or(401u16)?;
        let value = crate::daily::get(&tx, &id, now, &st.store.rewards).map_err(|_| 503u16)?;
        tx.commit().map_err(|_| 503u16)?;
        Ok(value)
    }

    pub fn daily_analytics(
        &self,
        session: &str,
        event: &str,
        event_id: &str,
    ) -> Result<serde_json::Value, u16> {
        if !["daily_challenges_viewed", "daily_challenges_play_clicked"].contains(&event)
            || !(8..=64).contains(&event_id.len())
            || !event_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(400);
        }
        let mut st = self.lock();
        let now = now_ms();
        let who = st
            .sessions
            .profile(session, now)
            .filter(|p| !p.id.is_guest())
            .ok_or(401u16)?;
        if st.daily_analytics.add(&who.id.to_string(), now, 60_000) > 60 {
            return Err(429);
        }
        let tx = st.store.conn.unchecked_transaction().map_err(|_| 503u16)?;
        let id = crate::progress::identity(&tx, &who, now)
            .map_err(|_| 503u16)?
            .ok_or(401u16)?;
        crate::progress::enqueue_event(
            &tx,
            &format!("clientdaily:{id}:{event_id}"),
            event,
            serde_json::json!({"player_id":id,"received_ms":now}),
        )
        .map_err(|_| 503u16)?;
        tx.commit().map_err(|_| 503u16)?;
        crate::progress::publish_events(&st.store.conn);
        Ok(serde_json::json!({"ok":true}))
    }

    pub fn personal(
        &self,
        session: &str,
        query: crate::progress::HistoryQuery,
        detail: Option<&str>,
        local: Option<&crate::progress::LocalReport>,
        profile: bool,
    ) -> Result<serde_json::Value, u16> {
        let mut st = self.lock();
        let who = st
            .sessions
            .profile(session, now_ms())
            .filter(|p| !p.id.is_guest())
            .ok_or(401u16)?;
        if !query.valid() || local.is_some_and(|r| !r.valid()) {
            return Err(400);
        }
        if local.is_some() && st.local_reports.add(&who.id.to_string(), now_ms(), 60_000) > 60 {
            return Err(429);
        }
        let tx = st.store.conn.unchecked_transaction().map_err(|_| 503u16)?;
        let id = crate::progress::identity(&tx, &who, now_ms())
            .map_err(|_| 503u16)?
            .ok_or(401u16)?;
        let value = if let Some(report) = local {
            crate::progress::local(&tx, &who, report, now_ms(), &st.store.rewards)
        } else if let Some(event) = detail {
            Ok(crate::progress::detail(&tx, &id, event)
                .map_err(|_| 503u16)?
                .ok_or(404u16)?)
        } else if profile {
            crate::progress::profile(
                &tx,
                &who,
                query.source.as_deref().unwrap_or("all"),
                now_ms(),
            )
        } else {
            crate::progress::matches(&tx, &id, &query)
        }
        .map_err(|_| 503u16)?;
        tx.commit().map_err(|_| 503u16)?;
        crate::progress::publish_events(&st.store.conn);
        Ok(value)
    }

    // ---------- connections ----------

    /// A verified user opened a socket. An older socket of the same user is
    /// told it was replaced and closed: one account, one live connection.
    /// `client`: the player's network (`server::client_key`), for rate limits.
    pub fn connect(
        &self,
        profile: Profile,
        client: String,
    ) -> (u64, mpsc::UnboundedReceiver<Outgoing>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let now = now_ms();
        let mut st = self.lock();
        let id = st.next_conn;
        st.next_conn += 1;
        let player = profile.id;
        if let Some(old) = st
            .by_user
            .insert(player, id)
            .and_then(|old| st.conns.remove(&old))
        {
            self.send(
                &old,
                &ServerMsg::ServerError {
                    code: "session_replaced",
                    message: "Игра открыта на другом устройстве",
                    room_id: None,
                },
            );
            let _ = old.tx.send(Outgoing::Close);
        }
        let conn = Conn {
            profile: profile.clone(),
            client,
            tx,
        };
        self.send(
            &conn,
            &ServerMsg::Welcome {
                user: PublicUser {
                    name: profile.name.clone(),
                    photo: profile.photo.clone(),
                },
                server_time_ms: now,
            },
        );
        st.conns.insert(id, conn);
        self.metrics
            .connections_total
            .fetch_add(1, Ordering::Relaxed);

        // Back in every open room this user belongs to.
        let ids: Vec<String> = st
            .rooms
            .values()
            .filter(|r| r.is_open() && r.slot_of(player).is_some())
            .map(|r| r.id.clone())
            .collect();
        for room_id in ids {
            let room = st.rooms.get_mut(&room_id).expect("room");
            let was_away = room
                .members()
                .any(|m| m.profile.id == player && !m.connected);
            room.set_connected(player, true, &self.config.timings, now);
            if was_away {
                self.metrics.reconnects.fetch_add(1, Ordering::Relaxed);
            }
            self.commit(&mut st, &room_id, now);
        }
        for table_id in Self::tables_of(&st, player) {
            let t = st.tables.get_mut(&table_id).expect("table");
            t.set_connected(player, true, &self.config.timings, now);
            self.commit_table(&mut st, &table_id, now);
        }
        (id, rx)
    }

    /// Open tables where `player` still has a seat.
    fn tables_of(st: &State, player: PlayerId) -> Vec<String> {
        st.tables
            .values()
            .filter(|t| t.is_open() && t.members().any(|m| m.profile.id == player))
            .map(|t| t.id.clone())
            .collect()
    }

    pub fn disconnect(&self, conn_id: u64) {
        let now = now_ms();
        let mut st = self.lock();
        let Some(conn) = st.conns.remove(&conn_id) else {
            return;
        };
        let player = conn.profile.id;
        if st.by_user.get(&player) != Some(&conn_id) {
            return; // replaced by a newer connection, which keeps the seat
        }
        st.by_user.remove(&player);
        let ids: Vec<String> = st
            .rooms
            .values()
            .filter(|r| r.is_open() && r.slot_of(player).is_some())
            .map(|r| r.id.clone())
            .collect();
        for room_id in ids {
            st.rooms.get_mut(&room_id).expect("room").set_connected(
                player,
                false,
                &self.config.timings,
                now,
            );
            self.commit(&mut st, &room_id, now);
        }
        for table_id in Self::tables_of(&st, player) {
            let t = st.tables.get_mut(&table_id).expect("table");
            t.set_connected(player, false, &self.config.timings, now);
            self.commit_table(&mut st, &table_id, now);
        }
    }

    // ---------- commands ----------

    /// Handles one text frame from `conn_id`.
    pub fn handle(&self, conn_id: u64, text: &str) {
        let now = now_ms();
        let mut st = self.lock();
        let Some((profile, client)) = st
            .conns
            .get(&conn_id)
            .map(|c| (c.profile.clone(), c.client.clone()))
        else {
            return;
        };
        let reply = |st: &State, msg: ServerMsg| {
            if let Some(c) = st.conns.get(&conn_id) {
                self.send(c, &msg);
            }
        };
        let env: Envelope = match serde_json::from_str(text) {
            Ok(e) => e,
            Err(_) => {
                return reply(
                    &st,
                    error(None, "bad_request", "Не удалось разобрать сообщение"),
                );
            }
        };
        if env.v != PROTOCOL_VERSION {
            return reply(
                &st,
                error(None, "unsupported_version", "Обновите приложение"),
            );
        }
        if env
            .msg
            .ids()
            .iter()
            .any(|id| id.is_empty() || id.len() > MAX_ID_LEN)
        {
            return reply(
                &st,
                error(None, "bad_request", "Не удалось разобрать сообщение"),
            );
        }
        if !self.flush_pending(&mut st, now) {
            return reply(
                &st,
                error(
                    None,
                    "persistence_pending",
                    "Сохраняем результат. Повторите действие",
                ),
            );
        }
        let player = profile.id;
        let timings = self.config.timings;
        let mut dealer = OsDealer;
        if env.msg.is_table() {
            return self.handle_table(&mut st, conn_id, &profile, env.msg, now);
        }

        let result: Result<Option<String>, (Option<String>, RoomError)> = match env.msg {
            ClientMsg::Heartbeat => {
                reply(
                    &st,
                    ServerMsg::Pong {
                        server_time_ms: now,
                    },
                );
                Ok(None)
            }
            ClientMsg::CreateRoom { .. } if player.is_guest() => {
                reply(
                    &st,
                    error(
                        None,
                        "guest_cannot_host",
                        "Создать комнату можно только в игре внутри VK. Войдите по коду друга",
                    ),
                );
                Ok(None)
            }
            ClientMsg::CreateRoom { request_id } => {
                if let Some(id) = st
                    .create_requests
                    .get(&(player, request_id.clone()))
                    .cloned()
                {
                    reply(
                        &st,
                        ServerMsg::RoomCreated {
                            room_id: id.clone(),
                            request_id,
                            reused: false,
                        },
                    );
                    Ok(Some(id))
                } else {
                    // "Играть с другом" again brings the host back to their open room
                    // (a friend may already be waiting in it), so a link already sent
                    // stays valid. Older empty rooms are closed.
                    let mut open: Vec<(bool, u64, String)> = st
                        .rooms
                        .values()
                        .filter(|r| {
                            r.host.profile.id == player
                                && matches!(r.status, Status::Waiting | Status::Ready)
                        })
                        .map(|r| (r.status == Status::Ready, r.created_ms, r.id.clone()))
                        .collect();
                    open.sort();
                    let reuse = open.pop().map(|(_, _, id)| id);
                    let waiting = open.into_iter().filter(|(ready, _, _)| !ready);
                    for (_, _, id) in waiting {
                        let _ = st.rooms.get_mut(&id).expect("room").leave(player, now);
                        self.commit(&mut st, &id, now);
                    }
                    if let Some(id) = reuse {
                        let room = st.rooms.get_mut(&id).expect("room");
                        room.activity_ms = now;
                        room.set_connected(player, true, &timings, now);
                        st.create_requests
                            .insert((player, request_id.clone()), id.clone());
                        tracing::info!(room = %ids::log_tag(&id), "room reused");
                        reply(
                            &st,
                            ServerMsg::RoomCreated {
                                room_id: id.clone(),
                                request_id,
                                reused: true,
                            },
                        );
                        return self.commit(&mut st, &id, now);
                    }
                    let hosted = st
                        .rooms
                        .values()
                        .filter(|r| r.host.profile.id == player && r.status == Status::Ready)
                        .count();
                    if hosted >= MAX_HOSTED_ROOMS {
                        Err((None, RoomError::TooManyRooms))
                    } else {
                        // Codes of open rooms must not clash: a friend would land elsewhere.
                        let id = Self::fresh_id(&st);
                        st.rooms.insert(
                            id.clone(),
                            Room::new(id.clone(), profile.clone(), request_id.clone(), now),
                        );
                        st.create_requests
                            .insert((player, request_id.clone()), id.clone());
                        self.metrics.rooms_created.fetch_add(1, Ordering::Relaxed);
                        tracing::info!(room = %ids::log_tag(&id), "room created");
                        reply(
                            &st,
                            ServerMsg::RoomCreated {
                                room_id: id.clone(),
                                request_id,
                                reused: false,
                            },
                        );
                        Ok(Some(id))
                    }
                }
            }
            ClientMsg::FindRoom { code } => {
                let keys = [format!("u:{player}"), format!("n:{client}")];
                if keys
                    .iter()
                    .any(|k| st.code_misses.count(k, now, CODE_MISSES_WINDOW_MS) >= CODE_MISSES_MAX)
                {
                    reply(
                        &st,
                        error(
                            None,
                            "too_many_attempts",
                            "Слишком много неверных кодов. Попробуйте через 10 минут или попросите ссылку",
                        ),
                    );
                    return;
                }
                let code = ids::parse_code(&code);
                let found = code.as_ref().and_then(|code| {
                    st.rooms
                        .values()
                        .find(|r| r.status == Status::Waiting && ids::room_code(&r.id) == *code)
                });
                let table = code.as_ref().and_then(|code| {
                    st.tables
                        .values()
                        .find(|t| t.joinable() && ids::room_code(&t.id) == *code)
                });
                if let (None, Some(t)) = (found, table) {
                    let preview = t.preview(player);
                    tracing::info!(table = %ids::log_tag(&t.id), "table found by code");
                    reply(&st, ServerMsg::TablePreview { preview });
                    return;
                }
                match found {
                    Some(room) => {
                        let preview = room.preview(player);
                        tracing::info!(
                            room = %ids::log_tag(&room.id),
                            own = preview.already_member,
                            "found by code"
                        );
                        reply(&st, ServerMsg::RoomPreview { preview });
                    }
                    None => {
                        let misses = keys
                            .iter()
                            .map(|k| st.code_misses.add(k, now, CODE_MISSES_WINDOW_MS))
                            .max()
                            .unwrap_or(0);
                        tracing::info!(misses, "code: not found");
                        reply(
                            &st,
                            error(
                                None,
                                "code_not_found",
                                "Комнаты с таким кодом нет. Проверьте цифры — или комната уже закрыта",
                            ),
                        );
                    }
                }
                Ok(None)
            }
            // An invite link may lead to a table: same link format, its own preview.
            ClientMsg::PeekRoom { room_id }
                if !st.rooms.contains_key(&room_id)
                    && self.find_table(&mut st, &room_id).is_some() =>
            {
                let preview = st.tables[&room_id].preview(player);
                tracing::info!(table = %ids::log_tag(&room_id), status = ?preview.status, "table peek");
                reply(&st, ServerMsg::TablePreview { preview });
                Ok(None)
            }
            ClientMsg::PeekRoom { room_id } => match self.find(&mut st, &room_id) {
                Some(room) => {
                    let preview = room.preview(player);
                    tracing::info!(
                        room = %ids::log_tag(&room_id),
                        status = ?room.status,
                        own = preview.already_member,
                        "peek"
                    );
                    reply(&st, ServerMsg::RoomPreview { preview });
                    Ok(None)
                }
                None => {
                    tracing::info!(room = %ids::log_tag(&room_id), "peek: not found");
                    Err((Some(room_id), RoomError::NotFound))
                }
            },
            ClientMsg::JoinRoom { room_id } => self.with_room(&mut st, &room_id, |room, st| {
                let was_waiting = room.status == Status::Waiting;
                let slot = room.join(profile.clone(), now)?;
                if was_waiting && room.status == Status::Ready {
                    let waited = now.saturating_sub(room.created_ms);
                    self.metrics
                        .wait_for_friend_ms_total
                        .fetch_add(waited, Ordering::Relaxed);
                    self.metrics
                        .wait_for_friend_count
                        .fetch_add(1, Ordering::Relaxed);
                    let host = room.host.profile.id;
                    match st.store.head_to_head(host, player) {
                        Ok(score) => room.score = score,
                        Err(e) => tracing::error!("head-to-head: {e}"),
                    }
                }
                room.set_connected(player, true, &timings, now);
                tracing::info!(room = %ids::log_tag(&room.id), ?slot, "joined");
                Ok(())
            }),
            ClientMsg::LeaveRoom { room_id } => {
                self.with_room(&mut st, &room_id, |room, _| room.leave(player, now))
            }
            ClientMsg::SetReady { room_id, ready } => {
                self.with_room(&mut st, &room_id, |room, _| {
                    let before = room.status;
                    room.set_ready(player, ready, &mut dealer, now)?;
                    if before == Status::Ready && room.status == Status::Playing {
                        self.metrics.matches_started.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(())
                })
            }
            ClientMsg::PlayerAction {
                room_id,
                match_id,
                action_id,
                expected_state_version,
                action,
            } => {
                let outcome = st
                    .rooms
                    .get_mut(&room_id)
                    .ok_or(RoomError::NotFound)
                    .and_then(|room| {
                        room.action(
                            player,
                            &match_id,
                            &action_id,
                            expected_state_version,
                            action,
                            &timings,
                            now,
                        )
                    });
                match outcome {
                    Ok(ack) => {
                        self.metrics
                            .actions_accepted
                            .fetch_add(1, Ordering::Relaxed);
                        reply(
                            &st,
                            ServerMsg::ActionAccepted {
                                action_id,
                                state_version: ack.state_version,
                                duplicate: ack.duplicate,
                            },
                        );
                        if ack.duplicate {
                            // Nothing changed; just resend the current state.
                            if let Some(c) = st.conns.get(&conn_id) {
                                self.send_snapshot(&st, c, &room_id, now);
                            }
                            Ok(None)
                        } else {
                            Ok(Some(room_id))
                        }
                    }
                    Err(e) => {
                        self.metrics
                            .actions_rejected
                            .fetch_add(1, Ordering::Relaxed);
                        reply(
                            &st,
                            ServerMsg::ActionRejected {
                                action_id,
                                code: e.code(),
                                message: e.message(),
                            },
                        );
                        // The client may be behind: give it the truth.
                        if let Some(c) = st.conns.get(&conn_id) {
                            self.send_snapshot(&st, c, &room_id, now);
                        }
                        Ok(None)
                    }
                }
            }
            ClientMsg::RequestSnapshot { room_id } => match st.rooms.get(&room_id) {
                Some(room) if room.slot_of(player).is_some() => {
                    if let Some(c) = st.conns.get(&conn_id) {
                        self.send_snapshot(&st, c, &room_id, now);
                    }
                    Ok(None)
                }
                Some(_) => Err((Some(room_id), RoomError::NotMember)),
                None => Err((Some(room_id), RoomError::NotFound)),
            },
            ClientMsg::RequestRematch { room_id } => {
                self.with_room(&mut st, &room_id, |room, _| {
                    room.request_rematch(player, &mut dealer, now)
                })
            }
            ClientMsg::AcceptRematch { room_id } => self.with_room(&mut st, &room_id, |room, _| {
                room.accept_rematch(player, &mut dealer, now)
            }),
            ClientMsg::DeclineRematch { room_id } => {
                self.with_room(&mut st, &room_id, |room, _| {
                    room.decline_rematch(player, now)
                })
            }
            ClientMsg::Surrender { room_id } => {
                self.with_room(&mut st, &room_id, |room, _| room.surrender(player, now))
            }
            ClientMsg::CreateTable { .. }
            | ClientMsg::CreateSolo { .. }
            | ClientMsg::JoinTable { .. }
            | ClientMsg::LeaveTable { .. }
            | ClientMsg::TableAddBot { .. }
            | ClientMsg::TableRemoveBot { .. }
            | ClientMsg::StartTable { .. }
            | ClientMsg::TableAction { .. }
            | ClientMsg::TableAgain { .. }
            | ClientMsg::RequestTableSnapshot { .. } => unreachable!("handled by handle_table"),
        };

        match result {
            Ok(Some(room_id)) => self.commit(&mut st, &room_id, now),
            Ok(None) => {}
            Err((room_id, e)) => reply(&st, error(room_id, e.code(), e.message())),
        }
    }

    /// Runs `f` on a room the user may act in; maps errors for the reply.
    fn with_room(
        &self,
        st: &mut State,
        room_id: &str,
        f: impl FnOnce(&mut Room, &State) -> Result<(), RoomError>,
    ) -> Result<Option<String>, (Option<String>, RoomError)> {
        if self.find(st, room_id).is_none() {
            return Err((Some(room_id.to_string()), RoomError::NotFound));
        }
        // Take the room out so `f` can read the rest of the state.
        let mut room = st.rooms.remove(room_id).expect("found above");
        let result = f(&mut room, st);
        st.rooms.insert(room_id.to_string(), room);
        result
            .map(|()| Some(room_id.to_string()))
            .map_err(|e| (Some(room_id.to_string()), e))
    }

    /// A room in memory, or a closed one brought back from the store.
    fn find<'a>(&self, st: &'a mut State, room_id: &str) -> Option<&'a Room> {
        if !st.rooms.contains_key(room_id) {
            let stored = st.store.room(room_id).ok().flatten()?;
            st.rooms.insert(room_id.to_string(), stored);
        }
        st.rooms.get(room_id)
    }

    /// Persists a changed room, records finished matches and sends every
    /// member a fresh snapshot.
    fn commit(&self, st: &mut State, room_id: &str, now: u64) {
        let Some(room) = st.rooms.get_mut(room_id) else {
            return;
        };
        let events = room.take_events();
        let room = st.rooms.get(room_id).expect("room");
        st.pending_rooms.insert(room_id.to_string());
        if let Err(e) = st.store.save_room(room, now) {
            self.metrics
                .persistence_errors
                .fetch_add(1, Ordering::Relaxed);
            tracing::error!(room = %ids::log_tag(room_id), "save failed: {e}");
        } else {
            st.pending_rooms.remove(room_id);
        }
        for event in events {
            let Event::MatchFinished { forfeit, .. } = event;
            self.metrics
                .matches_finished
                .fetch_add(1, Ordering::Relaxed);
            if forfeit.is_some() {
                self.metrics.forfeits.fetch_add(1, Ordering::Relaxed);
            }
        }
        for m in room.members() {
            if let Some(c) = st
                .by_user
                .get(&m.profile.id)
                .and_then(|id| st.conns.get(id))
            {
                self.send_snapshot(st, c, room_id, now);
            }
        }
    }

    /// A new room or table id whose six-digit code no open room or table uses:
    /// a friend typing it must land in the right place.
    fn fresh_id(st: &State) -> String {
        let taken: std::collections::HashSet<String> = st
            .rooms
            .values()
            .filter(|r| r.is_open())
            .map(|r| ids::room_code(&r.id))
            .chain(
                st.tables
                    .values()
                    .filter(|t| t.is_open())
                    .map(|t| ids::room_code(&t.id)),
            )
            .collect();
        std::iter::repeat_with(ids::room_id)
            .find(|id| !taken.contains(&ids::room_code(id)))
            .expect("an unused code")
    }

    /// A table in memory, or a closed one brought back from the store.
    fn find_table<'a>(&self, st: &'a mut State, id: &str) -> Option<&'a TableRoom> {
        if !st.tables.contains_key(id) {
            let stored = st.store.table(id).ok().flatten()?;
            st.tables.insert(id.to_string(), stored);
        }
        st.tables.get(id)
    }

    /// Handles a table message; replies and commits on its own.
    fn handle_table(
        &self,
        st: &mut State,
        conn_id: u64,
        profile: &Profile,
        msg: ClientMsg,
        now: u64,
    ) {
        let player = profile.id;
        let timings = self.config.timings;
        let mut dealer = OsDealer;
        let reply = |st: &State, msg: ServerMsg| {
            if let Some(c) = st.conns.get(&conn_id) {
                self.send(c, &msg);
            }
        };
        let fail = |st: &State, id: Option<String>, e: RoomError| {
            reply(st, error(id, e.code(), e.message()));
        };
        // Runs `f` on a table; on success the table is saved and broadcast.
        let on_table =
            |st: &mut State,
             id: String,
             f: &mut dyn FnMut(&mut TableRoom) -> Result<(), RoomError>| {
                if self.find_table(st, &id).is_none() {
                    return fail(st, Some(id), RoomError::NotFound);
                }
                let t = st.tables.get_mut(&id).expect("found above");
                match f(t) {
                    Ok(()) => self.commit_table(st, &id, now),
                    Err(e) => fail(st, Some(id), e),
                }
            };
        match msg {
            ClientMsg::CreateTable { .. } | ClientMsg::CreateSolo { .. } if player.is_guest() => {
                reply(
                    st,
                    error(
                        None,
                        "guest_cannot_host",
                        "Собрать стол можно только в игре внутри VK или Telegram. Войдите по коду друга",
                    ),
                )
            }
            ClientMsg::CreateSolo {
                request_id,
                opponent,
            } => {
                let key = (player, request_id.clone());
                // A retry (including after restart) keeps the same match. A new
                // request also resumes an unfinished Solo instead of farming starts.
                let existing = st.table_requests.get(&key).cloned().or_else(|| {
                    st.tables
                        .values()
                        .find(|t| {
                            t.host == player
                                && t.status == TableStatus::Playing
                                && t.seats.len() == 2
                                && t.seats
                                    .iter()
                                    .any(|s| matches!(s, crate::table_room::TableSeat::Bot { .. }))
                        })
                        .map(|t| t.id.clone())
                });
                let (id, reused) = if let Some(id) = existing {
                    (id, true)
                } else {
                    let id = Self::fresh_id(st);
                    let created = (|| {
                        let mut t = TableRoom::new(
                            id.clone(),
                            profile.clone(),
                            2,
                            request_id.clone(),
                            now,
                        )?;
                        t.add_bot(player, opponent, now)?;
                        t.start(player, &mut dealer, now)?;
                        Ok::<_, RoomError>(t)
                    })();
                    match created {
                        Ok(t) => {
                            st.tables.insert(id.clone(), t);
                            self.metrics.tables_created.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => return fail(st, None, e),
                    }
                    (id, false)
                };
                st.table_requests.insert(key, id.clone());
                // Save before acknowledging. If persistence fails, replay the
                // original command after reconnect; no new game is created.
                self.commit_table(st, &id, now);
                if !st.pending_tables.contains(&id) {
                    reply(
                        st,
                        ServerMsg::TableCreated {
                            table_id: id.clone(),
                            request_id,
                            reused,
                        },
                    );
                    if let Some(c) = st.conns.get(&conn_id) {
                        self.send_table_snapshot(st, c, &id, now);
                    }
                } else {
                    reply(
                        st,
                        error(
                            None,
                            "persistence_pending",
                            "Сохраняем игру. Повторите подключение.",
                        ),
                    );
                }
            }
            ClientMsg::CreateTable { request_id, seats } => {
                let key = (player, request_id.clone());
                if let Some(id) = st.table_requests.get(&key).cloned() {
                    reply(
                        st,
                        ServerMsg::TableCreated {
                            table_id: id.clone(),
                            request_id,
                            reused: false,
                        },
                    );
                    return self.commit_table(st, &id, now);
                }
                // "Стол с друзьями" again returns to an open lobby of the same
                // size, so a link already sent keeps working; others close.
                let lobbies: Vec<(usize, String)> = st
                    .tables
                    .values()
                    .filter(|t| t.host == player && t.status == TableStatus::Lobby)
                    .map(|t| (t.seats.len(), t.id.clone()))
                    .collect();
                let reuse = lobbies
                    .iter()
                    .find(|(n, _)| *n == seats)
                    .map(|(_, id)| id.clone());
                for (_, id) in lobbies.iter().filter(|(_, id)| Some(id) != reuse.as_ref()) {
                    let _ = st.tables.get_mut(id).expect("table").leave(player, now);
                    self.commit_table(st, id, now);
                }
                let (id, reused) = match reuse {
                    Some(id) => (id, true),
                    None => {
                        let id = Self::fresh_id(st);
                        match TableRoom::new(
                            id.clone(),
                            profile.clone(),
                            seats,
                            request_id.clone(),
                            now,
                        ) {
                            Ok(t) => {
                                st.tables.insert(id.clone(), t);
                                self.metrics.tables_created.fetch_add(1, Ordering::Relaxed);
                                tracing::info!(table = %ids::log_tag(&id), seats, "table created");
                                (id, false)
                            }
                            Err(e) => return fail(st, None, e),
                        }
                    }
                };
                st.table_requests.insert(key, id.clone());
                reply(
                    st,
                    ServerMsg::TableCreated {
                        table_id: id.clone(),
                        request_id,
                        reused,
                    },
                );
                self.commit_table(st, &id, now);
            }
            ClientMsg::JoinTable { table_id } => on_table(st, table_id, &mut |t| {
                let seat = t.join(profile.clone(), now)?;
                t.set_connected(player, true, &timings, now);
                tracing::info!(table = %ids::log_tag(&t.id), seat, "joined table");
                Ok(())
            }),
            ClientMsg::LeaveTable { table_id } => {
                on_table(st, table_id, &mut |t| t.leave(player, now))
            }
            ClientMsg::TableAddBot { table_id, opponent } => {
                on_table(st, table_id, &mut |t| t.add_bot(player, opponent, now))
            }
            ClientMsg::TableRemoveBot { table_id, seat } => {
                on_table(st, table_id, &mut |t| t.remove_bot(player, seat, now))
            }
            ClientMsg::StartTable { table_id } => on_table(st, table_id, &mut |t| {
                t.start(player, &mut dealer, now)?;
                self.metrics.tables_started.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }),
            ClientMsg::TableAgain { table_id } => on_table(st, table_id, &mut |t| {
                t.vote_again(player, &mut dealer, now)
            }),
            ClientMsg::RequestTableSnapshot { table_id } => {
                match self
                    .find_table(st, &table_id)
                    .map(|t| t.seat_of(player).is_some())
                {
                    Some(true) => {
                        if let Some(c) = st.conns.get(&conn_id) {
                            self.send_table_snapshot(st, c, &table_id, now);
                        }
                    }
                    Some(false) => fail(st, Some(table_id), RoomError::NotMember),
                    None => fail(st, Some(table_id), RoomError::NotFound),
                }
            }
            ClientMsg::TableAction {
                table_id,
                match_id,
                action_id,
                expected_state_version,
                action,
            } => {
                let outcome = st
                    .tables
                    .get_mut(&table_id)
                    .ok_or(RoomError::NotFound)
                    .and_then(|t| {
                        t.action(
                            player,
                            &match_id,
                            &action_id,
                            expected_state_version,
                            action,
                            &timings,
                            now,
                        )
                    });
                match outcome {
                    Ok(ack) => {
                        self.metrics
                            .actions_accepted
                            .fetch_add(1, Ordering::Relaxed);
                        reply(
                            st,
                            ServerMsg::ActionAccepted {
                                action_id,
                                state_version: ack.state_version,
                                duplicate: ack.duplicate,
                            },
                        );
                        if ack.duplicate {
                            if let Some(c) = st.conns.get(&conn_id) {
                                self.send_table_snapshot(st, c, &table_id, now);
                            }
                        } else {
                            self.commit_table(st, &table_id, now);
                        }
                    }
                    Err(e) => {
                        self.metrics
                            .actions_rejected
                            .fetch_add(1, Ordering::Relaxed);
                        reply(
                            st,
                            ServerMsg::ActionRejected {
                                action_id,
                                code: e.code(),
                                message: e.message(),
                            },
                        );
                        if let Some(c) = st.conns.get(&conn_id) {
                            self.send_table_snapshot(st, c, &table_id, now);
                        }
                    }
                }
            }
            _ => unreachable!("only table messages come here"),
        }
    }

    /// Persists a changed table and sends every person at it a snapshot.
    fn commit_table(&self, st: &mut State, id: &str, now: u64) {
        let Some(t) = st.tables.get_mut(id) else {
            return;
        };
        for event in t.take_events() {
            let TableEvent::Finished { people, bots } = event;
            self.metrics.tables_finished.fetch_add(1, Ordering::Relaxed);
            tracing::info!(table = %ids::log_tag(id), people, bots, "table finished");
        }
        let t = st.tables.get(id).expect("table");
        st.pending_tables.insert(id.to_string());
        if let Err(e) = st.store.save_table(t, now) {
            self.metrics
                .persistence_errors
                .fetch_add(1, Ordering::Relaxed);
            tracing::error!(table = %ids::log_tag(id), "save failed: {e}");
        } else {
            st.pending_tables.remove(id);
        }
        // Everyone seated, including who just left: they see the result.
        let players: Vec<PlayerId> = t
            .seats
            .iter()
            .filter_map(|s| match s {
                crate::table_room::TableSeat::Player { member, .. } => Some(member.profile.id),
                _ => None,
            })
            .collect();
        for p in players {
            if let Some(c) = st.by_user.get(&p).and_then(|c| st.conns.get(c)) {
                self.send_table_snapshot(st, c, id, now);
            }
        }
    }

    fn send_table_snapshot(&self, st: &State, conn: &Conn, id: &str, now: u64) {
        let Some(view) = st
            .tables
            .get(id)
            .and_then(|t| t.view_for(conn.profile.id, &self.config.timings))
        else {
            return;
        };
        self.send(
            conn,
            &ServerMsg::TableSnapshot {
                server_time_ms: now,
                table: Box::new(view),
            },
        );
    }

    fn send_snapshot(&self, st: &State, conn: &Conn, room_id: &str, now: u64) {
        let Some(view) = st
            .rooms
            .get(room_id)
            .and_then(|r| r.view_for(conn.profile.id, &self.config.timings))
        else {
            return;
        };
        self.send(
            conn,
            &ServerMsg::StateSnapshot {
                server_time_ms: now,
                room: Box::new(view),
            },
        );
    }

    fn flush_pending(&self, st: &mut State, now: u64) -> bool {
        st.pending_rooms.retain(|id| {
            st.rooms
                .get(id)
                .is_some_and(|r| st.store.save_room(r, now).is_err())
        });
        st.pending_tables.retain(|id| {
            st.tables
                .get(id)
                .is_some_and(|t| st.store.save_table(t, now).is_err())
        });
        st.pending_rooms.is_empty() && st.pending_tables.is_empty()
    }

    /// Timers for every room; called a few times per second.
    pub fn tick(&self) {
        let now = now_ms();
        let mut st = self.lock();
        if !self.flush_pending(&mut st, now) {
            return;
        }
        crate::progress::publish_events(&st.store.conn);
        let mut dealer = OsDealer;
        let ids: Vec<String> = st.rooms.keys().cloned().collect();
        for id in ids {
            let room = st.rooms.get_mut(&id).expect("room");
            if room.tick(&mut dealer as &mut dyn Dealer, &self.config.timings, now) {
                self.commit(&mut st, &id, now);
            }
        }
        let ids: Vec<String> = st.tables.keys().cloned().collect();
        for id in ids {
            let t = st.tables.get_mut(&id).expect("table");
            if t.tick(&mut dealer as &mut dyn Dealer, &self.config.timings, now) {
                self.commit_table(&mut st, &id, now);
            }
        }
        st.rooms
            .retain(|_, r| r.is_open() || now.saturating_sub(r.activity_ms) < CLOSED_IN_MEMORY_MS);
        st.tables
            .retain(|_, t| t.is_open() || now.saturating_sub(t.activity_ms) < CLOSED_IN_MEMORY_MS);
        let live: std::collections::HashSet<String> = st.rooms.keys().cloned().collect();
        st.create_requests.retain(|_, id| live.contains(id));
        let live: std::collections::HashSet<String> = st.tables.keys().cloned().collect();
        st.table_requests.retain(|_, id| live.contains(id));
        st.local_reports.prune(now, 60_000);
        st.daily_analytics.prune(now, 60_000);
        st.code_misses.prune(now, CODE_MISSES_WINDOW_MS);
        st.guest_auths.prune(now, GUEST_AUTH_WINDOW_MS);
        st.sessions.prune(now);
    }

    /// Housekeeping that may touch the disk; called rarely.
    pub fn prune_store(&self) {
        let st = self.lock();
        if let Err(e) = st.store.prune_closed(now_ms(), CLOSED_IN_DB_MS) {
            tracing::error!("prune failed: {e}");
        }
    }

    pub fn health(&self) -> serde_json::Value {
        let st = self.lock();
        let mut by_status: HashMap<&'static str, usize> = HashMap::new();
        for r in st.rooms.values() {
            let name = match r.status {
                Status::Waiting => "waiting",
                Status::Ready => "ready",
                Status::Playing => "playing",
                Status::Finished => "finished",
                Status::Closed => "closed",
            };
            *by_status.entry(name).or_default() += 1;
        }
        let mut tables: HashMap<&'static str, usize> = HashMap::new();
        for t in st.tables.values() {
            let name = match t.status {
                TableStatus::Lobby => "lobby",
                TableStatus::Playing => "playing",
                TableStatus::Finished => "finished",
                TableStatus::Closed => "closed",
            };
            *tables.entry(name).or_default() += 1;
        }
        serde_json::json!({
            "ok": true,
            "protocol": PROTOCOL_VERSION,
            "rooms": by_status,
            "tables": tables,
            "connections": st.conns.len(),
            "metrics": self.metrics,
        })
    }
}

fn error(room_id: Option<String>, code: &'static str, message: &'static str) -> ServerMsg {
    ServerMsg::ServerError {
        code,
        message,
        room_id,
    }
}
