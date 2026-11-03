//! Heads-up No-Limit Texas Hold'em match engine ("Blitz8").
//!
//! The engine is the single source of truth: it owns stacks, pot, legal
//! actions and outcomes. Clients only ever receive a [`PlayerView`] for one
//! seat and send back a [`PlayerAction`].
//!
//! Chip invariant: `stacks + bets + pot == TOTAL_CHIPS` after every transition.

use crate::card::{Card, cards as parse_cards, full_deck};
use crate::eval::{BestHand, Category, HandValue, best_hand};
use crate::history::HandRecord;
use crate::rng::{Rng, mix};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const SCHEMA_VERSION: u32 = 1;
pub const STARTING_STACK: u32 = 600;
pub const TOTAL_CHIPS: u32 = 2 * STARTING_STACK;
pub const HANDS_PER_MATCH: u8 = 8;

/// Hole cards for seat i are `deck[i]` and `deck[i + 2]`; the board is `deck[4..9]`.
const DEALT_CARDS: usize = 9;
const TUTORIAL_DECK: &str = "As Qd Kc 9c Ad 7h 2c 5s Kh";

/// Blinds (small, big) for a 1-based hand number.
pub fn blinds(hand_number: u8) -> (u32, u32) {
    match hand_number {
        0..=2 => (10, 20),
        3..=4 => (15, 30),
        5..=6 => (20, 40),
        _ => (30, 60),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    Human,
    Bot,
}

impl Seat {
    pub const BOTH: [Seat; 2] = [Seat::Human, Seat::Bot];

    pub fn other(self) -> Seat {
        match self {
            Seat::Human => Seat::Bot,
            Seat::Bot => Seat::Human,
        }
    }

    pub fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
}

impl Street {
    pub fn board_len(self) -> usize {
        match self {
            Street::Preflop => 0,
            Street::Flop => 3,
            Street::Turn => 4,
            Street::River => 5,
        }
    }

    pub fn next(self) -> Street {
        match self {
            Street::Preflop => Street::Flop,
            Street::Flop => Street::Turn,
            Street::Turn | Street::River => Street::River,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    Standard,
    /// One deterministic, clearly labelled training hand; never counted in stats.
    Tutorial,
}

/// Which bot the human plays against. How each one plays lives in the `bot`
/// crate; the engine only records the choice so a save resumes against the
/// same opponent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Opponent {
    /// Balanced; the default and the only opponent before 1.2.
    #[default]
    Alex,
    /// Beginner: loose, passive, misreads hands.
    Novice,
    /// Plays few hands and rarely bluffs.
    Tight,
    /// Calls a lot, rarely raises.
    Station,
    /// Loose-aggressive: bets, raises and bluffs often.
    Aggressor,
}

impl Opponent {
    pub const ALL: [Opponent; 5] = [
        Opponent::Novice,
        Opponent::Station,
        Opponent::Tight,
        Opponent::Alex,
        Opponent::Aggressor,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Waiting for `to_act`.
    InHand,
    /// Hand finished; outcome is visible until the client calls [`Match::next_hand`].
    HandOver,
    MatchOver,
}

/// What a client may send. Bets and raises are expressed as the total
/// street bet the player wants to reach ("raise to"), not the increment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerAction {
    Fold,
    Check,
    Call,
    RaiseTo { amount: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    SmallBlind,
    BigBlind,
    Fold,
    Check,
    Call,
    Bet,
    Raise,
    /// Uncalled part of a bet returned to its owner.
    Return,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub seat: Seat,
    pub street: Street,
    pub kind: ActionKind,
    /// Chips moved by this action.
    pub amount: u32,
    /// The player's total bet on this street after the action.
    pub to: u32,
    pub all_in: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetKind {
    HalfPot,
    Pot,
    AllIn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub kind: PresetKind,
    pub to: u32,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaiseKind {
    Bet,
    Raise,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaiseOption {
    pub kind: RaiseKind,
    pub min_to: u32,
    pub max_to: u32,
    /// Suggested slider step; any integer in `min_to..=max_to` is legal.
    pub step: u32,
    pub presets: Vec<Preset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegalActions {
    pub fold: bool,
    pub check: bool,
    /// Chips needed to call (capped by the stack), if calling is possible.
    pub call: Option<u32>,
    pub call_is_all_in: bool,
    pub raise: Option<RaiseOption>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShownHand {
    pub hole: [Card; 2],
    pub category: Category,
    pub best: [Card; 5],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOutcome {
    /// `None` means the pot was split.
    pub winner: Option<Seat>,
    pub by_fold: bool,
    /// Street on which the hand ended (river for any showdown).
    pub ended_on: Street,
    pub pot: u32,
    pub awarded: [u32; 2],
    /// Stack change over the whole hand.
    pub net: [i32; 2],
    /// Present only when cards were shown down.
    pub showdown: Option<[ShownHand; 2]>,
    /// Board length when an all-in runout started, so the client can reveal
    /// the remaining streets one by one.
    pub runout_from: Option<usize>,
}

/// Internal record of a finished hand. It holds both players' hole cards, so
/// it never leaves the engine as is: clients get [`HandRecord`]s projected for
/// one seat (see `history.rs`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandSummary {
    pub number: u8,
    pub button: Seat,
    pub sb: u32,
    pub bb: u32,
    /// Stacks before the blinds. Absent in 1.0 saves; restored by [`Match::upgrade`].
    #[serde(default)]
    pub start_stacks: [u32; 2],
    /// `[human, bot]` hole cards. Absent in 1.0 saves.
    #[serde(default)]
    pub holes: Option<[[Card; 2]; 2]>,
    /// Only the cards that were actually dealt face up.
    pub board: Vec<Card>,
    pub actions: Vec<ActionRecord>,
    pub outcome: HandOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchOutcome {
    Win,
    Loss,
    Draw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BestShown {
    pub category: Category,
    pub cards: [Card; 5],
    value: HandValue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchResult {
    /// From the human's point of view.
    pub outcome: MatchOutcome,
    pub final_stacks: [u32; 2],
    pub hands_played: u8,
    pub hands_won: [u8; 2],
    pub ended_early: bool,
    /// Best hand the human made at a showdown during the match.
    pub best_hand: Option<BestShown>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    WrongPhase,
    NotYourTurn,
    IllegalAction,
    InvalidAmount { min: u32, max: u32 },
    IncompatibleSave { found: u32 },
    CorruptSave(&'static str),
}

impl EngineError {
    pub fn code(&self) -> &'static str {
        match self {
            EngineError::WrongPhase => "wrong_phase",
            EngineError::NotYourTurn => "not_your_turn",
            EngineError::IllegalAction => "illegal_action",
            EngineError::InvalidAmount { .. } => "invalid_amount",
            EngineError::IncompatibleSave { .. } => "incompatible_save",
            EngineError::CorruptSave(_) => "corrupt_save",
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::InvalidAmount { min, max } => {
                write!(f, "amount must be within {min}..={max}")
            }
            EngineError::IncompatibleSave { found } => {
                write!(
                    f,
                    "save schema {found} is not supported (expected {SCHEMA_VERSION})"
                )
            }
            EngineError::CorruptSave(why) => write!(f, "corrupt save: {why}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for EngineError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hand {
    pub number: u8,
    pub button: Seat,
    pub sb: u32,
    pub bb: u32,
    deck: Vec<Card>,
    pub street: Street,
    board_len: usize,
    pub bets: [u32; 2],
    /// Chips collected from finished betting rounds.
    pub pot: u32,
    pub(crate) start_stacks: [u32; 2],
    pub to_act: Option<Seat>,
    acted: [bool; 2],
    /// Size of the last full bet/raise increment: the minimum next raise.
    last_raise: u32,
    pub actions: Vec<ActionRecord>,
    pub outcome: Option<HandOutcome>,
}

impl Hand {
    pub fn hole(&self, seat: Seat) -> [Card; 2] {
        [self.deck[seat.idx()], self.deck[seat.idx() + 2]]
    }

    pub fn board(&self) -> &[Card] {
        &self.deck[4..4 + self.board_len]
    }

    fn max_bet(&self) -> u32 {
        self.bets[0].max(self.bets[1])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub schema: u32,
    pub id: String,
    pub kind: MatchKind,
    /// Absent in saves before 1.2, which were all against Alex.
    #[serde(default)]
    pub opponent: Opponent,
    rng: Rng,
    pub stacks: [u32; 2],
    pub phase: Phase,
    pub hand: Hand,
    pub history: Vec<HandSummary>,
    best_human: Option<BestShown>,
    pub result: Option<MatchResult>,
}

impl Match {
    /// A fresh 8-hand match against Alex.
    pub fn new(seed: u64) -> Match {
        Match::against(seed, Opponent::Alex)
    }

    /// A fresh 8-hand match. The button for hand 1 is drawn at random.
    pub fn against(seed: u64, opponent: Opponent) -> Match {
        let mut rng = Rng::new(seed);
        let button = if rng.chance(0.5) {
            Seat::Human
        } else {
            Seat::Bot
        };
        let mut m = Match::empty(format!("{:016x}", mix(seed)), MatchKind::Standard, rng);
        m.opponent = opponent;
        m.start_hand(1, button, None);
        m
    }

    /// The single deterministic training hand. The human holds the button so
    /// that their first decision is answering the big blind.
    pub fn new_tutorial() -> Match {
        let mut m = Match::empty("tutorial".into(), MatchKind::Tutorial, Rng::new(0));
        m.start_hand(1, Seat::Human, Some(parse_cards(TUTORIAL_DECK)));
        m
    }

    fn empty(id: String, kind: MatchKind, rng: Rng) -> Match {
        Match {
            schema: SCHEMA_VERSION,
            id,
            kind,
            opponent: Opponent::Alex,
            rng,
            stacks: [STARTING_STACK; 2],
            phase: Phase::InHand,
            hand: Hand {
                number: 0,
                button: Seat::Human,
                sb: 0,
                bb: 0,
                deck: Vec::new(),
                street: Street::Preflop,
                board_len: 0,
                bets: [0; 2],
                pot: 0,
                start_stacks: [STARTING_STACK; 2],
                to_act: None,
                acted: [false; 2],
                last_raise: 0,
                actions: Vec::new(),
                outcome: None,
            },
            history: Vec::new(),
            best_human: None,
            result: None,
        }
    }

    pub fn hands_total(&self) -> u8 {
        match self.kind {
            MatchKind::Standard => HANDS_PER_MATCH,
            MatchKind::Tutorial => 1,
        }
    }

    /// A seed for the bot's own randomness that is stable for a given
    /// decision point, so reloading a save reproduces the same bot action.
    pub fn decision_seed(&self) -> u64 {
        let salt = ((self.hand.number as u64) << 32) | self.hand.actions.len() as u64;
        self.rng.derive(salt)
    }

    fn start_hand(&mut self, number: u8, button: Seat, preset: Option<Vec<Card>>) {
        let deck = preset.unwrap_or_else(|| {
            let mut d = full_deck();
            self.rng.shuffle(&mut d);
            d.truncate(DEALT_CARDS);
            d
        });
        let (sb, bb) = blinds(number);
        self.hand = Hand {
            number,
            button,
            sb,
            bb,
            deck,
            street: Street::Preflop,
            board_len: 0,
            bets: [0; 2],
            pot: 0,
            start_stacks: self.stacks,
            to_act: None,
            acted: [false; 2],
            last_raise: bb,
            actions: Vec::new(),
            outcome: None,
        };
        self.phase = Phase::InHand;
        // Heads-up: the button posts the small blind and acts first preflop.
        self.put_in(button, sb, ActionKind::SmallBlind);
        self.put_in(button.other(), bb, ActionKind::BigBlind);
        self.settle(button);
    }

    /// Moves up to `amount` chips from a stack into the street bet.
    fn put_in(&mut self, seat: Seat, amount: u32, kind: ActionKind) -> u32 {
        let i = seat.idx();
        let paid = amount.min(self.stacks[i]);
        self.stacks[i] -= paid;
        self.hand.bets[i] += paid;
        self.hand.actions.push(ActionRecord {
            seat,
            street: self.hand.street,
            kind,
            amount: paid,
            to: self.hand.bets[i],
            all_in: self.stacks[i] == 0,
        });
        paid
    }

    fn needs_action(&self, seat: Seat) -> bool {
        let (me, opp) = (seat.idx(), seat.other().idx());
        self.stacks[me] > 0
            && (self.hand.bets[me] < self.hand.max_bet()
                || (!self.hand.acted[me] && self.stacks[opp] > 0))
    }

    /// Hands the turn to `preferred` or the other seat, or closes the round.
    fn settle(&mut self, preferred: Seat) {
        for seat in [preferred, preferred.other()] {
            if self.needs_action(seat) {
                self.hand.to_act = Some(seat);
                return;
            }
        }
        self.hand.to_act = None;
        self.end_betting_round();
    }

    fn return_uncalled(&mut self) {
        let h = &self.hand;
        let hi = if h.bets[0] >= h.bets[1] {
            Seat::Human
        } else {
            Seat::Bot
        };
        let excess = h.bets[hi.idx()] - h.bets[hi.other().idx()];
        if excess > 0 {
            self.hand.bets[hi.idx()] -= excess;
            self.stacks[hi.idx()] += excess;
            self.hand.actions.push(ActionRecord {
                seat: hi,
                street: self.hand.street,
                kind: ActionKind::Return,
                amount: excess,
                to: self.hand.bets[hi.idx()],
                all_in: false,
            });
        }
    }

    fn collect_bets(&mut self) {
        self.return_uncalled();
        self.hand.pot += self.hand.bets[0] + self.hand.bets[1];
        self.hand.bets = [0; 2];
    }

    fn end_betting_round(&mut self) {
        self.collect_bets();
        if self.hand.street == Street::River {
            self.showdown(None);
        } else if self.stacks.contains(&0) {
            // Someone is all-in and betting is closed: run out the board.
            let from = self.hand.board_len;
            self.hand.street = Street::River;
            self.hand.board_len = 5;
            self.showdown(Some(from));
        } else {
            self.hand.street = self.hand.street.next();
            self.hand.board_len = self.hand.street.board_len();
            self.hand.acted = [false; 2];
            self.hand.last_raise = self.hand.bb;
            // Postflop the big blind (non-button) acts first.
            self.settle(self.hand.button.other());
        }
    }

    fn showdown(&mut self, runout_from: Option<usize>) {
        let shown = Seat::BOTH.map(|seat| {
            let mut seven = self.hand.hole(seat).to_vec();
            seven.extend_from_slice(self.hand.board());
            best_hand(&seven)
        });
        let pot = self.hand.pot;
        let mut awarded = [0u32; 2];
        let winner = match shown[0].value.cmp(&shown[1].value) {
            std::cmp::Ordering::Greater => Some(Seat::Human),
            std::cmp::Ordering::Less => Some(Seat::Bot),
            std::cmp::Ordering::Equal => None,
        };
        match winner {
            Some(w) => awarded[w.idx()] = pot,
            None => {
                // Odd chip goes to the player out of position (big blind).
                let oop = self.hand.button.other().idx();
                awarded = [pot / 2; 2];
                awarded[oop] += pot % 2;
            }
        }
        let human_best = shown[Seat::Human.idx()];
        if self.best_human.is_none_or(|b| human_best.value > b.value) {
            self.best_human = Some(BestShown {
                category: human_best.category(),
                cards: human_best.cards,
                value: human_best.value,
            });
        }
        let show = |seat: Seat, b: &BestHand| ShownHand {
            hole: self.hand.hole(seat),
            category: b.category(),
            best: b.cards,
        };
        let showdown = [show(Seat::Human, &shown[0]), show(Seat::Bot, &shown[1])];
        self.finish_hand(winner, awarded, false, Some(showdown), runout_from);
    }

    fn finish_hand(
        &mut self,
        winner: Option<Seat>,
        awarded: [u32; 2],
        by_fold: bool,
        showdown: Option<[ShownHand; 2]>,
        runout_from: Option<usize>,
    ) {
        let pot = self.hand.pot;
        debug_assert_eq!(awarded[0] + awarded[1], pot);
        self.stacks[0] += awarded[0];
        self.stacks[1] += awarded[1];
        self.hand.pot = 0;
        self.hand.to_act = None;
        let start = self.hand.start_stacks;
        let outcome = HandOutcome {
            winner,
            by_fold,
            ended_on: self.hand.street,
            pot,
            awarded,
            net: [0, 1].map(|i| self.stacks[i] as i32 - start[i] as i32),
            showdown,
            runout_from,
        };
        self.hand.outcome = Some(outcome.clone());
        self.history.push(HandSummary {
            number: self.hand.number,
            button: self.hand.button,
            sb: self.hand.sb,
            bb: self.hand.bb,
            start_stacks: start,
            holes: Some(Seat::BOTH.map(|s| self.hand.hole(s))),
            board: self.hand.board().to_vec(),
            actions: self.hand.actions.clone(),
            outcome,
        });
        self.phase = Phase::HandOver;
        debug_assert_eq!(self.chips_in_play(), TOTAL_CHIPS);
    }

    /// Whether the match ends once the current hand is over.
    pub fn is_final_hand(&self) -> bool {
        self.hand.number >= self.hands_total() || self.stacks.contains(&0)
    }

    pub fn chips_in_play(&self) -> u32 {
        self.stacks[0] + self.stacks[1] + self.hand.bets[0] + self.hand.bets[1] + self.hand.pot
    }

    pub fn legal_actions(&self, seat: Seat) -> Option<LegalActions> {
        if self.phase != Phase::InHand || self.hand.to_act != Some(seat) {
            return None;
        }
        let h = &self.hand;
        let (me, opp) = (seat.idx(), seat.other().idx());
        let max_bet = h.max_bet();
        let to_call = max_bet - h.bets[me];
        let stack = self.stacks[me];

        let raise = (self.stacks[opp] > 0 && stack > to_call)
            .then(|| {
                let max_to = (h.bets[me] + stack).min(h.bets[opp] + self.stacks[opp]);
                (max_to > max_bet).then(|| {
                    let min_to = (max_bet + h.last_raise).min(max_to);
                    let pot_after_call = h.pot + h.bets[0] + h.bets[1] + to_call;
                    let preset = |kind, to: u32| Preset {
                        kind,
                        to,
                        enabled: (min_to..=max_to).contains(&to),
                    };
                    RaiseOption {
                        kind: if max_bet == 0 {
                            RaiseKind::Bet
                        } else {
                            RaiseKind::Raise
                        },
                        min_to,
                        max_to,
                        step: h.sb.max(1),
                        presets: vec![
                            preset(PresetKind::HalfPot, max_bet + pot_after_call / 2),
                            preset(PresetKind::Pot, max_bet + pot_after_call),
                            preset(PresetKind::AllIn, max_to),
                        ],
                    }
                })
            })
            .flatten();

        Some(LegalActions {
            fold: to_call > 0,
            check: to_call == 0,
            call: (to_call > 0).then_some(to_call.min(stack)),
            call_is_all_in: to_call > 0 && to_call >= stack,
            raise,
        })
    }

    pub fn apply(&mut self, seat: Seat, action: PlayerAction) -> Result<(), EngineError> {
        if self.phase != Phase::InHand {
            return Err(EngineError::WrongPhase);
        }
        let legal = self.legal_actions(seat).ok_or(EngineError::NotYourTurn)?;
        let i = seat.idx();
        match action {
            PlayerAction::Fold if legal.fold => {
                self.hand.acted[i] = true;
                self.hand.actions.push(ActionRecord {
                    seat,
                    street: self.hand.street,
                    kind: ActionKind::Fold,
                    amount: 0,
                    to: self.hand.bets[i],
                    all_in: false,
                });
                self.hand.to_act = None;
                self.collect_bets();
                let winner = seat.other();
                let mut awarded = [0; 2];
                awarded[winner.idx()] = self.hand.pot;
                self.finish_hand(Some(winner), awarded, true, None, None);
                return Ok(());
            }
            PlayerAction::Check if legal.check => {
                self.hand.acted[i] = true;
                self.hand.actions.push(ActionRecord {
                    seat,
                    street: self.hand.street,
                    kind: ActionKind::Check,
                    amount: 0,
                    to: self.hand.bets[i],
                    all_in: false,
                });
            }
            PlayerAction::Call if legal.call.is_some() => {
                self.hand.acted[i] = true;
                let to_call = self.hand.max_bet() - self.hand.bets[i];
                self.put_in(seat, to_call, ActionKind::Call);
            }
            PlayerAction::RaiseTo { amount } => {
                let r = legal.raise.ok_or(EngineError::IllegalAction)?;
                if !(r.min_to..=r.max_to).contains(&amount) {
                    return Err(EngineError::InvalidAmount {
                        min: r.min_to,
                        max: r.max_to,
                    });
                }
                let max_bet = self.hand.max_bet();
                let increment = amount - max_bet;
                // A short all-in raise does not change the minimum raise size.
                if increment >= self.hand.last_raise {
                    self.hand.last_raise = increment;
                }
                self.hand.acted[i] = true;
                let kind = match r.kind {
                    RaiseKind::Bet => ActionKind::Bet,
                    RaiseKind::Raise => ActionKind::Raise,
                };
                self.put_in(seat, amount - self.hand.bets[i], kind);
            }
            _ => return Err(EngineError::IllegalAction),
        }
        self.settle(seat.other());
        Ok(())
    }

    /// Leaves the hand summary: starts the next hand or ends the match.
    pub fn next_hand(&mut self) -> Result<(), EngineError> {
        if self.phase != Phase::HandOver {
            return Err(EngineError::WrongPhase);
        }
        if self.is_final_hand() {
            self.finish_match();
        } else {
            let number = self.hand.number + 1;
            let button = self.hand.button.other();
            self.start_hand(number, button, None);
        }
        Ok(())
    }

    fn finish_match(&mut self) {
        let [human, bot] = self.stacks;
        let mut hands_won = [0u8; 2];
        for h in &self.history {
            if let Some(w) = h.outcome.winner {
                hands_won[w.idx()] += 1;
            }
        }
        self.result = Some(MatchResult {
            outcome: match human.cmp(&bot) {
                std::cmp::Ordering::Greater => MatchOutcome::Win,
                std::cmp::Ordering::Less => MatchOutcome::Loss,
                std::cmp::Ordering::Equal => MatchOutcome::Draw,
            },
            final_stacks: self.stacks,
            hands_played: self.hand.number,
            hands_won,
            ended_early: self.hand.number < self.hands_total(),
            best_hand: self.best_human,
        });
        self.phase = Phase::MatchOver;
    }

    /// Fills fields that older, still compatible snapshots don't carry.
    pub fn upgrade(&mut self) {
        if self.history.iter().any(|h| h.start_stacks == [0, 0]) {
            let mut stacks = [STARTING_STACK; 2];
            for h in &mut self.history {
                h.start_stacks = stacks;
                for (stack, net) in stacks.iter_mut().zip(h.outcome.net) {
                    *stack = stack.saturating_add_signed(net);
                }
            }
        }
    }

    /// Sanity checks for a snapshot loaded from storage.
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.schema != SCHEMA_VERSION {
            return Err(EngineError::IncompatibleSave { found: self.schema });
        }
        let h = &self.hand;
        if h.deck.len() != DEALT_CARDS {
            return Err(EngineError::CorruptSave("deck size"));
        }
        let mut seen = [false; 52];
        for c in &h.deck {
            if std::mem::replace(&mut seen[c.index()], true) {
                return Err(EngineError::CorruptSave("duplicate card"));
            }
        }
        if h.number == 0 || h.number > self.hands_total() {
            return Err(EngineError::CorruptSave("hand number"));
        }
        if h.board_len != h.street.board_len() {
            return Err(EngineError::CorruptSave("board"));
        }
        if self.chips_in_play() != TOTAL_CHIPS {
            return Err(EngineError::CorruptSave("chip count"));
        }
        let consistent = match self.phase {
            Phase::InHand => h.to_act.is_some() && h.outcome.is_none(),
            Phase::HandOver => h.to_act.is_none() && h.outcome.is_some(),
            Phase::MatchOver => self.result.is_some(),
        };
        if !consistent {
            return Err(EngineError::CorruptSave("phase"));
        }
        Ok(())
    }

    /// Everything `seat` is allowed to know. Both the UI and the bot are fed
    /// exclusively through this projection.
    pub fn view(&self, seat: Seat) -> PlayerView {
        let h = &self.hand;
        let showdown = h.outcome.as_ref().and_then(|o| o.showdown);
        PlayerView {
            schema: self.schema,
            match_id: self.id.clone(),
            kind: self.kind,
            opponent: self.opponent,
            phase: self.phase,
            me: seat,
            hand_number: h.number,
            hands_total: self.hands_total(),
            sb: h.sb,
            bb: h.bb,
            button: h.button,
            street: h.street,
            stacks: self.stacks,
            bets: h.bets,
            pot: h.pot,
            board: h.board().to_vec(),
            hole: h.hole(seat),
            opponent_hole: showdown.map(|s| s[seat.other().idx()].hole),
            to_act: h.to_act,
            to_call: h.max_bet() - h.bets[seat.idx()],
            legal: self.legal_actions(seat),
            actions: h.actions.clone(),
            outcome: h.outcome.clone(),
            is_final_hand: self.is_final_hand(),
            next_blinds: (!self.is_final_hand()).then(|| blinds(h.number + 1)),
            result: self.result.clone(),
            history: self.hand_records(seat, false),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerView {
    pub schema: u32,
    pub match_id: String,
    pub kind: MatchKind,
    pub opponent: Opponent,
    pub phase: Phase,
    pub me: Seat,
    pub hand_number: u8,
    pub hands_total: u8,
    pub sb: u32,
    pub bb: u32,
    pub button: Seat,
    pub street: Street,
    /// Indexed by seat: `[human, bot]`.
    pub stacks: [u32; 2],
    pub bets: [u32; 2],
    pub pot: u32,
    pub board: Vec<Card>,
    pub hole: [Card; 2],
    /// Only after a showdown; never after a fold.
    pub opponent_hole: Option<[Card; 2]>,
    pub to_act: Option<Seat>,
    pub to_call: u32,
    pub legal: Option<LegalActions>,
    pub actions: Vec<ActionRecord>,
    pub outcome: Option<HandOutcome>,
    pub is_final_hand: bool,
    pub next_blinds: Option<(u32, u32)>,
    pub result: Option<MatchResult>,
    /// Finished hands of this match, as `me` is allowed to see them.
    pub history: Vec<HandRecord>,
}

#[cfg(test)]
#[path = "engine_tests.rs"]
pub(crate) mod tests;
