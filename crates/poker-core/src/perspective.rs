//! A match seen from either seat as if it were the bottom ("human") seat.
//!
//! The engine indexes everything by [`Seat`]: `Human` is index 0, `Bot` is
//! index 1. In a duel both seats are people, and each client renders itself at
//! the bottom of the table exactly like Solo does. [`Match::view_from`] gives
//! the guest (seat `Bot`) a view in which it is `Human`, with every seat field
//! and `[seat0, seat1]` pair swapped. Hidden information is unchanged: the
//! mirror is applied to [`Match::view`], which already hides the other hand.

use crate::engine::{
    ActionRecord, HandOutcome, Match, MatchOutcome, MatchResult, PlayerView, Seat,
};
use crate::history::HandRecord;

fn swap<T>([a, b]: [T; 2]) -> [T; 2] {
    [b, a]
}

impl ActionRecord {
    fn mirrored(self) -> ActionRecord {
        ActionRecord {
            seat: self.seat.other(),
            ..self
        }
    }
}

impl HandOutcome {
    fn mirrored(self) -> HandOutcome {
        HandOutcome {
            winner: self.winner.map(Seat::other),
            awarded: swap(self.awarded),
            net: swap(self.net),
            showdown: self.showdown.map(swap),
            ..self
        }
    }
}

impl HandRecord {
    fn mirrored(self) -> HandRecord {
        HandRecord {
            button: self.button.other(),
            start_stacks: swap(self.start_stacks),
            actions: self
                .actions
                .into_iter()
                .map(ActionRecord::mirrored)
                .collect(),
            outcome: self.outcome.map(HandOutcome::mirrored),
            ..self
        }
    }
}

impl MatchResult {
    fn mirrored(self) -> MatchResult {
        MatchResult {
            outcome: match self.outcome {
                MatchOutcome::Win => MatchOutcome::Loss,
                MatchOutcome::Loss => MatchOutcome::Win,
                MatchOutcome::Draw => MatchOutcome::Draw,
            },
            final_stacks: swap(self.final_stacks),
            hands_won: swap(self.hands_won),
            best_hand: self.opponent_best_hand,
            opponent_best_hand: self.best_hand,
            forfeit: self.forfeit.map(Seat::other),
            ..self
        }
    }
}

impl PlayerView {
    /// The same view with the two seats exchanged.
    pub fn mirrored(self) -> PlayerView {
        PlayerView {
            me: self.me.other(),
            button: self.button.other(),
            stacks: swap(self.stacks),
            bets: swap(self.bets),
            to_act: self.to_act.map(Seat::other),
            actions: self
                .actions
                .into_iter()
                .map(ActionRecord::mirrored)
                .collect(),
            outcome: self.outcome.map(HandOutcome::mirrored),
            result: self.result.map(MatchResult::mirrored),
            history: self.history.into_iter().map(HandRecord::mirrored).collect(),
            ..self
        }
    }
}

impl Match {
    /// What `seat` may see, presented with `seat` as `Human` (index 0).
    pub fn view_from(&self, seat: Seat) -> PlayerView {
        let view = self.view(seat);
        match seat {
            Seat::Human => view,
            Seat::Bot => view.mirrored(),
        }
    }
}

#[cfg(test)]
#[path = "perspective_tests.rs"]
mod tests;
