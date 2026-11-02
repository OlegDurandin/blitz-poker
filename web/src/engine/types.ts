// Mirrors the serde output of crates/poker-core. The UI renders these values
// and never derives game rules (pot, legality, winners) on its own.

export type Seat = 'human' | 'bot';
export type Street = 'preflop' | 'flop' | 'turn' | 'river';
export type Phase = 'in_hand' | 'hand_over' | 'match_over';
export type MatchKind = 'standard' | 'tutorial';
/** Two characters: rank `23456789TJQKA` + suit `shdc`, e.g. "As", "Td". */
export type Card = string;

export type Category =
  | 'high_card'
  | 'pair'
  | 'two_pair'
  | 'three_of_a_kind'
  | 'straight'
  | 'flush'
  | 'full_house'
  | 'four_of_a_kind'
  | 'straight_flush'
  | 'royal_flush';

export type PlayerAction =
  | { type: 'fold' }
  | { type: 'check' }
  | { type: 'call' }
  | { type: 'raise_to'; amount: number };

export type ActionKind =
  | 'small_blind'
  | 'big_blind'
  | 'fold'
  | 'check'
  | 'call'
  | 'bet'
  | 'raise'
  | 'return';

export interface ActionRecord {
  seat: Seat;
  street: Street;
  kind: ActionKind;
  amount: number;
  to: number;
  all_in: boolean;
}

export type PresetKind = 'half_pot' | 'pot' | 'all_in';

export interface Preset {
  kind: PresetKind;
  to: number;
  enabled: boolean;
}

export interface RaiseOption {
  kind: 'bet' | 'raise';
  min_to: number;
  max_to: number;
  step: number;
  presets: Preset[];
}

export interface LegalActions {
  fold: boolean;
  check: boolean;
  call: number | null;
  call_is_all_in: boolean;
  raise: RaiseOption | null;
}

export interface ShownHand {
  hole: [Card, Card];
  category: Category;
  best: Card[];
}

export interface HandOutcome {
  winner: Seat | null;
  by_fold: boolean;
  ended_on: Street;
  pot: number;
  awarded: [number, number];
  net: [number, number];
  showdown: [ShownHand, ShownHand] | null;
  runout_from: number | null;
}

export interface HandSummary {
  number: number;
  button: Seat;
  sb: number;
  bb: number;
  board: Card[];
  actions: ActionRecord[];
  outcome: HandOutcome;
}

export interface MatchResult {
  outcome: 'win' | 'loss' | 'draw';
  final_stacks: [number, number];
  hands_played: number;
  hands_won: [number, number];
  ended_early: boolean;
  best_hand: { category: Category; cards: Card[] } | null;
}

export interface PlayerView {
  schema: number;
  match_id: string;
  kind: MatchKind;
  phase: Phase;
  me: Seat;
  hand_number: number;
  hands_total: number;
  sb: number;
  bb: number;
  button: Seat;
  street: Street;
  /** Indexed `[human, bot]`. */
  stacks: [number, number];
  bets: [number, number];
  pot: number;
  board: Card[];
  hole: [Card, Card];
  opponent_hole: [Card, Card] | null;
  to_act: Seat | null;
  to_call: number;
  legal: LegalActions | null;
  actions: ActionRecord[];
  outcome: HandOutcome | null;
  is_final_hand: boolean;
  next_blinds: [number, number] | null;
  result: MatchResult | null;
  history: HandSummary[];
}

/** Opaque engine snapshot; stored and passed back verbatim. */
export type MatchState = unknown;

export type EngineErrorCode =
  | 'bad_request'
  | 'wrong_phase'
  | 'not_your_turn'
  | 'illegal_action'
  | 'invalid_amount'
  | 'incompatible_save'
  | 'corrupt_save';

export type EngineResult =
  | { ok: true; state: MatchState; view: PlayerView; bot_action?: PlayerAction }
  | { ok: false; error: { code: EngineErrorCode; message: string } };

export const SEAT_INDEX: Record<Seat, 0 | 1> = { human: 0, bot: 1 };
