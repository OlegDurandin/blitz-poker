//! Blitz Poker Solo rules engine: cards, evaluation, the heads-up match
//! state machine and hand history. No UI code computes any of this on its own.

pub mod card;
pub mod engine;
pub mod eval;
pub mod history;
pub mod perspective;
pub mod rng;
pub mod table;

pub use card::{Card, Suit};
pub use engine::{
    ActionKind, ActionRecord, EngineError, HANDS_PER_MATCH, HandOutcome, HandSummary, LegalActions,
    Match, MatchKind, MatchOutcome, MatchResult, Opponent, Phase, PlayerAction, PlayerView,
    RaiseKind, SCHEMA_VERSION, STARTING_STACK, Seat, Street, TOTAL_CHIPS,
};
pub use eval::{BestHand, Category, HandValue, best_hand, hand_value};
pub use history::{
    HISTORY_SCHEMA_VERSION, HandRecord, ReplayError, ReplayEvent, ReplayFrame, replay,
};
pub use table::{
    MAX_SEATS, MIN_SEATS, PotShare, SeatKind, Table, TableKind, TableOutcome, TableResult,
    TableView,
};
