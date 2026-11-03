//! Hand history: what one seat may see of a hand, and a step-by-step replay.
//!
//! The engine keeps a private [`HandSummary`] per finished hand (both players'
//! hole cards included). Everything that leaves the engine is a [`HandRecord`]
//! projected for one seat: own hole cards always, the opponent's only if they
//! were shown down. [`replay`] rebuilds the public table state after every
//! recorded event from that projection alone, so a replay can never reveal
//! more than the original hand did.

use crate::card::Card;
use crate::engine::{
    ActionKind, ActionRecord, HandOutcome, HandSummary, Match, Phase, Seat, ShownHand, Street,
};
use serde::{Deserialize, Serialize};

/// Bumped when the shape of [`HandRecord`] changes incompatibly.
pub const HISTORY_SCHEMA_VERSION: u32 = 1;

/// One hand as a given seat is allowed to see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandRecord {
    pub number: u8,
    pub button: Seat,
    pub sb: u32,
    pub bb: u32,
    /// `[human, bot]` before the blinds.
    pub start_stacks: [u32; 2],
    /// The viewer's own hole cards; `None` for hands saved by 1.0.
    pub hole: Option<[Card; 2]>,
    /// Face-up cards only: a hand that ended on the flop has three.
    pub board: Vec<Card>,
    /// In order of play, blinds and returned uncalled bets included.
    pub actions: Vec<ActionRecord>,
    /// `None` while the hand is unfinished (an abandoned match).
    pub outcome: Option<HandOutcome>,
}

impl HandSummary {
    pub fn record_for(&self, seat: Seat) -> HandRecord {
        HandRecord {
            number: self.number,
            button: self.button,
            sb: self.sb,
            bb: self.bb,
            start_stacks: self.start_stacks,
            hole: self.holes.map(|h| h[seat.idx()]),
            board: self.board.clone(),
            actions: self.actions.clone(),
            outcome: Some(self.outcome.clone()),
        }
    }
}

impl Match {
    /// Finished hands as `seat` sees them, plus the hand in progress when
    /// `include_current` is set and the hand is not over yet.
    pub fn hand_records(&self, seat: Seat, include_current: bool) -> Vec<HandRecord> {
        let mut records: Vec<HandRecord> =
            self.history.iter().map(|h| h.record_for(seat)).collect();
        if include_current && self.phase == Phase::InHand {
            let h = &self.hand;
            records.push(HandRecord {
                number: h.number,
                button: h.button,
                sb: h.sb,
                bb: h.bb,
                start_stacks: h.start_stacks,
                hole: Some(h.hole(seat)),
                board: h.board().to_vec(),
                actions: h.actions.clone(),
                outcome: None,
            });
        }
        records
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplayEvent {
    /// Cards dealt, before the blinds.
    Start,
    Action {
        action: ActionRecord,
    },
    /// Bets collected and the next street's cards turned. `runout` marks
    /// streets dealt with no betting because someone was all-in.
    Street {
        street: Street,
        runout: bool,
    },
    Showdown {
        hands: [ShownHand; 2],
    },
    Award {
        winner: Option<Seat>,
        by_fold: bool,
        pot: u32,
        awarded: [u32; 2],
        net: [i32; 2],
    },
    /// The record stops here: the match was abandoned mid-hand.
    Unfinished,
}

/// Public table state right after `event`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReplayFrame {
    pub event: ReplayEvent,
    pub street: Street,
    pub board: Vec<Card>,
    pub stacks: [u32; 2],
    pub bets: [u32; 2],
    pub pot: u32,
    /// Both hands once they have been shown down; `None` before that and
    /// always after a fold.
    pub showdown: Option<[ShownHand; 2]>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReplayError(pub &'static str);

struct Table {
    street: Street,
    board_len: usize,
    stacks: [u32; 2],
    bets: [u32; 2],
    pot: u32,
    showdown: Option<[ShownHand; 2]>,
}

impl Table {
    fn frame(&self, record: &HandRecord, event: ReplayEvent) -> ReplayFrame {
        ReplayFrame {
            event,
            street: self.street,
            board: record.board[..self.board_len].to_vec(),
            stacks: self.stacks,
            bets: self.bets,
            pot: self.pot,
            showdown: self.showdown,
        }
    }

    fn collect(&mut self) {
        self.pot += self.bets[0] + self.bets[1];
        self.bets = [0; 2];
    }

    /// Collects bets and deals the next street.
    fn advance(&mut self, record: &HandRecord) -> Result<(), ReplayError> {
        if self.street == Street::River {
            return Err(ReplayError("action after the river"));
        }
        self.collect();
        self.street = self.street.next();
        self.board_len = self.street.board_len();
        if record.board.len() < self.board_len {
            return Err(ReplayError("board shorter than the streets played"));
        }
        Ok(())
    }
}

/// Rebuilds the hand event by event. Only chips recorded by the engine are
/// moved; nothing is recomputed from rules, so replay and live play agree by
/// construction (and the tests check that they do).
pub fn replay(record: &HandRecord) -> Result<Vec<ReplayFrame>, ReplayError> {
    let mut t = Table {
        street: Street::Preflop,
        board_len: 0,
        stacks: record.start_stacks,
        bets: [0; 2],
        pot: 0,
        showdown: None,
    };
    let mut frames = vec![t.frame(record, ReplayEvent::Start)];
    let runout = record
        .outcome
        .as_ref()
        .is_some_and(|o| o.runout_from.is_some());

    for a in &record.actions {
        while t.street < a.street {
            t.advance(record)?;
            let event = ReplayEvent::Street {
                street: t.street,
                runout: false,
            };
            frames.push(t.frame(record, event));
        }
        if a.street != t.street {
            return Err(ReplayError("actions out of street order"));
        }
        let i = a.seat.idx();
        if a.kind == ActionKind::Return {
            t.bets[i] = t.bets[i]
                .checked_sub(a.amount)
                .ok_or(ReplayError("return"))?;
            t.stacks[i] += a.amount;
        } else {
            t.stacks[i] = t.stacks[i]
                .checked_sub(a.amount)
                .ok_or(ReplayError("stack"))?;
            t.bets[i] += a.amount;
        }
        if t.bets[i] != a.to {
            return Err(ReplayError("bet total does not match the record"));
        }
        frames.push(t.frame(record, ReplayEvent::Action { action: *a }));
    }

    let Some(outcome) = &record.outcome else {
        // The last action may have closed a street that is already dealt.
        while t.board_len < record.board.len() {
            t.advance(record)?;
            let event = ReplayEvent::Street {
                street: t.street,
                runout: false,
            };
            frames.push(t.frame(record, event));
        }
        frames.push(t.frame(record, ReplayEvent::Unfinished));
        return Ok(frames);
    };

    // Streets dealt after the last action: an all-in runout.
    while t.street < outcome.ended_on {
        t.advance(record)?;
        let event = ReplayEvent::Street {
            street: t.street,
            runout,
        };
        frames.push(t.frame(record, event));
    }
    t.collect();
    if t.pot != outcome.pot || record.board.len() != t.board_len {
        return Err(ReplayError("pot or board does not match the outcome"));
    }
    if let Some(hands) = outcome.showdown {
        t.showdown = Some(hands);
        frames.push(t.frame(record, ReplayEvent::Showdown { hands }));
    }
    for (stack, won) in t.stacks.iter_mut().zip(outcome.awarded) {
        *stack += won;
    }
    t.pot = 0;
    let net = [0, 1].map(|i| t.stacks[i] as i64 - record.start_stacks[i] as i64);
    if net != outcome.net.map(i64::from) {
        return Err(ReplayError("stacks do not match the outcome"));
    }
    frames.push(t.frame(
        record,
        ReplayEvent::Award {
            winner: outcome.winner,
            by_fold: outcome.by_fold,
            pot: outcome.pot,
            awarded: outcome.awarded,
            net: outcome.net,
        },
    ));
    Ok(frames)
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
