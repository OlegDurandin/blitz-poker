//! No-Limit Hold'em for 2 to [`MAX_SEATS`] players ("Третий не лишний").
//!
//! The same game as [`crate::Match`], generalised to a table: seats are
//! numbered `0..n` clockwise, side pots form when players are all-in for
//! different amounts, and a seat that runs out of chips (or leaves) sits out
//! the rest of the match. With two seats it plays exactly like `Match`
//! (heads-up rules: the button posts the small blind and acts first
//! preflop), which `table_tests::heads_up_matches_the_duel_engine` checks
//! hand by hand on random play.
//!
//! Fairness: a match lasts a whole number of orbits (see [`hands_for`]) and
//! blinds rise per orbit, so every seat posts every blind level equally
//! often while nobody busts.
//!
//! Chip invariant: `stacks + bets + pot == seats * STARTING_STACK` after
//! every transition.

use crate::card::{Card, full_deck};
use crate::engine::{
    ActionKind, BestShown, EngineError, LegalActions, Opponent, Phase, PlayerAction, Preset,
    PresetKind, RaiseKind, RaiseOption, STARTING_STACK, ShownHand, Street,
};
use crate::eval::{BestHand, best_hand};
use crate::rng::{Rng, mix};
use serde::{Deserialize, Serialize};

pub const TABLE_SCHEMA_VERSION: u32 = 1;
pub const MIN_SEATS: usize = 2;
pub const MAX_SEATS: usize = 6;
/// A match is at least this many hands, rounded up to whole orbits.
const MIN_HANDS: usize = 8;
/// Blinds per orbit; the last level holds for any further orbits.
const BLIND_LEVELS: [(u32, u32); 4] = [(10, 20), (15, 30), (20, 40), (30, 60)];

/// Hands in a match: the fewest whole orbits that make at least eight
/// hands (2 seats: 8, 3: 9, 4: 8, 5: 10, 6: 12).
pub fn hands_for(seats: usize) -> u8 {
    let seats = seats.max(1);
    MIN_HANDS.div_ceil(seats).saturating_mul(seats) as u8
}

/// Blinds (small, big) for a 1-based hand number: one level per orbit.
/// With two seats this is the duel schedule (`engine::blinds`).
pub fn table_blinds(hand_number: u8, seats: usize) -> (u32, u32) {
    let orbit = usize::from(hand_number.max(1) - 1) / seats.max(1);
    BLIND_LEVELS[orbit.min(BLIND_LEVELS.len() - 1)]
}

/// Cards a hand needs: two per seat and the board. Seat `s` holds
/// `deck[s]` and `deck[s + n]`; the board is `deck[2n..2n + 5]`.
pub fn dealt_cards(seats: usize) -> usize {
    2 * seats + 5
}

/// Who sits in a seat. The engine plays both the same way; the kind is kept
/// so a saved table resumes with the same bots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SeatKind {
    Player,
    Bot { opponent: Opponent },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    /// One person against bots, dealt from the table's own seeded RNG.
    Solo,
    /// Played through the server, which deals every hand (CSPRNG).
    Online,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableAction {
    pub seat: usize,
    pub street: Street,
    pub kind: ActionKind,
    /// Chips moved by this action.
    pub amount: u32,
    /// The player's total bet on this street after the action.
    pub to: u32,
    pub all_in: bool,
}

/// One pot of a finished hand: the main pot first, then side pots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PotShare {
    pub amount: u32,
    /// Seats still in the hand that had covered this pot.
    pub eligible: Vec<usize>,
    pub winners: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableOutcome {
    pub by_fold: bool,
    /// Street on which the hand ended (river for any showdown).
    pub ended_on: Street,
    pub pot: u32,
    pub pots: Vec<PotShare>,
    /// Per seat.
    pub awarded: Vec<u32>,
    /// Stack change over the whole hand, per seat.
    pub net: Vec<i32>,
    /// Per seat, present only when cards were shown down: the hands of the
    /// seats that reached the showdown, `None` for the others.
    pub showdown: Option<Vec<Option<ShownHand>>>,
    /// Board length when an all-in runout started.
    pub runout_from: Option<usize>,
}

impl TableOutcome {
    /// The sole winner of the main pot; `None` when it was split.
    pub fn winner(&self) -> Option<usize> {
        match self.pots.first().map(|p| p.winners.as_slice()) {
            Some([w]) => Some(*w),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableHand {
    pub number: u8,
    pub button: usize,
    pub sb_seat: usize,
    pub bb_seat: usize,
    pub sb: u32,
    pub bb: u32,
    deck: Vec<Card>,
    pub street: Street,
    board_len: usize,
    /// Seats dealt into this hand: they had chips and had not left.
    pub dealt: Vec<bool>,
    pub folded: Vec<bool>,
    /// Current street bets.
    pub bets: Vec<u32>,
    /// Everything each seat put in this hand, current street included.
    pub committed: Vec<u32>,
    /// Chips collected from finished betting rounds.
    pub pot: u32,
    pub start_stacks: Vec<u32>,
    pub to_act: Option<usize>,
    /// Acted since the last full bet or raise on this street.
    acted: Vec<bool>,
    /// Size of the last full bet/raise increment: the minimum next raise.
    last_raise: u32,
    pub actions: Vec<TableAction>,
    pub outcome: Option<TableOutcome>,
}

impl TableHand {
    fn seats(&self) -> usize {
        self.dealt.len()
    }

    pub fn hole(&self, seat: usize) -> [Card; 2] {
        [self.deck[seat], self.deck[seat + self.seats()]]
    }

    pub fn board(&self) -> &[Card] {
        let start = 2 * self.seats();
        &self.deck[start..start + self.board_len]
    }

    fn max_bet(&self) -> u32 {
        self.bets.iter().copied().max().unwrap_or(0)
    }

    /// Dealt in and not folded.
    fn contends(&self, seat: usize) -> bool {
        self.dealt[seat] && !self.folded[seat]
    }

    fn contenders(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.seats()).filter(|&s| self.contends(s))
    }
}

/// Internal record of a finished hand, with every dealt seat's hole cards;
/// clients get [`TableHandRecord`]s projected for one seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableHandSummary {
    pub number: u8,
    pub button: usize,
    pub sb_seat: usize,
    pub bb_seat: usize,
    pub sb: u32,
    pub bb: u32,
    pub start_stacks: Vec<u32>,
    holes: Vec<Option<[Card; 2]>>,
    pub board: Vec<Card>,
    pub actions: Vec<TableAction>,
    pub outcome: TableOutcome,
}

/// A finished hand as one seat may see it: its own hole cards, and other
/// seats' cards only if they were shown down (they are in `outcome`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableHandRecord {
    pub number: u8,
    pub button: usize,
    pub sb_seat: usize,
    pub bb_seat: usize,
    pub sb: u32,
    pub bb: u32,
    pub start_stacks: Vec<u32>,
    /// `None` if the viewer was not dealt in.
    pub hole: Option<[Card; 2]>,
    pub board: Vec<Card>,
    pub actions: Vec<TableAction>,
    pub outcome: TableOutcome,
}

impl TableHandSummary {
    pub fn record_for(&self, seat: usize) -> TableHandRecord {
        TableHandRecord {
            number: self.number,
            button: self.button,
            sb_seat: self.sb_seat,
            bb_seat: self.bb_seat,
            sb: self.sb,
            bb: self.bb,
            start_stacks: self.start_stacks.clone(),
            hole: self.holes.get(seat).copied().flatten(),
            board: self.board.clone(),
            actions: self.actions.clone(),
            outcome: self.outcome.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableResult {
    pub final_stacks: Vec<u32>,
    /// 1-based place per seat. Equal stacks share a place (1, 1, 3); seats
    /// that left come after everyone who stayed, the earliest to leave last.
    pub places: Vec<u8>,
    pub hands_played: u8,
    /// Hands in which the seat alone won the main pot.
    pub hands_won: Vec<u8>,
    pub ended_early: bool,
    /// Best hand each seat made at a showdown.
    pub best_hands: Vec<Option<BestShown>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub schema: u32,
    pub id: String,
    pub kind: TableKind,
    pub seats: Vec<SeatKind>,
    rng: Rng,
    pub stacks: Vec<u32>,
    /// Seats that left, in the order they left. They play no further hands.
    pub left: Vec<usize>,
    pub phase: Phase,
    pub hand: TableHand,
    pub history: Vec<TableHandSummary>,
    best: Vec<Option<BestShown>>,
    pub result: Option<TableResult>,
}

fn checked_deal(deck: &[Card], seats: usize) -> Result<Vec<Card>, EngineError> {
    let need = dealt_cards(seats);
    if deck.len() < need {
        return Err(EngineError::CorruptSave("deck size"));
    }
    let deal = deck[..need].to_vec();
    let mut seen = [false; 52];
    for c in &deal {
        if std::mem::replace(&mut seen[c.index()], true) {
            return Err(EngineError::CorruptSave("duplicate card"));
        }
    }
    Ok(deal)
}

fn check_seats(seats: &[SeatKind]) -> Result<(), EngineError> {
    if (MIN_SEATS..=MAX_SEATS).contains(&seats.len()) {
        Ok(())
    } else {
        Err(EngineError::IllegalAction)
    }
}

impl Table {
    /// A table against bots, dealt from a seeded RNG. The first button is
    /// drawn at random.
    pub fn solo(seed: u64, seats: Vec<SeatKind>) -> Result<Table, EngineError> {
        check_seats(&seats)?;
        let mut rng = Rng::new(seed);
        let button = rng.below(seats.len() as u64) as usize;
        let mut t = Table::empty(format!("{:016x}", mix(seed)), TableKind::Solo, seats, rng);
        t.start_hand(1, button, None);
        Ok(t)
    }

    /// A table run by the server: it picks the first button and deals every
    /// hand from its CSPRNG; the table never deals on its own.
    pub fn online(
        id: String,
        seats: Vec<SeatKind>,
        button: usize,
        deck: &[Card],
    ) -> Result<Table, EngineError> {
        check_seats(&seats)?;
        if button >= seats.len() {
            return Err(EngineError::IllegalAction);
        }
        let deal = checked_deal(deck, seats.len())?;
        let mut t = Table::empty(id, TableKind::Online, seats, Rng::new(0));
        t.start_hand(1, button, Some(deal));
        Ok(t)
    }

    fn empty(id: String, kind: TableKind, seats: Vec<SeatKind>, rng: Rng) -> Table {
        let n = seats.len();
        Table {
            schema: TABLE_SCHEMA_VERSION,
            id,
            kind,
            seats,
            rng,
            stacks: vec![STARTING_STACK; n],
            left: Vec::new(),
            phase: Phase::InHand,
            hand: TableHand {
                number: 0,
                button: 0,
                sb_seat: 0,
                bb_seat: 0,
                sb: 0,
                bb: 0,
                deck: Vec::new(),
                street: Street::Preflop,
                board_len: 0,
                dealt: vec![false; n],
                folded: vec![false; n],
                bets: vec![0; n],
                committed: vec![0; n],
                pot: 0,
                start_stacks: vec![STARTING_STACK; n],
                to_act: None,
                acted: vec![false; n],
                last_raise: 0,
                actions: Vec::new(),
                outcome: None,
            },
            history: Vec::new(),
            best: vec![None; n],
            result: None,
        }
    }

    fn n(&self) -> usize {
        self.seats.len()
    }

    pub fn total_chips(&self) -> u32 {
        STARTING_STACK * self.n() as u32
    }

    pub fn hands_total(&self) -> u8 {
        hands_for(self.n())
    }

    /// Has chips and has not left: will be dealt the next hand.
    pub fn is_live(&self, seat: usize) -> bool {
        self.stacks[seat] > 0 && !self.left.contains(&seat)
    }

    fn live_count(&self) -> usize {
        (0..self.n()).filter(|&s| self.is_live(s)).count()
    }

    /// The first live seat clockwise after `from` (`from` itself if alone).
    fn next_live(&self, from: usize) -> usize {
        let n = self.n();
        (1..=n)
            .map(|i| (from + i) % n)
            .find(|&s| self.is_live(s))
            .unwrap_or(from)
    }

    /// A seed for bot randomness that is stable for a given decision point.
    pub fn decision_seed(&self) -> u64 {
        let salt = ((self.hand.number as u64) << 32) | self.hand.actions.len() as u64;
        self.rng.derive(salt)
    }

    fn start_hand(&mut self, number: u8, button: usize, preset: Option<Vec<Card>>) {
        let n = self.n();
        let deck = preset.unwrap_or_else(|| {
            let mut d = full_deck();
            self.rng.shuffle(&mut d);
            d.truncate(dealt_cards(n));
            d
        });
        let (sb, bb) = table_blinds(number, n);
        let dealt: Vec<bool> = (0..n).map(|s| self.is_live(s)).collect();
        // Heads-up the button posts the small blind; otherwise the next seat.
        let heads_up = dealt.iter().filter(|&&d| d).count() == 2;
        let sb_seat = if heads_up {
            button
        } else {
            self.next_live(button)
        };
        let bb_seat = self.next_live(sb_seat);
        self.hand = TableHand {
            number,
            button,
            sb_seat,
            bb_seat,
            sb,
            bb,
            deck,
            street: Street::Preflop,
            board_len: 0,
            dealt,
            folded: vec![false; n],
            bets: vec![0; n],
            committed: vec![0; n],
            pot: 0,
            start_stacks: self.stacks.clone(),
            to_act: None,
            acted: vec![false; n],
            last_raise: bb,
            actions: Vec::new(),
            outcome: None,
        };
        self.phase = Phase::InHand;
        self.put_in(sb_seat, sb, ActionKind::SmallBlind);
        self.put_in(bb_seat, bb, ActionKind::BigBlind);
        // Preflop the seat after the big blind opens (heads-up: the button).
        self.settle((bb_seat + 1) % n);
    }

    fn record(&mut self, seat: usize, kind: ActionKind, amount: u32) {
        self.hand.actions.push(TableAction {
            seat,
            street: self.hand.street,
            kind,
            amount,
            to: self.hand.bets[seat],
            all_in: amount > 0 && self.stacks[seat] == 0,
        });
    }

    /// Moves up to `amount` chips from a stack into the street bet.
    fn put_in(&mut self, seat: usize, amount: u32, kind: ActionKind) -> u32 {
        let paid = amount.min(self.stacks[seat]);
        self.stacks[seat] -= paid;
        self.hand.bets[seat] += paid;
        self.hand.committed[seat] += paid;
        self.hand.actions.push(TableAction {
            seat,
            street: self.hand.street,
            kind,
            amount: paid,
            to: self.hand.bets[seat],
            all_in: self.stacks[seat] == 0,
        });
        paid
    }

    fn needs_action(&self, seat: usize) -> bool {
        let h = &self.hand;
        if !h.contends(seat) || self.stacks[seat] == 0 {
            return false;
        }
        let others_can_bet = h.contenders().any(|s| s != seat && self.stacks[s] > 0);
        h.bets[seat] < h.max_bet() || (!h.acted[seat] && others_can_bet)
    }

    /// Hands the turn to the first seat from `from` clockwise that must act,
    /// or closes the betting round.
    fn settle(&mut self, from: usize) {
        let n = self.n();
        if let Some(seat) = (0..n)
            .map(|i| (from + i) % n)
            .find(|&s| self.needs_action(s))
        {
            self.hand.to_act = Some(seat);
            return;
        }
        self.hand.to_act = None;
        self.end_betting_round();
    }

    /// The part of the top bet nobody matched goes back to its owner.
    fn return_uncalled(&mut self) {
        let h = &self.hand;
        let Some(top) = (0..h.seats()).max_by_key(|&s| (h.bets[s], std::cmp::Reverse(s))) else {
            return;
        };
        let second = (0..h.seats())
            .filter(|&s| s != top)
            .map(|s| h.bets[s])
            .max()
            .unwrap_or(0);
        let excess = h.bets[top] - second;
        if excess > 0 {
            self.hand.bets[top] -= excess;
            self.hand.committed[top] -= excess;
            self.stacks[top] += excess;
            self.hand.actions.push(TableAction {
                seat: top,
                street: self.hand.street,
                kind: ActionKind::Return,
                amount: excess,
                to: self.hand.bets[top],
                all_in: false,
            });
        }
    }

    fn collect_bets(&mut self) {
        self.return_uncalled();
        self.hand.pot += self.hand.bets.iter().sum::<u32>();
        self.hand.bets.iter_mut().for_each(|b| *b = 0);
    }

    fn end_betting_round(&mut self) {
        self.collect_bets();
        let with_chips = self
            .hand
            .contenders()
            .filter(|&s| self.stacks[s] > 0)
            .count();
        if self.hand.street == Street::River {
            self.showdown(None);
        } else if with_chips <= 1 {
            // Everyone but at most one is all-in: run out the board.
            let from = self.hand.board_len;
            self.hand.street = Street::River;
            self.hand.board_len = 5;
            self.showdown(Some(from));
        } else {
            self.hand.street = self.hand.street.next();
            self.hand.board_len = self.hand.street.board_len();
            self.hand.acted.iter_mut().for_each(|a| *a = false);
            self.hand.last_raise = self.hand.bb;
            // Postflop the first seat after the button acts first.
            self.settle((self.hand.button + 1) % self.n());
        }
    }

    /// Seats in the order odd chips are handed out: clockwise from the
    /// button's left (heads-up: the big blind first).
    fn clockwise_from_button(&self) -> impl Iterator<Item = usize> + '_ {
        let n = self.n();
        (1..=n).map(move |i| (self.hand.button + i) % n)
    }

    /// Splits `amount` between `winners`; odd chips go one each from the
    /// button's left.
    fn split(&self, amount: u32, winners: &[usize], awarded: &mut [u32]) {
        let share = amount / winners.len() as u32;
        let mut odd = amount % winners.len() as u32;
        for &w in winners {
            awarded[w] += share;
        }
        for s in self.clockwise_from_button() {
            if odd == 0 {
                break;
            }
            if winners.contains(&s) {
                awarded[s] += 1;
                odd -= 1;
            }
        }
    }

    /// Main pot and side pots by how much each seat still in the hand put in.
    /// Chips of folded seats go into the pots they reached; anything above
    /// the deepest contender stays in the last pot.
    fn build_pots(&self) -> Vec<(u32, Vec<usize>)> {
        let h = &self.hand;
        let mut levels: Vec<u32> = h.contenders().map(|s| h.committed[s]).collect();
        levels.sort_unstable();
        levels.dedup();
        let mut pots = Vec::new();
        let mut prev = 0;
        for (i, &level) in levels.iter().enumerate() {
            let last = i + 1 == levels.len();
            let amount: u32 = h
                .committed
                .iter()
                .map(|&c| if last { c } else { c.min(level) } - c.min(prev))
                .sum();
            let eligible: Vec<usize> = h
                .contenders()
                .filter(|&s| h.committed[s] >= level)
                .collect();
            if amount > 0 {
                pots.push((amount, eligible));
            }
            prev = level;
        }
        pots
    }

    fn showdown(&mut self, runout_from: Option<usize>) {
        let n = self.n();
        let mut made: Vec<Option<BestHand>> = vec![None; n];
        for s in self.hand.contenders().collect::<Vec<_>>() {
            let mut seven = self.hand.hole(s).to_vec();
            seven.extend_from_slice(self.hand.board());
            made[s] = Some(best_hand(&seven));
        }
        let mut awarded = vec![0u32; n];
        let mut pots = Vec::new();
        for (amount, eligible) in self.build_pots() {
            let top = eligible
                .iter()
                .filter_map(|&s| made[s].as_ref().map(|b| b.value))
                .max()
                .expect("a pot has an eligible seat");
            let winners: Vec<usize> = eligible
                .iter()
                .copied()
                .filter(|&s| made[s].as_ref().is_some_and(|b| b.value == top))
                .collect();
            self.split(amount, &winners, &mut awarded);
            pots.push(PotShare {
                amount,
                eligible,
                winners,
            });
        }
        for (best, m) in self.best.iter_mut().zip(&made) {
            if let Some(m) = m
                && best.is_none_or(|b| m.value > b.value)
            {
                *best = Some(BestShown {
                    category: m.category(),
                    cards: m.cards,
                    value: m.value,
                });
            }
        }
        let shown: Vec<Option<ShownHand>> = (0..n)
            .map(|s| {
                made[s].as_ref().map(|b| ShownHand {
                    hole: self.hand.hole(s),
                    category: b.category(),
                    best: b.cards,
                })
            })
            .collect();
        self.finish_hand(awarded, pots, false, Some(shown), runout_from);
    }

    /// Everyone else folded: the last seat takes the pot unseen.
    fn win_by_fold(&mut self) {
        self.hand.to_act = None;
        self.collect_bets();
        let winner = self.hand.contenders().next().expect("one seat remains");
        let mut awarded = vec![0; self.n()];
        awarded[winner] = self.hand.pot;
        let pots = vec![PotShare {
            amount: self.hand.pot,
            eligible: vec![winner],
            winners: vec![winner],
        }];
        self.finish_hand(awarded, pots, true, None, None);
    }

    fn finish_hand(
        &mut self,
        awarded: Vec<u32>,
        pots: Vec<PotShare>,
        by_fold: bool,
        showdown: Option<Vec<Option<ShownHand>>>,
        runout_from: Option<usize>,
    ) {
        let pot = self.hand.pot;
        debug_assert_eq!(awarded.iter().sum::<u32>(), pot);
        for (stack, a) in self.stacks.iter_mut().zip(&awarded) {
            *stack += a;
        }
        self.hand.pot = 0;
        self.hand.to_act = None;
        let start = self.hand.start_stacks.clone();
        let outcome = TableOutcome {
            by_fold,
            ended_on: self.hand.street,
            pot,
            pots,
            awarded,
            net: (0..self.n())
                .map(|s| self.stacks[s] as i32 - start[s] as i32)
                .collect(),
            showdown,
            runout_from,
        };
        self.hand.outcome = Some(outcome.clone());
        self.history.push(TableHandSummary {
            number: self.hand.number,
            button: self.hand.button,
            sb_seat: self.hand.sb_seat,
            bb_seat: self.hand.bb_seat,
            sb: self.hand.sb,
            bb: self.hand.bb,
            start_stacks: start,
            holes: (0..self.n())
                .map(|s| self.hand.dealt[s].then(|| self.hand.hole(s)))
                .collect(),
            board: self.hand.board().to_vec(),
            actions: self.hand.actions.clone(),
            outcome,
        });
        self.phase = Phase::HandOver;
        debug_assert_eq!(self.chips_in_play(), self.total_chips());
    }

    /// Whether the match ends once the current hand is over.
    pub fn is_final_hand(&self) -> bool {
        self.hand.number >= self.hands_total() || self.live_count() < 2
    }

    pub fn chips_in_play(&self) -> u32 {
        self.stacks.iter().sum::<u32>() + self.hand.bets.iter().sum::<u32>() + self.hand.pot
    }

    pub fn legal_actions(&self, seat: usize) -> Option<LegalActions> {
        if self.phase != Phase::InHand || self.hand.to_act != Some(seat) {
            return None;
        }
        let h = &self.hand;
        let max_bet = h.max_bet();
        let to_call = max_bet - h.bets[seat];
        let stack = self.stacks[seat];
        // Nobody can be made to call more than the deepest other player has.
        let others_cover = h
            .contenders()
            .filter(|&s| s != seat)
            .map(|s| h.bets[s] + self.stacks[s])
            .max()
            .unwrap_or(0);

        // A short all-in raise does not reopen betting for seats that acted.
        let raise = (!h.acted[seat] && stack > to_call)
            .then(|| {
                let max_to = (h.bets[seat] + stack).min(others_cover);
                (max_to > max_bet).then(|| {
                    let min_to = (max_bet + h.last_raise).min(max_to);
                    let pot_after_call = h.pot + h.bets.iter().sum::<u32>() + to_call;
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
            fold: true,
            check: to_call == 0,
            call: (to_call > 0).then_some(to_call.min(stack)),
            call_is_all_in: to_call > 0 && to_call >= stack,
            raise,
        })
    }

    pub fn apply(&mut self, seat: usize, action: PlayerAction) -> Result<(), EngineError> {
        if self.phase != Phase::InHand {
            return Err(EngineError::WrongPhase);
        }
        let legal = self.legal_actions(seat).ok_or(EngineError::NotYourTurn)?;
        match action {
            PlayerAction::Fold if legal.fold => {
                self.fold(seat);
                return Ok(());
            }
            PlayerAction::Check if legal.check => {
                self.hand.acted[seat] = true;
                self.record(seat, ActionKind::Check, 0);
            }
            PlayerAction::Call if legal.call.is_some() => {
                self.hand.acted[seat] = true;
                let to_call = self.hand.max_bet() - self.hand.bets[seat];
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
                let increment = amount - self.hand.max_bet();
                if increment >= self.hand.last_raise {
                    // A full raise: everyone else may act (and raise) again.
                    self.hand.last_raise = increment;
                    self.hand.acted.iter_mut().for_each(|a| *a = false);
                }
                self.hand.acted[seat] = true;
                let kind = match r.kind {
                    RaiseKind::Bet => ActionKind::Bet,
                    RaiseKind::Raise => ActionKind::Raise,
                };
                self.put_in(seat, amount - self.hand.bets[seat], kind);
            }
            _ => return Err(EngineError::IllegalAction),
        }
        self.settle((seat + 1) % self.n());
        Ok(())
    }

    /// Folds `seat`, in turn or not (a seat that leaves mid-hand).
    fn fold(&mut self, seat: usize) {
        self.hand.acted[seat] = true;
        self.hand.folded[seat] = true;
        self.record(seat, ActionKind::Fold, 0);
        if self.hand.contenders().count() == 1 {
            self.win_by_fold();
        } else {
            let from = match self.hand.to_act {
                Some(s) if s != seat => s,
                _ => (seat + 1) % self.n(),
            };
            self.settle(from);
        }
    }

    /// `seat` leaves the table (gave up, disconnected for good): folds any
    /// hand in progress and plays no further hands; it finishes below every
    /// seat that stayed. The match ends at once if fewer than two remain
    /// and no hand is in progress.
    pub fn leave(&mut self, seat: usize) -> Result<(), EngineError> {
        if seat >= self.n() || self.left.contains(&seat) {
            return Err(EngineError::IllegalAction);
        }
        if self.phase == Phase::MatchOver {
            return Err(EngineError::WrongPhase);
        }
        self.left.push(seat);
        if self.phase == Phase::InHand && self.hand.contends(seat) {
            self.fold(seat);
        }
        if self.phase == Phase::HandOver && self.live_count() < 2 {
            self.finish_match();
        }
        Ok(())
    }

    /// Leaves the hand summary: starts the next hand or ends the match.
    pub fn next_hand(&mut self) -> Result<(), EngineError> {
        if self.phase != Phase::HandOver {
            return Err(EngineError::WrongPhase);
        }
        if self.kind == TableKind::Online && !self.is_final_hand() {
            // Online decks must come from the server's CSPRNG.
            return Err(EngineError::IllegalAction);
        }
        self.advance(None);
        Ok(())
    }

    /// Online version of [`Table::next_hand`]: the next hand is dealt from
    /// `deck` (only the first [`dealt_cards`] are used).
    pub fn next_hand_dealt(&mut self, deck: &[Card]) -> Result<(), EngineError> {
        if self.phase != Phase::HandOver {
            return Err(EngineError::WrongPhase);
        }
        self.advance(Some(checked_deal(deck, self.n())?));
        Ok(())
    }

    fn advance(&mut self, deck: Option<Vec<Card>>) {
        if self.is_final_hand() {
            self.finish_match();
        } else {
            let button = self.next_live(self.hand.button);
            self.start_hand(self.hand.number + 1, button, deck);
        }
    }

    /// What a player who ran out of time does: check if free, else fold.
    pub fn timeout_action(&self, seat: usize) -> Option<PlayerAction> {
        self.legal_actions(seat).map(|l| {
            if l.check {
                PlayerAction::Check
            } else {
                PlayerAction::Fold
            }
        })
    }

    /// How long `seat` lasted: its stack, and for a busted seat the hand it
    /// went out in and the stack it had then. Later and deeper is better.
    fn standing(&self, seat: usize) -> (u32, u8, u32) {
        if self.stacks[seat] > 0 {
            return (self.stacks[seat], u8::MAX, 0);
        }
        self.history
            .iter()
            .rev()
            .find(|h| h.start_stacks[seat] > 0)
            .map_or((0, 0, 0), |h| (0, h.number, h.start_stacks[seat]))
    }

    fn places(&self) -> Vec<u8> {
        let n = self.n();
        let stayed: Vec<usize> = (0..n).filter(|s| !self.left.contains(s)).collect();
        let mut places = vec![0u8; n];
        for &s in &stayed {
            let mine = self.standing(s);
            let better = stayed.iter().filter(|&&o| self.standing(o) > mine).count();
            places[s] = better as u8 + 1;
        }
        // The last to leave ranks right after those who stayed.
        for (i, &s) in self.left.iter().rev().enumerate() {
            places[s] = (stayed.len() + i + 1) as u8;
        }
        places
    }

    /// Ends the match after a finished hand, before its last hand: a solo
    /// player who has run out of chips need not watch the bots play on.
    /// Places count as they stand.
    pub fn end_now(&mut self) -> Result<(), EngineError> {
        if self.phase != Phase::HandOver {
            return Err(EngineError::WrongPhase);
        }
        self.finish_match();
        Ok(())
    }

    fn finish_match(&mut self) {
        self.hand.to_act = None;
        let mut hands_won = vec![0u8; self.n()];
        for h in &self.history {
            if let Some(w) = h.outcome.winner() {
                hands_won[w] += 1;
            }
        }
        let hands_played = self.history.len() as u8;
        self.result = Some(TableResult {
            final_stacks: self.stacks.clone(),
            places: self.places(),
            hands_played,
            hands_won,
            ended_early: hands_played < self.hands_total() || !self.left.is_empty(),
            best_hands: self.best.clone(),
        });
        self.phase = Phase::MatchOver;
    }

    /// Sanity checks for a snapshot loaded from storage.
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.schema != TABLE_SCHEMA_VERSION {
            return Err(EngineError::IncompatibleSave { found: self.schema });
        }
        let n = self.n();
        check_seats(&self.seats).map_err(|_| EngineError::CorruptSave("seats"))?;
        let h = &self.hand;
        let sizes = [
            self.stacks.len(),
            self.best.len(),
            h.dealt.len(),
            h.folded.len(),
            h.bets.len(),
            h.committed.len(),
            h.start_stacks.len(),
            h.acted.len(),
        ];
        if sizes.iter().any(|&len| len != n) {
            return Err(EngineError::CorruptSave("seat count"));
        }
        if self.left.iter().any(|&s| s >= n) {
            return Err(EngineError::CorruptSave("left"));
        }
        if h.deck.len() != dealt_cards(n) {
            return Err(EngineError::CorruptSave("deck size"));
        }
        checked_deal(&h.deck, n)?;
        if h.number == 0 || h.number > self.hands_total() {
            return Err(EngineError::CorruptSave("hand number"));
        }
        if h.board_len != h.street.board_len() {
            return Err(EngineError::CorruptSave("board"));
        }
        if [h.button, h.sb_seat, h.bb_seat].iter().any(|&s| s >= n) {
            return Err(EngineError::CorruptSave("positions"));
        }
        if self.chips_in_play() != self.total_chips() {
            return Err(EngineError::CorruptSave("chip count"));
        }
        let consistent = match self.phase {
            Phase::InHand => h.to_act.is_some_and(|s| s < n) && h.outcome.is_none(),
            Phase::HandOver => h.to_act.is_none() && h.outcome.is_some(),
            Phase::MatchOver => self.result.is_some(),
        };
        if !consistent {
            return Err(EngineError::CorruptSave("phase"));
        }
        Ok(())
    }

    /// Finished hands as `seat` may see them.
    pub fn hand_records(&self, seat: usize) -> Vec<TableHandRecord> {
        self.history.iter().map(|h| h.record_for(seat)).collect()
    }

    /// Everything `seat` is allowed to know; the UI and the bots get only
    /// this. Seats are absolute: the UI turns the table so `me` sits at the
    /// bottom.
    pub fn view(&self, seat: usize) -> TableView {
        let h = &self.hand;
        let shown = h.outcome.as_ref().and_then(|o| o.showdown.as_ref());
        TableView {
            schema: self.schema,
            table_id: self.id.clone(),
            kind: self.kind,
            seats: self.seats.clone(),
            phase: self.phase,
            me: seat,
            hand_number: h.number,
            hands_total: self.hands_total(),
            sb: h.sb,
            bb: h.bb,
            button: h.button,
            sb_seat: h.sb_seat,
            bb_seat: h.bb_seat,
            street: h.street,
            stacks: self.stacks.clone(),
            bets: h.bets.clone(),
            pot: h.pot,
            dealt: h.dealt.clone(),
            folded: h.folded.clone(),
            left: (0..self.n()).map(|s| self.left.contains(&s)).collect(),
            board: h.board().to_vec(),
            hole: h
                .dealt
                .get(seat)
                .copied()
                .unwrap_or(false)
                .then(|| h.hole(seat)),
            shown: (0..self.n())
                .map(|s| shown.and_then(|v| v[s].map(|x| x.hole)))
                .collect(),
            to_act: h.to_act,
            to_call: h.max_bet() - h.bets.get(seat).copied().unwrap_or(0),
            legal: self.legal_actions(seat),
            actions: h.actions.clone(),
            outcome: h.outcome.clone(),
            is_final_hand: self.is_final_hand(),
            next_blinds: (!self.is_final_hand()).then(|| table_blinds(h.number + 1, self.n())),
            result: self.result.clone(),
            history: self.hand_records(seat),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableView {
    pub schema: u32,
    pub table_id: String,
    pub kind: TableKind,
    pub seats: Vec<SeatKind>,
    pub phase: Phase,
    pub me: usize,
    pub hand_number: u8,
    pub hands_total: u8,
    pub sb: u32,
    pub bb: u32,
    pub button: usize,
    pub sb_seat: usize,
    pub bb_seat: usize,
    pub street: Street,
    /// All per-seat lists are indexed by absolute seat.
    pub stacks: Vec<u32>,
    pub bets: Vec<u32>,
    pub pot: u32,
    pub dealt: Vec<bool>,
    pub folded: Vec<bool>,
    pub left: Vec<bool>,
    pub board: Vec<Card>,
    /// `None` when `me` was not dealt in (busted or left).
    pub hole: Option<[Card; 2]>,
    /// Other seats' cards, only after they were shown down.
    pub shown: Vec<Option<[Card; 2]>>,
    pub to_act: Option<usize>,
    pub to_call: u32,
    pub legal: Option<LegalActions>,
    pub actions: Vec<TableAction>,
    pub outcome: Option<TableOutcome>,
    pub is_final_hand: bool,
    pub next_blinds: Option<(u32, u32)>,
    pub result: Option<TableResult>,
    pub history: Vec<TableHandRecord>,
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
