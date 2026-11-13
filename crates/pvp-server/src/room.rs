//! A private duel room: the state machine from the 2.0 spec, with no I/O.
//!
//! ```text
//! Waiting ──guest joins──▶ Ready ──both ready──▶ Playing ──match over──▶ Finished
//!    ▲                       │                                  ▲            │
//!    └────guest leaves───────┘          both accept a rematch ──┼────────────┘
//! Waiting/Ready/Finished ──TTL, cancel or leave──▶ Closed        │
//! Playing ──timeout / disconnect / surrender──▶ Finished (forfeit)
//! ```
//!
//! The host always sits in engine seat `Human`, the guest in seat `Bot` (the
//! engine numbers seats 0 and 1; see `poker_core::perspective`). Time is
//! passed in as `now` (milliseconds), so every timer is testable.
//!
//! Two-player assumptions live here: `host`/`guest`, `[u64; 2]` time banks
//! and the head-to-head score. A 3–6 seat table would replace them with a
//! seat list; see the README.

use crate::player::PlayerId;
use poker_core::card::Card;
use poker_core::{EngineError, Match, MatchOutcome, Phase, PlayerAction, PlayerView, Seat};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Server-controlled timings (milliseconds).
#[derive(Clone, Copy, Debug)]
pub struct Timings {
    /// Time for each decision before the time bank starts running.
    pub turn_ms: u64,
    /// Extra time per player for the whole match.
    pub bank_ms: u64,
    /// How long a dropped player may take to come back mid-match.
    pub grace_ms: u64,
    /// Grace for everyone after a server restart.
    pub restart_grace_ms: u64,
    /// Idle lifetime of a room that has not started (or has finished).
    pub room_ttl_ms: u64,
    /// Pause on the hand result before the next hand is dealt.
    pub hand_pause_ms: u64,
    /// Longer pause after an all-in, while the board is revealed.
    pub runout_pause_ms: u64,
    /// How long a bot at a table "thinks", so people can follow the play.
    pub bot_think_ms: u64,
}

impl Default for Timings {
    fn default() -> Timings {
        Timings {
            turn_ms: 20_000,
            bank_ms: 30_000,
            grace_ms: 60_000,
            restart_grace_ms: 180_000,
            room_ttl_ms: 15 * 60_000,
            hand_pause_ms: 4_000,
            runout_pause_ms: 6_500,
            bot_think_ms: 900,
        }
    }
}

/// Source of everything random: decks, the first button, match ids.
pub trait Dealer {
    fn deck(&mut self) -> Vec<Card>;
    fn coin(&mut self) -> bool;
    fn match_id(&mut self) -> String;
}

/// Production dealer: the OS CSPRNG.
pub struct OsDealer;

impl Dealer for OsDealer {
    fn deck(&mut self) -> Vec<Card> {
        crate::ids::shuffled_deck()
    }
    fn coin(&mut self) -> bool {
        crate::ids::coin()
    }
    fn match_id(&mut self) -> String {
        crate::ids::match_id()
    }
}

/// A VK user as shown to the other player. The name and photo come from the
/// client (VKWebAppGetUserInfo) and are display-only; `id` is verified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Verified on sign-in (VK, Telegram) or derived from a guest key.
    /// Stored as `player`: rooms saved before other platforms still load.
    #[serde(rename = "player")]
    pub id: PlayerId,
    pub name: String,
    pub photo: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub profile: Profile,
    pub ready: bool,
    pub connected: bool,
    /// Set while a player who left mid-match may still come back.
    pub grace_until_ms: Option<u64>,
}

impl Member {
    pub(crate) fn new(profile: Profile) -> Member {
        Member {
            profile,
            ready: false,
            connected: true,
            grace_until_ms: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Host,
    Guest,
}

impl Slot {
    pub fn seat(self) -> Seat {
        match self {
            Slot::Host => Seat::Human,
            Slot::Guest => Seat::Bot,
        }
    }

    pub fn other(self) -> Slot {
        match self {
            Slot::Host => Slot::Guest,
            Slot::Guest => Slot::Host,
        }
    }

    fn idx(self) -> usize {
        self.seat().idx()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Waiting,
    Ready,
    Playing,
    Finished,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    CancelledByHost,
    Expired,
    /// Someone left after the match; `left` says who.
    Left,
    /// Both players were gone mid-match; no result is recorded.
    Aborted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    #[serde(default)]
    pub started_ms: u64,
    #[serde(default)]
    pub hand_finished_ms: Vec<u64>,
    #[serde(default)]
    pub finished_ms: Option<u64>,
    pub match_id: String,
    pub m: Match,
    /// Bumped on every accepted transition; clients send it back with actions.
    pub version: u64,
    pub turn_started_ms: u64,
    /// Remaining time bank, `[host, guest]`.
    pub bank_ms: [u64; 2],
    /// When the current hand ended (the result pause runs from here).
    pub hand_over_ms: Option<u64>,
    /// Recently applied action ids and the version they produced.
    recent: VecDeque<(String, u64)>,
    /// After a server restart the turn clock waits for the acting player
    /// to reconnect.
    #[serde(default)]
    hold_turn: bool,
}

/// Head-to-head between the two people in the room, `host`/`guest` wins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Score {
    pub host: u32,
    pub guest: u32,
    pub draws: u32,
}

/// Something the hub must persist or report beyond the room itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    MatchFinished {
        host: PlayerId,
        guest: PlayerId,
        outcome_for_host: MatchOutcome,
        forfeit: Option<Slot>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub created_ms: u64,
    pub activity_ms: u64,
    pub status: Status,
    pub close_reason: Option<CloseReason>,
    pub left: Option<Slot>,
    pub host: Member,
    pub guest: Option<Member>,
    pub game: Option<Game>,
    pub rematch_by: Option<Slot>,
    pub rematch_declined_by: Option<Slot>,
    pub score: Score,
    /// The client's `request_id` for `create_room`, for idempotent retries.
    pub create_request: String,
    #[serde(skip)]
    events: Vec<Event>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomError {
    NotFound,
    Full,
    Closed,
    NotMember,
    WrongState,
    NotYourTurn,
    Stale,
    WrongMatch,
    Illegal,
    InvalidAmount,
    OpponentAway,
    TooManyRooms,
}

impl RoomError {
    pub fn code(self) -> &'static str {
        match self {
            RoomError::NotFound => "room_not_found",
            RoomError::Full => "room_full",
            RoomError::Closed => "room_closed",
            RoomError::NotMember => "not_member",
            RoomError::WrongState => "wrong_state",
            RoomError::NotYourTurn => "not_your_turn",
            RoomError::Stale => "stale_state",
            RoomError::WrongMatch => "wrong_match",
            RoomError::Illegal => "illegal_action",
            RoomError::InvalidAmount => "invalid_amount",
            RoomError::OpponentAway => "opponent_away",
            RoomError::TooManyRooms => "too_many_rooms",
        }
    }

    /// Short Russian text for the player.
    pub fn message(self) -> &'static str {
        match self {
            RoomError::NotFound => "Комната не найдена",
            RoomError::Full => "Комната уже занята",
            RoomError::Closed => "Приглашение больше не действует",
            RoomError::NotMember => "Вы не участник этой комнаты",
            RoomError::WrongState => "Сейчас это действие недоступно",
            RoomError::NotYourTurn => "Сейчас ход соперника",
            RoomError::Stale => "Стол обновился, повторите ход",
            RoomError::WrongMatch => "Этот матч уже закончился",
            RoomError::Illegal => "Такой ход сейчас невозможен",
            RoomError::InvalidAmount => "Недопустимый размер ставки",
            RoomError::OpponentAway => "Друг не в сети",
            RoomError::TooManyRooms => "Слишком много открытых комнат",
        }
    }
}

impl From<EngineError> for RoomError {
    fn from(e: EngineError) -> RoomError {
        match e {
            EngineError::NotYourTurn => RoomError::NotYourTurn,
            EngineError::InvalidAmount { .. } => RoomError::InvalidAmount,
            _ => RoomError::Illegal,
        }
    }
}

/// Result of a `player_action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionAck {
    pub state_version: u64,
    /// The same `action_id` was already applied; nothing changed.
    pub duplicate: bool,
}

const RECENT_ACTIONS: usize = 64;

impl Room {
    pub fn new(id: String, host: Profile, create_request: String, now: u64) -> Room {
        Room {
            id,
            created_ms: now,
            activity_ms: now,
            status: Status::Waiting,
            close_reason: None,
            left: None,
            host: Member::new(host),
            guest: None,
            game: None,
            rematch_by: None,
            rematch_declined_by: None,
            score: Score::default(),
            create_request,
            events: Vec::new(),
        }
    }

    pub fn slot_of(&self, player: PlayerId) -> Option<Slot> {
        if self.host.profile.id == player {
            Some(Slot::Host)
        } else if self.guest.as_ref().is_some_and(|g| g.profile.id == player) {
            Some(Slot::Guest)
        } else {
            None
        }
    }

    fn member(&self, slot: Slot) -> Option<&Member> {
        match slot {
            Slot::Host => Some(&self.host),
            Slot::Guest => self.guest.as_ref(),
        }
    }

    fn member_mut(&mut self, slot: Slot) -> Option<&mut Member> {
        match slot {
            Slot::Host => Some(&mut self.host),
            Slot::Guest => self.guest.as_mut(),
        }
    }

    fn require(&self, player: PlayerId) -> Result<Slot, RoomError> {
        self.slot_of(player).ok_or(RoomError::NotMember)
    }

    pub fn members(&self) -> impl Iterator<Item = &Member> {
        std::iter::once(&self.host).chain(self.guest.as_ref())
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn is_open(&self) -> bool {
        self.status != Status::Closed
    }

    fn close(&mut self, reason: CloseReason) {
        self.status = Status::Closed;
        self.close_reason = Some(reason);
        self.rematch_by = None;
    }

    /// Enter the room. The host coming back (another device, a reload) gets
    /// the host seat again; it never takes the guest's place.
    pub fn join(&mut self, profile: Profile, now: u64) -> Result<Slot, RoomError> {
        if let Some(slot) = self.slot_of(profile.id) {
            if self.status == Status::Closed {
                return Err(RoomError::Closed);
            }
            let m = self.member_mut(slot).expect("member exists");
            m.profile = profile;
            return Ok(slot);
        }
        match self.status {
            Status::Closed => Err(RoomError::Closed),
            Status::Waiting => {
                self.guest = Some(Member::new(profile));
                self.status = Status::Ready;
                self.activity_ms = now;
                Ok(Slot::Guest)
            }
            _ => Err(RoomError::Full),
        }
    }

    /// "Отменить комнату" (host) or "Выйти" (guest) before or after a match.
    pub fn leave(&mut self, player: PlayerId, now: u64) -> Result<(), RoomError> {
        let slot = self.require(player)?;
        self.activity_ms = now;
        match (self.status, slot) {
            (Status::Closed, _) => Ok(()),
            (Status::Playing, _) => Err(RoomError::WrongState),
            (Status::Waiting | Status::Ready, Slot::Host) => {
                self.close(CloseReason::CancelledByHost);
                Ok(())
            }
            (Status::Waiting | Status::Ready, Slot::Guest) => {
                self.guest = None;
                self.host.ready = false;
                self.status = Status::Waiting;
                Ok(())
            }
            (Status::Finished, _) => {
                self.left = Some(slot);
                self.close(CloseReason::Left);
                Ok(())
            }
        }
    }

    /// "Готов". When both are ready the match starts, exactly once.
    pub fn set_ready(
        &mut self,
        player: PlayerId,
        ready: bool,
        dealer: &mut dyn Dealer,
        now: u64,
    ) -> Result<(), RoomError> {
        let slot = self.require(player)?;
        if self.status != Status::Ready {
            return Err(RoomError::WrongState);
        }
        self.member_mut(slot).expect("member").ready = ready;
        self.activity_ms = now;
        if self.host.ready && self.guest.as_ref().is_some_and(|g| g.ready) {
            self.start_game(dealer, now);
        }
        Ok(())
    }

    fn start_game(&mut self, dealer: &mut dyn Dealer, now: u64) {
        let button = if dealer.coin() {
            Seat::Human
        } else {
            Seat::Bot
        };
        let m = Match::duel(dealer.match_id(), button, &dealer.deck()).expect("a full deck");
        self.game = Some(Game {
            started_ms: now,
            hand_finished_ms: Vec::new(),
            finished_ms: None,
            match_id: m.id.clone(),
            m,
            version: 1,
            turn_started_ms: now,
            bank_ms: [BANK_UNSET; 2],
            hand_over_ms: None,
            recent: VecDeque::new(),
            hold_turn: false,
        });
        self.status = Status::Playing;
        self.rematch_by = None;
        self.rematch_declined_by = None;
        for m in [Some(&mut self.host), self.guest.as_mut()]
            .into_iter()
            .flatten()
        {
            m.ready = false;
            m.grace_until_ms = None;
        }
        self.activity_ms = now;
    }

    /// A betting decision. Retries with the same `action_id` are answered
    /// without applying anything twice; a stale `expected_version` is refused.
    // The arguments are the protocol fields of `player_action` plus the clock.
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
    ) -> Result<ActionAck, RoomError> {
        let slot = self.require(player)?;
        if self.status != Status::Playing {
            return Err(RoomError::WrongState);
        }
        let game = self.game.as_mut().expect("playing room has a game");
        if game.match_id != match_id {
            return Err(RoomError::WrongMatch);
        }
        if let Some((_, version)) = game.recent.iter().find(|(id, _)| id == action_id) {
            return Ok(ActionAck {
                state_version: *version,
                duplicate: true,
            });
        }
        if expected_version != game.version {
            return Err(RoomError::Stale);
        }
        let seat = slot.seat();
        if game.m.phase != Phase::InHand || game.m.hand.to_act != Some(seat) {
            return Err(RoomError::NotYourTurn);
        }
        game.m.apply(seat, action)?;
        let bank = game.bank(slot, timings);
        let over = now
            .saturating_sub(game.turn_started_ms)
            .saturating_sub(timings.turn_ms);
        game.bank_ms[slot.idx()] = bank.saturating_sub(over);
        game.recent
            .push_back((action_id.to_string(), game.version + 1));
        if game.recent.len() > RECENT_ACTIONS {
            game.recent.pop_front();
        }
        self.after_transition(now);
        let version = self.game.as_ref().expect("game").version;
        Ok(ActionAck {
            state_version: version,
            duplicate: false,
        })
    }

    /// Bookkeeping after any engine transition.
    fn after_transition(&mut self, now: u64) {
        self.activity_ms = now;
        let game = self.game.as_mut().expect("game");
        game.hand_finished_ms.resize(game.m.history.len(), now);
        if game.m.phase == Phase::MatchOver && game.finished_ms.is_none() {
            game.finished_ms = Some(now);
            if game.started_ms > 0 && now.saturating_sub(game.started_ms) < 10_000 {
                tracing::info!(event = "daily_fast_match", match_id = %crate::ids::log_tag(&game.match_id), elapsed_ms = now.saturating_sub(game.started_ms));
            }
        }
        game.version += 1;
        game.turn_started_ms = now;
        match game.m.phase {
            Phase::InHand => game.hand_over_ms = None,
            Phase::HandOver => game.hand_over_ms = Some(now),
            Phase::MatchOver => self.finish(),
        }
    }

    fn finish(&mut self) {
        self.status = Status::Finished;
        self.rematch_by = None;
        let game = self.game.as_mut().expect("game");
        game.hand_over_ms = None;
        let result = game.m.result.clone().expect("finished match has a result");
        match result.outcome {
            MatchOutcome::Win => self.score.host += 1,
            MatchOutcome::Loss => self.score.guest += 1,
            MatchOutcome::Draw => self.score.draws += 1,
        }
        for m in [Some(&mut self.host), self.guest.as_mut()]
            .into_iter()
            .flatten()
        {
            m.grace_until_ms = None;
        }
        self.events.push(Event::MatchFinished {
            host: self.host.profile.id,
            guest: self
                .guest
                .as_ref()
                .expect("a finished duel has a guest")
                .profile
                .id,
            outcome_for_host: result.outcome,
            forfeit: result.forfeit.map(|s| {
                if s == Seat::Human {
                    Slot::Host
                } else {
                    Slot::Guest
                }
            }),
        });
    }

    /// "Сдаться и выйти": an immediate technical defeat.
    pub fn surrender(&mut self, player: PlayerId, now: u64) -> Result<(), RoomError> {
        let slot = self.require(player)?;
        if self.status != Status::Playing {
            return Err(RoomError::WrongState);
        }
        self.game.as_mut().expect("game").m.forfeit(slot.seat())?;
        self.after_transition(now);
        Ok(())
    }

    pub fn set_connected(
        &mut self,
        player: PlayerId,
        connected: bool,
        timings: &Timings,
        now: u64,
    ) {
        let playing = self.status == Status::Playing;
        let Some(slot) = self.slot_of(player) else {
            return;
        };
        let m = self.member_mut(slot).expect("member");
        m.connected = connected;
        m.grace_until_ms = (!connected && playing).then_some(now + timings.grace_ms);
    }

    /// "Предложить реванш", or accept one the opponent already proposed.
    pub fn request_rematch(
        &mut self,
        player: PlayerId,
        dealer: &mut dyn Dealer,
        now: u64,
    ) -> Result<(), RoomError> {
        let slot = self.require(player)?;
        if self.status != Status::Finished {
            return Err(RoomError::WrongState);
        }
        if !self.member(slot.other()).is_some_and(|m| m.connected) {
            return Err(RoomError::OpponentAway);
        }
        self.activity_ms = now;
        self.rematch_declined_by = None;
        if self.rematch_by == Some(slot.other()) {
            self.start_game(dealer, now);
        } else {
            self.rematch_by = Some(slot);
        }
        Ok(())
    }

    pub fn accept_rematch(
        &mut self,
        player: PlayerId,
        dealer: &mut dyn Dealer,
        now: u64,
    ) -> Result<(), RoomError> {
        let slot = self.require(player)?;
        if self.status != Status::Finished || self.rematch_by != Some(slot.other()) {
            return Err(RoomError::WrongState);
        }
        self.request_rematch(player, dealer, now)
    }

    pub fn decline_rematch(&mut self, player: PlayerId, now: u64) -> Result<(), RoomError> {
        let slot = self.require(player)?;
        if self.status != Status::Finished || self.rematch_by != Some(slot.other()) {
            return Err(RoomError::WrongState);
        }
        self.rematch_by = None;
        self.rematch_declined_by = Some(slot);
        self.activity_ms = now;
        Ok(())
    }

    /// Advances timers: turn timeouts, the pause between hands, reconnect
    /// grace and room expiry. Returns whether anything changed.
    pub fn tick(&mut self, dealer: &mut dyn Dealer, timings: &Timings, now: u64) -> bool {
        match self.status {
            Status::Closed => false,
            Status::Waiting | Status::Ready | Status::Finished => {
                if now.saturating_sub(self.activity_ms) >= timings.room_ttl_ms {
                    self.close(CloseReason::Expired);
                    true
                } else {
                    false
                }
            }
            Status::Playing => self.tick_playing(dealer, timings, now),
        }
    }

    fn tick_playing(&mut self, dealer: &mut dyn Dealer, timings: &Timings, now: u64) -> bool {
        // A player whose reconnect window ran out loses; if both are gone
        // the match is called off without a result.
        for slot in [Slot::Host, Slot::Guest] {
            let expired = self
                .member(slot)
                .and_then(|m| m.grace_until_ms)
                .is_some_and(|t| now >= t);
            if !expired {
                continue;
            }
            if self.member(slot.other()).is_some_and(|m| m.connected) {
                self.game
                    .as_mut()
                    .expect("game")
                    .m
                    .forfeit(slot.seat())
                    .expect("match in progress");
                self.after_transition(now);
            } else {
                self.close(CloseReason::Aborted);
            }
            return true;
        }

        let game = self.game.as_mut().expect("game");
        match game.m.phase {
            Phase::HandOver => {
                let runout = game
                    .m
                    .hand
                    .outcome
                    .as_ref()
                    .is_some_and(|o| o.runout_from.is_some());
                let pause = if runout {
                    timings.runout_pause_ms
                } else {
                    timings.hand_pause_ms
                };
                let since = game.hand_over_ms.unwrap_or(now);
                if now < since + pause {
                    return false;
                }
                if game.m.is_final_hand() {
                    game.m.next_hand().expect("hand over");
                } else {
                    game.m.next_hand_dealt(&dealer.deck()).expect("a full deck");
                }
                self.after_transition(now);
                true
            }
            Phase::InHand => {
                let seat = game.m.hand.to_act.expect("someone to act");
                let slot = if seat == Seat::Human {
                    Slot::Host
                } else {
                    Slot::Guest
                };
                if game.hold_turn {
                    let back = match slot {
                        Slot::Host => self.host.connected,
                        Slot::Guest => self.guest.as_ref().is_some_and(|g| g.connected),
                    };
                    if back {
                        game.hold_turn = false;
                        game.turn_started_ms = now;
                    }
                    return false;
                }
                let deadline = game.deadline(slot, timings);
                if now < deadline {
                    return false;
                }
                game.bank_ms[slot.idx()] = 0;
                let action = game.m.timeout_action(seat).expect("legal actions on turn");
                game.m.apply(seat, action).expect("timeout action is legal");
                self.after_transition(now);
                true
            }
            Phase::MatchOver => false,
        }
    }

    /// After a server restart nobody is connected yet: everyone gets a longer
    /// window to come back, and the current turn starts over, so downtime
    /// never costs anyone the match.
    pub fn restore_after_restart(&mut self, timings: &Timings, now: u64) {
        let playing = self.status == Status::Playing;
        for m in [Some(&mut self.host), self.guest.as_mut()]
            .into_iter()
            .flatten()
        {
            m.connected = false;
            m.grace_until_ms = playing.then_some(now + timings.restart_grace_ms);
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

    /// Everything `player` may see of the room right now.
    pub fn view_for(&self, player: PlayerId, timings: &Timings) -> Option<RoomView> {
        let slot = self.slot_of(player)?;
        let me = self.member(slot).expect("member");
        let opponent = self.member(slot.other());
        let game = self.game.as_ref().map(|g| {
            let view = g.m.view_from(slot.seat());
            let clock = (self.status == Status::Playing && g.m.phase == Phase::InHand).then(|| {
                let to_act = g.m.hand.to_act.expect("someone to act");
                let acting = if to_act == Seat::Human {
                    Slot::Host
                } else {
                    Slot::Guest
                };
                Clock {
                    my_turn: acting == slot,
                    deadline_ms: g.deadline(acting, timings),
                    turn_ms: timings.turn_ms,
                    bank_ms_me: g.bank(slot, timings),
                    bank_ms_opponent: g.bank(slot.other(), timings),
                }
            });
            GameView {
                match_id: g.match_id.clone(),
                state_version: g.version,
                next_hand_at_ms: (self.status == Status::Playing)
                    .then(|| {
                        let runout =
                            g.m.hand
                                .outcome
                                .as_ref()
                                .is_some_and(|o| o.runout_from.is_some());
                        let pause = if runout {
                            timings.runout_pause_ms
                        } else {
                            timings.hand_pause_ms
                        };
                        g.hand_over_ms.map(|t| t + pause)
                    })
                    .flatten(),
                clock,
                view,
            }
        });
        let (mine, theirs) = match slot {
            Slot::Host => (self.score.host, self.score.guest),
            Slot::Guest => (self.score.guest, self.score.host),
        };
        Some(RoomView {
            room_id: self.id.clone(),
            code: crate::ids::room_code(&self.id),
            status: self.status,
            close_reason: self.close_reason,
            left_by_me: self.left.map(|s| s == slot),
            you_are_host: slot == Slot::Host,
            expires_at_ms: matches!(
                self.status,
                Status::Waiting | Status::Ready | Status::Finished
            )
            .then_some(self.activity_ms + timings.room_ttl_ms),
            me: SlotView::of(me),
            opponent: opponent.map(SlotView::of),
            game,
            rematch: RematchView {
                requested_by_me: self.rematch_by == Some(slot),
                requested_by_opponent: self.rematch_by == Some(slot.other()),
                declined_by_opponent: self.rematch_declined_by == Some(slot.other()),
            },
            score: ScoreView {
                me: mine,
                opponent: theirs,
                draws: self.score.draws,
            },
        })
    }

    /// What someone holding the invite link sees before joining.
    pub fn preview(&self, player: PlayerId) -> RoomPreview {
        RoomPreview {
            room_id: self.id.clone(),
            host_name: self.host.profile.name.clone(),
            host_photo: self.host.profile.photo.clone(),
            joinable: self.status == Status::Waiting,
            already_member: self.slot_of(player).is_some(),
            status: self.status,
        }
    }
}

/// Marks a time bank that has not been touched yet (the configured amount).
const BANK_UNSET: u64 = u64::MAX;

impl Game {
    fn bank(&self, slot: Slot, timings: &Timings) -> u64 {
        match self.bank_ms[slot.idx()] {
            BANK_UNSET => timings.bank_ms,
            b => b,
        }
    }

    fn deadline(&self, slot: Slot, timings: &Timings) -> u64 {
        self.turn_started_ms + timings.turn_ms + self.bank(slot, timings)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SlotView {
    pub name: String,
    pub photo: Option<String>,
    pub ready: bool,
    pub connected: bool,
    /// While a dropped player may still come back.
    pub reconnect_deadline_ms: Option<u64>,
}

impl SlotView {
    fn of(m: &Member) -> SlotView {
        SlotView {
            name: m.profile.name.clone(),
            photo: m.profile.photo.clone(),
            ready: m.ready,
            connected: m.connected,
            reconnect_deadline_ms: m.grace_until_ms,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Clock {
    pub my_turn: bool,
    /// When the acting player's time (turn plus bank) runs out.
    pub deadline_ms: u64,
    pub turn_ms: u64,
    pub bank_ms_me: u64,
    pub bank_ms_opponent: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GameView {
    pub match_id: String,
    pub state_version: u64,
    /// The engine's view for this player, shown as the bottom seat.
    pub view: PlayerView,
    pub clock: Option<Clock>,
    /// When the next hand will be dealt, during the result pause.
    pub next_hand_at_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RematchView {
    pub requested_by_me: bool,
    pub requested_by_opponent: bool,
    pub declined_by_opponent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ScoreView {
    pub me: u32,
    pub opponent: u32,
    pub draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RoomView {
    pub room_id: String,
    /// Six digits to dictate instead of the link (`ids::room_code`).
    pub code: String,
    pub status: Status,
    pub close_reason: Option<CloseReason>,
    pub left_by_me: Option<bool>,
    pub you_are_host: bool,
    pub expires_at_ms: Option<u64>,
    pub me: SlotView,
    pub opponent: Option<SlotView>,
    pub game: Option<GameView>,
    pub rematch: RematchView,
    pub score: ScoreView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RoomPreview {
    pub room_id: String,
    pub host_name: String,
    pub host_photo: Option<String>,
    pub joinable: bool,
    pub already_member: bool,
    pub status: Status,
}

#[cfg(test)]
#[path = "room_tests.rs"]
mod tests;
