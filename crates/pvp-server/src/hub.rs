//! The hub: all rooms, all connections, one lock.
//!
//! Every command runs under a single mutex: it mutates one room, persists it
//! and sends each member their own snapshot. That makes joins, ready and
//! actions atomic without any distributed locking (one server process is
//! enough for private duels). Nothing here awaits while holding the lock.

use crate::auth::Sessions;
use crate::config::Config;
use crate::ids;
use crate::protocol::{
    ClientMsg, Envelope, MAX_ID_LEN, Outbound, PROTOCOL_VERSION, PublicUser, ServerMsg,
};
use crate::room::{Dealer, Event, OsDealer, Profile, Room, RoomError, Status};
use crate::store::Store;
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
/// Wrong room codes one user may try per window: a million codes are not
/// guessable at this rate.
const CODE_MISSES_MAX: u32 = 20;
const CODE_MISSES_WINDOW_MS: u64 = 10 * 60_000;

/// What the socket task should do.
#[derive(Debug)]
pub enum Outgoing {
    Text(String),
    /// Close the socket (e.g. the same account connected elsewhere).
    Close,
}

struct Conn {
    profile: Profile,
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
}

struct State {
    rooms: HashMap<String, Room>,
    conns: HashMap<u64, Conn>,
    /// The one live connection per VK user.
    by_user: HashMap<u64, u64>,
    /// (user, request_id) → room id, for idempotent `create_room`.
    create_requests: HashMap<(u64, String), String>,
    /// Wrong room codes per user: (window start, misses).
    code_misses: HashMap<u64, (u64, u32)>,
    sessions: Sessions,
    store: Store,
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
            .map(|r| {
                (
                    (r.host.profile.vk_id, r.create_request.clone()),
                    r.id.clone(),
                )
            })
            .collect();
        Hub {
            config,
            state: Mutex::new(State {
                rooms,
                conns: HashMap::new(),
                by_user: HashMap::new(),
                create_requests,
                code_misses: HashMap::new(),
                sessions: Sessions::default(),
                store,
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

    pub fn ticket_for(&self, session: &str) -> Option<String> {
        self.lock().sessions.ticket_for(session, now_ms())
    }

    pub fn redeem_ticket(&self, ticket: &str) -> Option<Profile> {
        self.lock().sessions.redeem(ticket, now_ms())
    }

    // ---------- connections ----------

    /// A verified user opened a socket. An older socket of the same user is
    /// told it was replaced and closed: one account, one live connection.
    pub fn connect(&self, profile: Profile) -> (u64, mpsc::UnboundedReceiver<Outgoing>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let now = now_ms();
        let mut st = self.lock();
        let id = st.next_conn;
        st.next_conn += 1;
        let vk = profile.vk_id;
        if let Some(old) = st
            .by_user
            .insert(vk, id)
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
            .filter(|r| r.is_open() && r.slot_of(vk).is_some())
            .map(|r| r.id.clone())
            .collect();
        for room_id in ids {
            let room = st.rooms.get_mut(&room_id).expect("room");
            let was_away = room
                .members()
                .any(|m| m.profile.vk_id == vk && !m.connected);
            room.set_connected(vk, true, &self.config.timings, now);
            if was_away {
                self.metrics.reconnects.fetch_add(1, Ordering::Relaxed);
            }
            self.commit(&mut st, &room_id, now);
        }
        (id, rx)
    }

    pub fn disconnect(&self, conn_id: u64) {
        let now = now_ms();
        let mut st = self.lock();
        let Some(conn) = st.conns.remove(&conn_id) else {
            return;
        };
        let vk = conn.profile.vk_id;
        if st.by_user.get(&vk) != Some(&conn_id) {
            return; // replaced by a newer connection, which keeps the seat
        }
        st.by_user.remove(&vk);
        let ids: Vec<String> = st
            .rooms
            .values()
            .filter(|r| r.is_open() && r.slot_of(vk).is_some())
            .map(|r| r.id.clone())
            .collect();
        for room_id in ids {
            st.rooms.get_mut(&room_id).expect("room").set_connected(
                vk,
                false,
                &self.config.timings,
                now,
            );
            self.commit(&mut st, &room_id, now);
        }
    }

    // ---------- commands ----------

    /// Handles one text frame from `conn_id`.
    pub fn handle(&self, conn_id: u64, text: &str) {
        let now = now_ms();
        let mut st = self.lock();
        let Some(profile) = st.conns.get(&conn_id).map(|c| c.profile.clone()) else {
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
        let vk = profile.vk_id;
        let timings = self.config.timings;
        let mut dealer = OsDealer;

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
            ClientMsg::CreateRoom { .. } if crate::room::is_guest(vk) => {
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
                if let Some(id) = st.create_requests.get(&(vk, request_id.clone())).cloned() {
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
                            r.host.profile.vk_id == vk
                                && matches!(r.status, Status::Waiting | Status::Ready)
                        })
                        .map(|r| (r.status == Status::Ready, r.created_ms, r.id.clone()))
                        .collect();
                    open.sort();
                    let reuse = open.pop().map(|(_, _, id)| id);
                    let waiting = open.into_iter().filter(|(ready, _, _)| !ready);
                    for (_, _, id) in waiting {
                        let _ = st.rooms.get_mut(&id).expect("room").leave(vk, now);
                        self.commit(&mut st, &id, now);
                    }
                    if let Some(id) = reuse {
                        let room = st.rooms.get_mut(&id).expect("room");
                        room.activity_ms = now;
                        room.set_connected(vk, true, &timings, now);
                        st.create_requests
                            .insert((vk, request_id.clone()), id.clone());
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
                        .filter(|r| r.host.profile.vk_id == vk && r.status == Status::Ready)
                        .count();
                    if hosted >= MAX_HOSTED_ROOMS {
                        Err((None, RoomError::TooManyRooms))
                    } else {
                        // Codes of open rooms must not clash: a friend would land elsewhere.
                        let taken: std::collections::HashSet<String> = st
                            .rooms
                            .values()
                            .filter(|r| r.is_open())
                            .map(|r| ids::room_code(&r.id))
                            .collect();
                        let id = std::iter::repeat_with(ids::room_id)
                            .find(|id| !taken.contains(&ids::room_code(id)))
                            .expect("an unused code");
                        st.rooms.insert(
                            id.clone(),
                            Room::new(id.clone(), profile.clone(), request_id.clone(), now),
                        );
                        st.create_requests
                            .insert((vk, request_id.clone()), id.clone());
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
                let (start, misses) = st.code_misses.get(&vk).copied().unwrap_or((now, 0));
                let (start, misses) = if now.saturating_sub(start) > CODE_MISSES_WINDOW_MS {
                    (now, 0)
                } else {
                    (start, misses)
                };
                if misses >= CODE_MISSES_MAX {
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
                let found = ids::parse_code(&code).and_then(|code| {
                    st.rooms
                        .values()
                        .find(|r| r.status == Status::Waiting && ids::room_code(&r.id) == code)
                });
                match found {
                    Some(room) => {
                        let preview = room.preview(vk);
                        tracing::info!(
                            room = %ids::log_tag(&room.id),
                            own = preview.already_member,
                            "found by code"
                        );
                        reply(&st, ServerMsg::RoomPreview { preview });
                    }
                    None => {
                        st.code_misses.insert(vk, (start, misses + 1));
                        tracing::info!(misses = misses + 1, "code: not found");
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
            ClientMsg::PeekRoom { room_id } => match self.find(&mut st, &room_id) {
                Some(room) => {
                    let preview = room.preview(vk);
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
                    let host = room.host.profile.vk_id;
                    match st.store.head_to_head(host, vk) {
                        Ok(score) => room.score = score,
                        Err(e) => tracing::error!("head-to-head: {e}"),
                    }
                }
                room.set_connected(vk, true, &timings, now);
                tracing::info!(room = %ids::log_tag(&room.id), ?slot, "joined");
                Ok(())
            }),
            ClientMsg::LeaveRoom { room_id } => {
                self.with_room(&mut st, &room_id, |room, _| room.leave(vk, now))
            }
            ClientMsg::SetReady { room_id, ready } => {
                self.with_room(&mut st, &room_id, |room, _| {
                    let before = room.status;
                    room.set_ready(vk, ready, &mut dealer, now)?;
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
                            vk,
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
                Some(room) if room.slot_of(vk).is_some() => {
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
                    room.request_rematch(vk, &mut dealer, now)
                })
            }
            ClientMsg::AcceptRematch { room_id } => self.with_room(&mut st, &room_id, |room, _| {
                room.accept_rematch(vk, &mut dealer, now)
            }),
            ClientMsg::DeclineRematch { room_id } => {
                self.with_room(&mut st, &room_id, |room, _| room.decline_rematch(vk, now))
            }
            ClientMsg::Surrender { room_id } => {
                self.with_room(&mut st, &room_id, |room, _| room.surrender(vk, now))
            }
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
        if let Err(e) = st.store.save_room(room, now) {
            self.metrics
                .persistence_errors
                .fetch_add(1, Ordering::Relaxed);
            tracing::error!(room = %ids::log_tag(room_id), "save failed: {e}");
        }
        for event in events {
            let Event::MatchFinished {
                host,
                guest,
                outcome_for_host,
                forfeit,
            } = event;
            self.metrics
                .matches_finished
                .fetch_add(1, Ordering::Relaxed);
            if forfeit.is_some() {
                self.metrics.forfeits.fetch_add(1, Ordering::Relaxed);
            }
            if let Err(e) = st.store.record_duel(host, guest, outcome_for_host) {
                self.metrics
                    .persistence_errors
                    .fetch_add(1, Ordering::Relaxed);
                tracing::error!("record duel failed: {e}");
            }
        }
        for m in room.members() {
            if let Some(c) = st
                .by_user
                .get(&m.profile.vk_id)
                .and_then(|id| st.conns.get(id))
            {
                self.send_snapshot(st, c, room_id, now);
            }
        }
    }

    fn send_snapshot(&self, st: &State, conn: &Conn, room_id: &str, now: u64) {
        let Some(view) = st
            .rooms
            .get(room_id)
            .and_then(|r| r.view_for(conn.profile.vk_id, &self.config.timings))
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

    /// Timers for every room; called a few times per second.
    pub fn tick(&self) {
        let now = now_ms();
        let mut st = self.lock();
        let mut dealer = OsDealer;
        let ids: Vec<String> = st.rooms.keys().cloned().collect();
        for id in ids {
            let room = st.rooms.get_mut(&id).expect("room");
            if room.tick(&mut dealer as &mut dyn Dealer, &self.config.timings, now) {
                self.commit(&mut st, &id, now);
            }
        }
        st.rooms
            .retain(|_, r| r.is_open() || now.saturating_sub(r.activity_ms) < CLOSED_IN_MEMORY_MS);
        let live: std::collections::HashSet<String> = st.rooms.keys().cloned().collect();
        st.create_requests.retain(|_, id| live.contains(id));
        st.code_misses
            .retain(|_, (start, _)| now.saturating_sub(*start) <= CODE_MISSES_WINDOW_MS);
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
        serde_json::json!({
            "ok": true,
            "protocol": PROTOCOL_VERSION,
            "rooms": by_status,
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
