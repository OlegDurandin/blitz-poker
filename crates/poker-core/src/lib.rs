//! Blitz Poker Solo rules engine: cards, evaluation and the heads-up match
//! state machine. No UI code computes any of this on its own.

pub mod card;
pub mod engine;
pub mod eval;
pub mod rng;

pub use card::{Card, Suit};
pub use engine::{
    ActionKind, ActionRecord, EngineError, HANDS_PER_MATCH, HandOutcome, HandSummary, LegalActions,
    Match, MatchKind, MatchOutcome, MatchResult, Phase, PlayerAction, PlayerView, RaiseKind,
    SCHEMA_VERSION, STARTING_STACK, Seat, Street, TOTAL_CHIPS,
};
pub use eval::{BestHand, Category, HandValue, best_hand, hand_value};
