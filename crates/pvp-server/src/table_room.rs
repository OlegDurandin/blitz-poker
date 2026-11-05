//! A table for friends ("Третий не лишний" online): 2–4 seats, people and
//! bots. A state machine with no I/O, like [`crate::room`].
//!
//! ```text
//! Lobby ──host starts (2+ seats taken)──▶ Playing ──match over──▶ Finished
//!   │                                       │                      │   ▲
//!   │                         every person gone: Closed            │   │
//!   │                                                 all people vote "ещё"
//! Lobby/Finished ──host cancels, TTL, everyone left──▶ Closed
//! ```
//!
//! The host sits in seat 0. Friends take free seats in the lobby; the host
//! may seat bots in the rest. When the game starts, empty seats are dropped
//! and the remaining order becomes the engine's seat numbers. Bots decide on
//! the server (`bot::decide_table`) after a short pause; people have a turn
//! clock and a time bank as in a duel. A player who leaves mid-game, or does
//! not come back in time, folds and sits out the rest of the match.

use crate::player::PlayerId;
use crate::room::{Dealer, Member, Profile, RoomError, Timings};
use poker_core::table::{SeatKind, Table, TableView};
use poker_core::{Opponent, Phase, PlayerAction};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MIN_CAPACITY: usize = 2;
pub const MAX_CAPACITY: usize = 4;
const RECENT_ACTIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableStatus {
    Lobby,
    Playing,
    Finished,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableCloseReason {
    CancelledByHost,
    Expired,
    /// Every person left.
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TableSeat {
    Empty,
    Player {
        member: Member,
        /// Left the table: folded out of the current match, gone after it.
        #[serde(default)]
        gone: bool,
    },
    Bot {
        opponent: Opponent,
    },
}

impl TableSeat {
    fn player(&self) -> Option<PlayerId> {
        match self {
            TableSeat::Player { member, .. } => Some(member.profile.id),
            _ => None,
        }
    }

    /// A person still at the table.
    fn present(&self) -> Option<&Member> {
        match self {
            TableSeat::Player {
                member,
                gone: false,
            } => Some(member),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableGame {
    pub match_id: String,
    pub t: Table,
    /// Bumped on every accepted transition; clients send it back with actions.
    pub version: u64,
    pub turn_started_ms: u64,
    /// Remaining time bank per seat (people only use it).
    pub bank_ms: Vec<u64>,
    pub hand_over_ms: Option<u64>,
    recent: VecDeque<(String, u64)>,
    /// After a server restart the clock waits for the acting person.
    #[serde(default)]
    hold_turn: bool,
}

impl TableGame {
    fn deadline(&self, seat: usize, timings: &Timings) -> u64 {
        self.turn_started_ms + timings.turn_ms + bank_of(self, seat, timings)
    }

    fn pause(&self, timings: &Timings) -> u64 {
        let runout = self
            .t
            .hand
            .outcome
            .as_ref()
            .is_some_and(|o| o.runout_from.is_some());
        if runout {
            timings.runout_pause_ms
        } else {
            timings.hand_pause_ms
        }
    }
}

/// Reported to the hub when a match ends (metrics, logs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableEvent {
    Finished { people: usize, bots: usize },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRoom {
    pub id: String,
    pub created_ms: u64,
    pub activity_ms: u64,
    pub status: TableStatus,
    pub close_reason: Option<TableCloseReason>,
    pub host: PlayerId,
    pub seats: Vec<TableSeat>,
    pub game: Option<TableGame>,
    /// People who want another match with the same line-up.
    pub again: Vec<PlayerId>,
    pub create_request: String,
    #[serde(skip)]
    events: Vec<TableEvent>,
}

impl TableRoom {
    pub fn new(
        id: String,
        host: Profile,
        capacity: usize,
        create_request: String,
        now: u64,
    ) -> Result<TableRoom, RoomError> {
        if !(MIN_CAPACITY..=MAX_CAPACITY).contains(&capacity) {
            return Err(RoomError::Illegal);
        }
        let mut seats = vec![TableSeat::Empty; capacity];
        let host_id = host.id;
        seats[0] = TableSeat::Player {
            member: Member::new(host),
            gone: false,
        };
        Ok(TableRoom {
            id,
            created_ms: now,
            activity_ms: now,
            status: TableStatus::Lobby,
            close_reason: None,
            host: host_id,
            seats,
            game: None,
            again: Vec::new(),
            create_request,
            events: Vec::new(),
        })
    }

    pub fn is_open(&self) -> bool {
        self.status != TableStatus::Closed
    }

    pub fn take_events(&mut self) -> Vec<TableEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn seat_of(&self, player: PlayerId) -> Option<usize> {
        self.seats.iter().position(|s| s.player() == Some(player))
    }

    /// People still at the table (not gone).
    pub fn members(&self) -> impl Iterator<Item = &Member> {
        self.seats.iter().filter_map(TableSeat::present)
    }

    fn require(&self, player: PlayerId) -> Result<usize, RoomError> {
        match self.seat_of(player) {
            Some(i) if self.seats[i].present().is_some() => Ok(i),
            _ => Err(RoomError::NotMember),
        }
    }

    fn require_host(&self, player: PlayerId) -> Result<(), RoomError> {
        self.require(player)?;
        if player == self.host {
            Ok(())
        } else {
            Err(RoomError::NotMember)
        }
    }

    fn close(&mut self, reason: TableCloseReason) {
        self.status = TableStatus::Closed;
        self.close_reason = Some(reason);
        self.again.clear();
    }

    pub fn joinable(&self) -> bool {
        self.status == TableStatus::Lobby && self.seats.contains(&TableSeat::Empty)
    }

    /// Take a free seat in the lobby. Someone already seated gets their seat
    /// back (a reload, another device).
    pub fn join(&mut self, profile: Profile, now: u64) -> Result<usize, RoomError> {
        if self.status == TableStatus::Closed {
            return Err(RoomError::Closed);
        }
        if let Some(i) = self.seat_of(profile.id) {
            if let TableSeat::Player { member, gone } = &mut self.seats[i] {
                if *gone {
                    return Err(RoomError::Closed);
                }
                member.profile = profile;
            }
            return Ok(i);
        }
        if self.status != TableStatus::Lobby {
            return Err(RoomError::Full);
        }
        let i = self
            .seats
            .iter()
            .position(|s| *s == TableSeat::Empty)
            .ok_or(RoomError::Full)?;
        self.seats[i] = TableSeat::Player {
            member: Member::new(profile),
            gone: false,
        };
        self.activity_ms = now;
        Ok(i)
    }

    /// Host, in the lobby: a bot takes the first free seat. Each bot once.
    pub fn add_bot(
        &mut self,
        player: PlayerId,
        opponent: Opponent,
        now: u64,
    ) -> Result<(), RoomError> {
        self.require_host(player)?;
        if self.status != TableStatus::Lobby {
            return Err(RoomError::WrongState);
        }
        if self.seats.contains(&TableSeat::Bot { opponent }) {
            return Err(RoomError::Illegal);
        }
        let i = self
            .seats
            .iter()
            .position(|s| *s == TableSeat::Empty)
            .ok_or(RoomError::Full)?;
        self.seats[i] = TableSeat::Bot { opponent };
        self.activity_ms = now;
        Ok(())
    }

    /// Host, in the lobby: frees a bot's seat for a friend.
    pub fn remove_bot(&mut self, player: PlayerId, seat: usize, now: u64) -> Result<(), RoomError> {
        self.require_host(player)?;
        if self.status != TableStatus::Lobby {
            return Err(RoomError::WrongState);
        }
        match self.seats.get(seat) {
            Some(TableSeat::Bot { .. }) => {
                self.seats[seat] = TableSeat::Empty;
                self.activity_ms = now;
                Ok(())
            }
            _ => Err(RoomError::Illegal),
        }
    }

    /// Lobby: a friend frees their seat, the host closes the table.
    /// Playing: fold and sit out the rest of the match.
    /// Finished: leave the line-up for the next match.
    pub fn leave(&mut self, player: PlayerId, now: u64) -> Result<(), RoomError> {
        let seat = self.require(player)?;
        self.activity_ms = now;
        match self.status {
            TableStatus::Closed => {}
            TableStatus::Lobby if player == self.host => {
                self.close(TableCloseReason::CancelledByHost)
            }
            TableStatus::Lobby => self.seats[seat] = TableSeat::Empty,
            TableStatus::Playing => {
                self.mark_gone(seat);
                let game = self.game.as_mut().expect("playing table has a game");
                // Out of chips, the place is already earned: just go.
                if game.t.stacks[seat] > 0 && !game.t.left.contains(&seat) {
                    game.t.leave(seat)?;
                    self.after_transition(now);
                }
                self.close_if_empty();
            }
            TableStatus::Finished => {
                self.mark_gone(seat);
                self.again.retain(|p| *p != player);
                self.close_if_empty();
            }
        }
        Ok(())
    }

    fn mark_gone(&mut self, seat: usize) {
        if let TableSeat::Player { gone, member } = &mut self.seats[seat] {
            *gone = true;
            member.grace_until_ms = None;
        }
    }

    /// Nobody left to play for: bots never play alone.
    fn close_if_empty(&mut self) {
        if self.status != TableStatus::Closed && self.members().next().is_none() {
            self.close(TableCloseReason::Empty);
        }
    }

    /// Host: deal the first hand with everyone seated now.
    pub fn start(
        &mut self,
        player: PlayerId,
        dealer: &mut dyn Dealer,
        now: u64,
    ) -> Result<(), RoomError> {
        self.require_host(player)?;
        if self.status != TableStatus::Lobby {
            return Err(RoomError::WrongState);
        }
        if self
            .seats
            .iter()
            .filter(|s| **s != TableSeat::Empty)
            .count()
            < 2
        {
            return Err(RoomError::WrongState);
        }
        self.seats.retain(|s| *s != TableSeat::Empty);
        self.deal_match(dealer, now);
        Ok(())
    }

    fn deal_match(&mut self, dealer: &mut dyn Dealer, now: u64) {
        // Who left during the last match is not part of this one.
        self.seats
            .retain(|s| !matches!(s, TableSeat::Player { gone: true, .. }));
        let kinds: Vec<SeatKind> = self
            .seats
            .iter()
            .map(|s| match s {
                TableSeat::Bot { opponent } => SeatKind::Bot {
                    opponent: *opponent,
                },
                _ => SeatKind::Player,
            })
            .collect();
        let n = kinds.len();
        let button = random_seat(dealer, n);
        let match_id = dealer.match_id();
        let t =
            Table::online(match_id.clone(), kinds, button, &dealer.deck()).expect("a valid table");
        self.game = Some(TableGame {
            match_id,
            t,
            version: 1,
            turn_started_ms: now,
            bank_ms: vec![u64::MAX; n],
            hand_over_ms: None,
            recent: VecDeque::new(),
            hold_turn: false,
        });
        self.status = TableStatus::Playing;
        self.again.clear();
        for s in &mut self.seats {
            if let TableSeat::Player { member, .. } = s {
                member.grace_until_ms = None;
            }
        }
        self.activity_ms = now;
    }

    // The arguments are the protocol fields of `table_action` plus the clock.
    #[allow(clippy::too_many_arguments)]
    pub fn action(
        &mut self,
        player: PlayerId,
        match_id: &str,
        action_id: &str,
        expected_version: u64,
        action: PlayerAction,
        timings: &Timings,
        now: u64,
    ) -> Result<crate::room::ActionAck, RoomError> {
        let seat = self.require(player)?;
        if self.status != TableStatus::Playing {
            return Err(RoomError::WrongState);
        }
        let game = self.game.as_mut().expect("playing table has a game");
        if game.match_id != match_id {
            return Err(RoomError::WrongMatch);
        }
        if let Some((_, version)) = game.recent.iter().find(|(id, _)| id == action_id) {
            return Ok(crate::room::ActionAck {
                state_version: *version,
                duplicate: true,
            });
        }
        if expected_version != game.version {
            return Err(RoomError::Stale);
        }
        if game.t.phase != Phase::InHand || game.t.hand.to_act != Some(seat) {
            return Err(RoomError::NotYourTurn);
        }
        game.t.apply(seat, action)?;
        let bank = bank_of(game, seat, timings);
        let over = now
            .saturating_sub(game.turn_started_ms)
            .saturating_sub(timings.turn_ms);
        game.bank_ms[seat] = bank.saturating_sub(over);
        game.recent
            .push_back((action_id.to_string(), game.version + 1));
        if game.recent.len() > RECENT_ACTIONS {
            game.recent.pop_front();
        }
        self.after_transition(now);
        Ok(crate::room::ActionAck {
            state_version: self.game.as_ref().expect("game").version,
            duplicate: false,
        })
    }

    fn after_transition(&mut self, now: u64) {
        self.activity_ms = now;
        let game = self.game.as_mut().expect("game");
        game.version += 1;
        game.turn_started_ms = now;
        match game.t.phase {
            Phase::InHand => game.hand_over_ms = None,
            Phase::HandOver => game.hand_over_ms = Some(now),
            Phase::MatchOver => {
                game.hand_over_ms = None;
                self.status = TableStatus::Finished;
                self.again.clear();
                let people = self.seats.iter().filter(|s| s.player().is_some()).count();
                self.events.push(TableEvent::Finished {
                    people,
                    bots: self.seats.len() - people,
                });
                for s in &mut self.seats {
                    if let TableSeat::Player { member, .. } = s {
                        member.grace_until_ms = None;
                    }
                }
            }
        }
    }

    /// "Ещё раз": the same line-up plays again once every person agrees.
    pub fn vote_again(
        &mut self,
        player: PlayerId,
        dealer: &mut dyn Dealer,
        now: u64,
    ) -> Result<(), RoomError> {
        self.require(player)?;
        let staying = self
            .seats
            .iter()
            .filter(|s| !matches!(s, TableSeat::Player { gone: true, .. }))
            .count();
        if self.status != TableStatus::Finished || staying < 2 {
            return Err(RoomError::WrongState);
        }
        if !self.again.contains(&player) {
            self.again.push(player);
        }
        self.activity_ms = now;
        if self.members().all(|m| self.again.contains(&m.profile.id)) {
            self.deal_match(dealer, now);
        }
        Ok(())
    }

    pub fn set_connected(
        &mut self,
        player: PlayerId,
        connected: bool,
        timings: &Timings,
        now: u64,
    ) {
        let playing = self.status == TableStatus::Playing;
        let Some(seat) = self.seat_of(player) else {
            return;
        };
        if let TableSeat::Player {
            member,
            gone: false,
        } = &mut self.seats[seat]
        {
            member.connected = connected;
            member.grace_until_ms = (!connected && playing).then_some(now + timings.grace_ms);
        }
    }

    /// Timers: bots' turns, turn timeouts, the pause between hands,
    /// reconnect windows and expiry. Returns whether anything changed.
    pub fn tick(&mut self, dealer: &mut dyn Dealer, timings: &Timings, now: u64) -> bool {
        match self.status {
            TableStatus::Closed => false,
            TableStatus::Lobby | TableStatus::Finished => {
                if now.saturating_sub(self.activity_ms) >= timings.room_ttl_ms {
                    self.close(TableCloseReason::Expired);
                    true
                } else {
                    false
                }
            }
            TableStatus::Playing => self.tick_playing(dealer, timings, now),
        }
    }

    fn tick_playing(&mut self, dealer: &mut dyn Dealer, timings: &Timings, now: u64) -> bool {
        // A person who did not come back in time is out of the match.
        let expired = self.seats.iter().position(|s| {
            s.present()
                .and_then(|m| m.grace_until_ms)
                .is_some_and(|t| now >= t)
        });
        if let Some(seat) = expired {
            let player = self.seats[seat].player().expect("a person");
            let _ = self.leave(player, now);
            return true;
        }

        let game = self.game.as_mut().expect("game");
        match game.t.phase {
            Phase::HandOver => {
                let since = game.hand_over_ms.unwrap_or(now);
                if now < since + game.pause(timings) {
                    return false;
                }
                if game.t.is_final_hand() {
                    game.t.next_hand().expect("hand over");
                } else {
                    game.t.next_hand_dealt(&dealer.deck()).expect("a full deck");
                }
                self.after_transition(now);
                true
            }
            Phase::InHand => {
                let seat = game.t.hand.to_act.expect("someone to act");
                match self.seats[seat] {
                    TableSeat::Bot { opponent } => {
                        if now < game.turn_started_ms + timings.bot_think_ms {
                            return false;
                        }
                        let action =
                            bot::decide_table(&game.t.view(seat), opponent, game.t.decision_seed());
                        game.t
                            .apply(seat, action)
                            .expect("bots choose legal actions");
                        self.after_transition(now);
                        true
                    }
                    _ => {
                        if game.hold_turn {
                            if self.seats[seat].present().is_some_and(|m| m.connected) {
                                let game = self.game.as_mut().expect("game");
                                game.hold_turn = false;
                                game.turn_started_ms = now;
                            }
                            return false;
                        }
                        if now < game.deadline(seat, timings) {
                            return false;
                        }
                        game.bank_ms[seat] = 0;
                        let action = game.t.timeout_action(seat).expect("legal actions on turn");
                        game.t.apply(seat, action).expect("timeout action is legal");
                        self.after_transition(now);
                        true
                    }
                }
            }
            Phase::MatchOver => false,
        }
    }

    /// After a server restart: everyone gets a longer window to come back and
    /// the current turn starts over.
    pub fn restore_after_restart(&mut self, timings: &Timings, now: u64) {
        let playing = self.status == TableStatus::Playing;
        for s in &mut self.seats {
            if let TableSeat::Player {
                member,
                gone: false,
            } = s
            {
                member.connected = false;
                member.grace_until_ms = playing.then_some(now + timings.restart_grace_ms);
            }
        }
        if let Some(game) = self.game.as_mut() {
            game.turn_started_ms = now;
            game.hold_turn = playing;
            if game.hand_over_ms.is_some() {
                game.hand_over_ms = Some(now);
            }
        }
        self.activity_ms = now;
    }

    /// Everything `player` may see of the table right now.
    pub fn view_for(&self, player: PlayerId, timings: &Timings) -> Option<TableRoomView> {
        let me = self.seat_of(player)?;
        let seats = self
            .seats
            .iter()
            .enumerate()
            .map(|(i, s)| match s {
                TableSeat::Empty => SeatView::Empty,
                TableSeat::Bot { opponent } => SeatView::Bot {
                    opponent: *opponent,
                },
                TableSeat::Player { member, gone } => SeatView::Player {
                    name: member.profile.name.clone(),
                    photo: member.profile.photo.clone(),
                    connected: member.connected,
                    reconnect_deadline_ms: member.grace_until_ms,
                    is_host: member.profile.id == self.host,
                    is_me: i == me,
                    gone: *gone,
                    again: self.again.contains(&member.profile.id),
                },
            })
            .collect();
        let game = self.game.as_ref().map(|g| {
            let playing = self.status == TableStatus::Playing;
            let clock = (playing && g.t.phase == Phase::InHand).then(|| {
                let seat = g.t.hand.to_act.expect("someone to act");
                let bot = matches!(self.seats[seat], TableSeat::Bot { .. });
                TableClock {
                    seat,
                    my_turn: seat == me,
                    deadline_ms: (!bot).then(|| g.deadline(seat, timings)),
                    turn_ms: timings.turn_ms,
                    bank_ms_me: bank_of(g, me, timings),
                }
            });
            TableGameView {
                match_id: g.match_id.clone(),
                state_version: g.version,
                view: g.t.view(me),
                clock,
                next_hand_at_ms: playing
                    .then(|| g.hand_over_ms.map(|t| t + g.pause(timings)))
                    .flatten(),
            }
        });
        Some(TableRoomView {
            table_id: self.id.clone(),
            code: crate::ids::room_code(&self.id),
            status: self.status,
            close_reason: self.close_reason,
            you_are_host: player == self.host,
            my_seat: me,
            expires_at_ms: matches!(self.status, TableStatus::Lobby | TableStatus::Finished)
                .then_some(self.activity_ms + timings.room_ttl_ms),
            seats,
            game,
            again: AgainView {
                votes: self.again.len(),
                needed: self.members().count(),
                mine: self.again.contains(&player),
            },
        })
    }

    pub fn preview(&self, player: PlayerId) -> TablePreview {
        let host = self.seats.iter().find_map(|s| match s {
            TableSeat::Player { member, .. } if member.profile.id == self.host => Some(member),
            _ => None,
        });
        TablePreview {
            table_id: self.id.clone(),
            host_name: host.map(|m| m.profile.name.clone()).unwrap_or_default(),
            host_photo: host.and_then(|m| m.profile.photo.clone()),
            joinable: self.joinable(),
            already_member: self.seat_of(player).is_some(),
            status: self.status,
            seats: self.seats.len(),
            taken: self
                .seats
                .iter()
                .filter(|s| **s != TableSeat::Empty)
                .count(),
        }
    }
}

fn bank_of(game: &TableGame, seat: usize, timings: &Timings) -> u64 {
    match game.bank_ms[seat] {
        u64::MAX => timings.bank_ms,
        b => b,
    }
}

/// A uniformly random seat from coin flips (bias below 2^-16).
fn random_seat(dealer: &mut dyn Dealer, n: usize) -> usize {
    let mut x = 0usize;
    for _ in 0..24 {
        x = x << 1 | dealer.coin() as usize;
    }
    x % n
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SeatView {
    Empty,
    Bot {
        opponent: Opponent,
    },
    Player {
        name: String,
        photo: Option<String>,
        connected: bool,
        reconnect_deadline_ms: Option<u64>,
        is_host: bool,
        is_me: bool,
        gone: bool,
        /// Voted for another match.
        again: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableClock {
    /// Who is acting.
    pub seat: usize,
    pub my_turn: bool,
    /// When a person's time (turn plus bank) runs out; none for a bot.
    pub deadline_ms: Option<u64>,
    pub turn_ms: u64,
    pub bank_ms_me: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableGameView {
    pub match_id: String,
    pub state_version: u64,
    /// The engine's view for my seat (`view.me`).
    pub view: TableView,
    pub clock: Option<TableClock>,
    pub next_hand_at_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AgainView {
    pub votes: usize,
    pub needed: usize,
    pub mine: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableRoomView {
    pub table_id: String,
    pub code: String,
    pub status: TableStatus,
    pub close_reason: Option<TableCloseReason>,
    pub you_are_host: bool,
    pub my_seat: usize,
    pub expires_at_ms: Option<u64>,
    pub seats: Vec<SeatView>,
    pub game: Option<TableGameView>,
    pub again: AgainView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TablePreview {
    pub table_id: String,
    pub host_name: String,
    pub host_photo: Option<String>,
    pub joinable: bool,
    pub already_member: bool,
    pub status: TableStatus,
    pub seats: usize,
    pub taken: usize,
}

#[cfg(test)]
#[path = "table_room_tests.rs"]
mod tests;
