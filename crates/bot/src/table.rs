//! The same five bots at a table of three or more ("Третий не лишний").
//!
//! A bot sees only the [`TableView`] for its own seat. With two players
//! dealt in, the table is a duel and the heads-up strategy plays it
//! unchanged. With more:
//! - Preflop: the opening range tightens with every player still to act
//!   behind; limpers are isolated or overlimped; raises are answered by
//!   Monte Carlo equity against every remaining player's range.
//! - Postflop: equity is measured against all remaining players; bet and
//!   raise thresholds use it rescaled to a heads-up scale (`strength`),
//!   calls compare the real equity with pot odds, and bluffs get rarer with
//!   every extra opponent.

use super::{
    ITERATIONS, Personality, Read, Situation, Style, all_in, decide, p, passive, play_postflop,
    preflop_equity, push_share, raise_to, sanitize, strong_draw, style, top_share,
};
use poker_core::card::full_deck;
use poker_core::rng::Rng;
use poker_core::table::{TableAction, TableView};
use poker_core::{
    ActionKind, ActionRecord, Card, LegalActions, MatchKind, Opponent, PlayerAction, PlayerView,
    Seat, Street, hand_value,
};

/// Picks a legal action for `view.me`. Deterministic for a given view and seed.
pub fn decide_table(view: &TableView, opponent: Opponent, seed: u64) -> PlayerAction {
    let Some(legal) = view.legal.as_ref() else {
        return PlayerAction::Check;
    };
    let Some(hole) = view.hole else {
        return passive(legal);
    };
    if view.dealt.iter().filter(|&&d| d).count() == 2 {
        return decide(
            &heads_up_view(view, hole),
            Personality::Standard(opponent),
            seed,
        );
    }
    let st = style(opponent);
    let mut rng = Rng::new(seed);
    let spot = Spot::new(view);
    let action = if view.street == Street::Preflop {
        preflop(view, hole, &spot, &st, legal, &mut rng)
    } else {
        postflop(view, hole, &spot, &st, legal, &mut rng)
    };
    sanitize(legal, action)
}

/// A table with two players dealt in, as the duel engine would show it to
/// the bot (seat `Bot`); the other dealt seat becomes `Human`.
fn heads_up_view(v: &TableView, hole: [Card; 2]) -> PlayerView {
    let me = v.me;
    let opp = (0..v.dealt.len())
        .find(|&s| s != me && v.dealt[s])
        .expect("two seats dealt in");
    let seat = |s: usize| if s == me { Seat::Bot } else { Seat::Human };
    let pair = |x: &[u32]| [x[opp], x[me]];
    PlayerView {
        schema: 1,
        match_id: v.table_id.clone(),
        kind: MatchKind::Standard,
        opponent: Opponent::Alex,
        phase: v.phase,
        me: Seat::Bot,
        hand_number: v.hand_number,
        hands_total: v.hands_total,
        sb: v.sb,
        bb: v.bb,
        button: seat(v.button),
        street: v.street,
        stacks: pair(&v.stacks),
        bets: pair(&v.bets),
        pot: v.pot,
        board: v.board.clone(),
        hole,
        opponent_hole: None,
        to_act: v.to_act.map(seat),
        to_call: v.to_call,
        legal: v.legal.clone(),
        actions: v
            .actions
            .iter()
            .filter(|a| a.seat == me || a.seat == opp)
            .map(|a| ActionRecord {
                seat: seat(a.seat),
                street: a.street,
                kind: a.kind,
                amount: a.amount,
                to: a.to,
                all_in: a.all_in,
            })
            .collect(),
        outcome: None,
        is_final_hand: v.is_final_hand,
        next_blinds: v.next_blinds,
        result: None,
        history: Vec::new(),
    }
}

fn aggressive(a: &TableAction) -> bool {
    matches!(a.kind, ActionKind::Bet | ActionKind::Raise)
}

fn voluntary(a: &TableAction) -> bool {
    matches!(
        a.kind,
        ActionKind::Check
            | ActionKind::Call
            | ActionKind::Bet
            | ActionKind::Raise
            | ActionKind::Fold
    )
}

/// The table from the bot's seat.
struct Spot {
    s: Situation,
    /// Players still in the hand besides the bot.
    opponents: Vec<usize>,
    /// Of those, players with chips who have not acted on this street yet.
    behind: usize,
    /// Players who just called the big blind preflop.
    limpers: usize,
    /// Players who called the current bet or raise on this street.
    callers: usize,
}

impl Spot {
    fn new(v: &TableView) -> Spot {
        let me = v.me;
        let n = v.dealt.len();
        let opponents: Vec<usize> = (0..n)
            .filter(|&s| s != me && v.dealt[s] && !v.folded[s])
            .collect();
        let street: Vec<&TableAction> = v.actions.iter().filter(|a| a.street == v.street).collect();
        let acted = |s: usize| street.iter().any(|a| a.seat == s && voluntary(a));
        let behind = opponents
            .iter()
            .filter(|&&s| v.stacks[s] > 0 && !acted(s))
            .count();
        let raises = street.iter().filter(|a| aggressive(a)).count();
        let last_raise = street.iter().rposition(|a| aggressive(a));
        let callers = street
            .iter()
            .skip(last_raise.map_or(0, |i| i + 1))
            .filter(|a| a.kind == ActionKind::Call && a.seat != me)
            .count();
        let limpers = if v.street == Street::Preflop && raises == 0 {
            callers
        } else {
            0
        };
        let pot_total = v.pot + v.bets.iter().sum::<u32>();
        let to_call = v.to_call.min(v.stacks[me]);
        let deepest = opponents
            .iter()
            .map(|&s| v.stacks[s] + v.bets[s])
            .max()
            .unwrap_or(0);
        let effective = (v.stacks[me] + v.bets[me]).min(deepest);
        let last_aggressor_before = v
            .actions
            .iter()
            .rev()
            .filter(|a| a.street < v.street)
            .find(|a| aggressive(a))
            .map(|a| a.seat);
        Spot {
            s: Situation {
                pot_total,
                max_bet: v.bets.iter().copied().max().unwrap_or(0),
                to_call,
                pot_odds: if to_call == 0 {
                    0.0
                } else {
                    to_call as f64 / (pot_total + to_call) as f64
                },
                effective_bb: effective as f64 / v.bb.max(1) as f64,
                bb: v.bb,
                raises,
                opponent_checked: street
                    .iter()
                    .any(|a| a.seat != me && a.kind == ActionKind::Check),
                was_aggressor: last_aggressor_before == Some(me),
            },
            opponents,
            behind,
            limpers,
            callers,
        }
    }
}

/// Fraction of starting hands `seat` is still assumed to hold, from how it
/// played this hand: every bet or raise narrows it, as does entering the
/// pot voluntarily or going all-in.
fn seat_range(v: &TableView, seat: usize) -> f64 {
    let mine = || v.actions.iter().filter(move |a| a.seat == seat);
    let aggression = mine().filter(|a| aggressive(a)).count();
    let base: f64 = match aggression {
        0 => 1.0,
        1 => 0.55,
        2 => 0.35,
        _ => 0.22,
    };
    let looseness = (aggression_frequency(v, seat) / 0.3).clamp(1.0, 3.0);
    let mut range = base.powf(1.0 / looseness);
    let entered = mine().any(|a| a.street == Street::Preflop && a.kind == ActionKind::Call);
    if entered {
        // Limping or calling in a multiway pot: not the bottom of the deck.
        range = range.min(0.7);
    }
    if mine()
        .any(|a| a.all_in && a.kind != ActionKind::BigBlind && a.kind != ActionKind::SmallBlind)
    {
        range = range.min(0.4);
    }
    range
}

/// Share of `seat`'s voluntary actions this match that were bets or raises,
/// shrunk towards a typical 0.3 while the sample is small.
fn aggression_frequency(v: &TableView, seat: usize) -> f64 {
    let past = v.history.iter().flat_map(|h| h.actions.iter());
    let (mut aggr, mut vol) = (0.0, 0.0);
    for a in past.chain(v.actions.iter()).filter(|a| a.seat == seat) {
        if aggressive(a) {
            aggr += 1.0;
        }
        if voluntary(a) {
            vol += 1.0;
        }
    }
    const PRIOR_WEIGHT: f64 = 8.0;
    (aggr + 0.3 * PRIOR_WEIGHT) / (vol + PRIOR_WEIGHT)
}

/// Equity against several players, each holding a hand from the top
/// `ranges[i]` of possible holdings; ties share the pot.
fn equity_vs(
    hole: [Card; 2],
    board: &[Card],
    ranges: &[f64],
    iterations: usize,
    rng: &mut Rng,
) -> f64 {
    let known: Vec<Card> = hole.iter().chain(board).copied().collect();
    let deck: Vec<Card> = full_deck()
        .into_iter()
        .filter(|c| !known.contains(c))
        .collect();
    let mut combos: Vec<(f64, Card, Card)> = Vec::with_capacity(deck.len() * deck.len() / 2);
    for i in 0..deck.len() {
        for j in i + 1..deck.len() {
            combos.push((preflop_equity(deck[i], deck[j]), deck[i], deck[j]));
        }
    }
    combos.sort_by(|x, y| y.0.total_cmp(&x.0));
    let keep: Vec<usize> = ranges
        .iter()
        .map(|r| ((combos.len() as f64 * r).ceil() as usize).clamp(1, combos.len()))
        .collect();

    let missing = 5 - board.len();
    let mut total = 0.0;
    let mut used: Vec<Card> = Vec::with_capacity(2 * ranges.len());
    let mut rest = Vec::with_capacity(deck.len());
    let mut cards = Vec::with_capacity(7);
    let mut done = 0;
    for _ in 0..iterations * 2 {
        if done == iterations {
            break;
        }
        used.clear();
        let mut ok = true;
        for &k in &keep {
            // A few tries for a hand that doesn't collide with the others.
            let pick = (0..12).find_map(|_| {
                let (_, a, b) = combos[rng.below(k as u64) as usize];
                (!used.contains(&a) && !used.contains(&b)).then_some((a, b))
            });
            match pick {
                Some((a, b)) => used.extend([a, b]),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        done += 1;
        rest.clear();
        rest.extend(deck.iter().copied().filter(|c| !used.contains(c)));
        for k in 0..missing {
            let j = k + rng.below((rest.len() - k) as u64) as usize;
            rest.swap(k, j);
        }
        cards.clear();
        cards.extend_from_slice(&hole);
        cards.extend_from_slice(board);
        cards.extend_from_slice(&rest[..missing]);
        let mine = hand_value(&cards);
        let mut tied = 0;
        let mut lost = false;
        for pair in used.chunks(2) {
            cards[0] = pair[0];
            cards[1] = pair[1];
            match hand_value(&cards).cmp(&mine) {
                std::cmp::Ordering::Greater => {
                    lost = true;
                    break;
                }
                std::cmp::Ordering::Equal => tied += 1,
                std::cmp::Ordering::Less => {}
            }
        }
        if !lost {
            total += 1.0 / (tied + 1) as f64;
        }
    }
    if done == 0 { 0.0 } else { total / done as f64 }
}

fn felt_equity(v: &TableView, hole: [Card; 2], spot: &Spot, st: &Style, rng: &mut Rng) -> f64 {
    let ranges: Vec<f64> = spot.opponents.iter().map(|&s| seat_range(v, s)).collect();
    let e = equity_vs(hole, &v.board, &ranges, ITERATIONS, rng);
    if st.noise > 0.0 {
        (e + (rng.unit() * 2.0 - 1.0) * st.noise).clamp(0.0, 1.0)
    } else {
        e
    }
}

/// Equity against `opponents` players rescaled to one opponent: winning
/// 45% against two players is about as strong as 67% against one.
fn strength(equity: f64, opponents: usize) -> f64 {
    equity.powf(1.0 / opponents.max(1) as f64)
}

/// Opening share for a seat with `behind` players still to act: the small
/// blind (one behind) opens the duel range, every extra player cuts it.
fn open_share(open: f64, behind: usize) -> f64 {
    open * 0.68f64.powi(behind.saturating_sub(1) as i32)
}

fn preflop(
    v: &TableView,
    hole: [Card; 2],
    spot: &Spot,
    st: &Style,
    legal: &LegalActions,
    rng: &mut Rng,
) -> PlayerAction {
    let s = &spot.s;
    let top = top_share(hole);
    let eff = s.effective_bb;
    let shove = all_in(legal);
    let bb = s.bb as f64;

    // Nobody has raised yet.
    if s.raises == 0 {
        if eff <= 10.0 {
            let push =
                (push_share(eff) * st.push * 0.8f64.powi(spot.behind.saturating_sub(1) as i32))
                    .min(0.9);
            return match shove {
                Some(a) if top <= push && spot.limpers == 0 => a,
                Some(a) if top <= push * 0.6 => a,
                _ => passive_or_fold(legal),
            };
        }
        let open = open_share(
            if eff > 16.0 {
                st.open_deep
            } else {
                st.open_mid
            },
            spot.behind,
        );
        let size = (2.5 + spot.limpers as f64) * bb * st.size.max(0.8);
        if legal.check {
            // Big blind after limps: raise strong hands, otherwise see a flop.
            if top <= (0.12 * st.raise).min(0.4) || rng.chance(p(0.03, st.bluff)) {
                return raise_to(legal, (size * 1.4) as u32).unwrap_or(PlayerAction::Check);
            }
            return PlayerAction::Check;
        }
        if spot.limpers == 0 {
            if top <= open {
                if st.limp > 0.0 && rng.chance(st.limp * 0.7) {
                    return PlayerAction::Call;
                }
                return raise_to(legal, size as u32).unwrap_or(PlayerAction::Call);
            }
            // The small blind may complete a little wider; the worst hands fold.
            let completing = s.to_call * 2 <= s.bb;
            if completing && top <= (open + 0.15).min(0.9) && rng.chance(0.5) {
                return PlayerAction::Call;
            }
            return PlayerAction::Fold;
        }
        // Limpers in front: isolate the best hands, overlimp playable ones.
        if top <= open * 0.5 {
            return raise_to(legal, size as u32).unwrap_or(PlayerAction::Call);
        }
        if top <= (open + 0.05 + st.limp * 0.15).min(0.85) {
            return PlayerAction::Call;
        }
        return PlayerAction::Fold;
    }

    // Facing a raise.
    let equity = felt_equity(v, hole, spot, st, rng);
    let stack = v.stacks[v.me];
    let committing = legal.call_is_all_in || s.to_call as f64 >= 0.35 * stack as f64;
    if committing {
        if equity + st.call_bonus / 2.0 >= s.pot_odds + 0.02 {
            return match shove {
                Some(a) if top <= 0.03 => a,
                _ => PlayerAction::Call,
            };
        }
        return PlayerAction::Fold;
    }
    let in_big_blind = v.bb_seat == v.me && v.bets[v.me] == v.bb;
    if s.raises == 1 {
        let heads_up_raise = spot.opponents.len() == 1;
        let bluff_3bet =
            heads_up_raise && (0.3..0.5).contains(&top) && rng.chance(p(0.04, st.bluff));
        if top <= st.three_bet * 0.7 || bluff_3bet {
            let three_bet = if eff <= 20.0 {
                shove
            } else {
                raise_to(legal, s.max_bet * (3 + spot.callers as u32))
            };
            if let Some(a) = three_bet {
                return a;
            }
        }
        let defend = if in_big_blind { 0.55 } else { 0.3 };
        let defend = (defend + st.call_bonus * 2.0).clamp(0.2, 1.0);
        if top > defend {
            return PlayerAction::Fold;
        }
        return if equity + 0.02 + st.call_bonus >= s.pot_odds {
            PlayerAction::Call
        } else {
            PlayerAction::Fold
        };
    }
    if top <= (st.three_bet * 0.35).max(0.02) {
        return shove.unwrap_or(PlayerAction::Call);
    }
    if top <= 0.12 + st.call_bonus * 2.0 && equity + st.call_bonus >= s.pot_odds + 0.02 {
        return PlayerAction::Call;
    }
    PlayerAction::Fold
}

fn passive_or_fold(legal: &LegalActions) -> PlayerAction {
    if legal.check {
        PlayerAction::Check
    } else {
        PlayerAction::Fold
    }
}

fn postflop(
    v: &TableView,
    hole: [Card; 2],
    spot: &Spot,
    st: &Style,
    legal: &LegalActions,
    rng: &mut Rng,
) -> PlayerAction {
    let equity = felt_equity(v, hole, spot, st, rng);
    let opponents = spot.opponents.len();
    let read = Read {
        street: v.street,
        equity,
        strength: strength(equity, opponents),
        draw: v.street != Street::River && strong_draw(hole, &v.board),
        bluff_scale: 0.4f64.powi(opponents.saturating_sub(1) as i32),
    };
    play_postflop(&read, &spot.s, st, legal, rng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::table::{SeatKind, Table};

    fn deck(holes: &[&str], board: &str) -> Vec<Card> {
        let parse = |s: &str| -> Vec<Card> {
            s.split_whitespace()
                .map(|c| Card::parse(c).unwrap())
                .collect()
        };
        let holes: Vec<Vec<Card>> = holes.iter().map(|h| parse(h)).collect();
        let mut d: Vec<Card> = holes.iter().map(|h| h[0]).collect();
        d.extend(holes.iter().map(|h| h[1]));
        d.extend(parse(board));
        for c in full_deck() {
            if !d.contains(&c) {
                d.push(c);
            }
        }
        d
    }

    fn table(holes: &[&str], board: &str, button: usize) -> Table {
        Table::online(
            "t".into(),
            vec![SeatKind::Player; holes.len()],
            button,
            &deck(holes, board),
        )
        .unwrap()
    }

    /// Share of 200 decisions of each style that match `pred`.
    fn share(t: &Table, seat: usize, pred: impl Fn(PlayerAction) -> bool) -> Vec<(Opponent, f64)> {
        let view = t.view(seat);
        Opponent::ALL
            .iter()
            .map(|&o| {
                let hits = (0..200u64)
                    .filter(|&seed| pred(decide_table(&view, o, seed * 31 + 7)))
                    .count();
                (o, hits as f64 / 200.0)
            })
            .collect()
    }

    #[test]
    fn multiway_equity_is_lower_than_heads_up() {
        let hole = [Card::parse("Kh").unwrap(), Card::parse("Qh").unwrap()];
        let mut rng = Rng::new(3);
        let one = equity_vs(hole, &[], &[1.0], 3000, &mut rng);
        let three = equity_vs(hole, &[], &[1.0, 1.0, 1.0], 3000, &mut rng);
        assert!((0.6..0.7).contains(&one), "{one}");
        assert!((0.3..0.45).contains(&three), "{three}");
        let aces = [Card::parse("As").unwrap(), Card::parse("Ad").unwrap()];
        assert!(equity_vs(aces, &[], &[1.0, 1.0], 2000, &mut rng) > 0.7);
    }

    #[test]
    fn early_seat_folds_trash_and_raises_aces() {
        // Four seats, button 3: seat 0 is the small blind, 1 the big blind,
        // seat 2 opens with three players behind.
        let trash = table(&["Ks Kd", "Qs Qd", "7c 2d", "Js Jd"], "Ah 9s 4c 3h 2s", 3);
        assert_eq!(trash.hand.to_act, Some(2));
        for (o, folds) in share(&trash, 2, |a| a == PlayerAction::Fold) {
            assert!(folds >= 0.95, "{o:?} folds 7-2 only {folds}");
        }
        let aces = table(&["Ks Kd", "Qs Qd", "As Ad", "Js Jd"], "Ah 9s 4c 3h 2s", 3);
        for (o, plays) in share(&aces, 2, |a| a != PlayerAction::Fold) {
            assert_eq!(plays, 1.0, "{o:?} folds aces");
        }
        // Passive styles may limp them; the others raise.
        for (o, raises) in share(&aces, 2, |a| matches!(a, PlayerAction::RaiseTo { .. })) {
            if matches!(o, Opponent::Alex | Opponent::Tight | Opponent::Aggressor) {
                assert!(raises >= 0.9, "{o:?} raises aces only {raises}");
            }
        }
    }

    #[test]
    fn bet_into_two_players_is_mostly_folded_without_a_hand() {
        let mut t = table(&["7c 2d", "Ks Kd", "As Qd"], "Ah Qs 9c 4d 3h", 0);
        // Seat 0 (button) calls, seat 1 completes, seat 2 checks.
        t.apply(0, PlayerAction::Call).unwrap();
        t.apply(1, PlayerAction::Call).unwrap();
        t.apply(2, PlayerAction::Check).unwrap();
        // Flop: seat 1 bets the pot, seat 2 calls; seat 0 has nothing.
        t.apply(1, PlayerAction::RaiseTo { amount: 60 }).unwrap();
        t.apply(2, PlayerAction::Call).unwrap();
        assert_eq!(t.hand.to_act, Some(0));
        for (o, folds) in share(&t, 0, |a| a == PlayerAction::Fold) {
            assert!(folds >= 0.8, "{o:?} folds air only {folds}");
        }
    }

    #[test]
    fn heads_up_tables_play_like_the_duel_bot() {
        let t = table(&["7c 2d", "Ah Kh"], "Qd 9s 4c 3h 2s", 0);
        let view = t.view(1);
        for o in Opponent::ALL {
            for seed in 0..20 {
                let duel = decide(
                    &heads_up_view(&view, view.hole.unwrap()),
                    Personality::Standard(o),
                    seed,
                );
                assert_eq!(decide_table(&view, o, seed), duel);
            }
        }
    }

    #[test]
    fn bots_finish_random_tables_with_legal_actions() {
        for seed in 0..12u64 {
            let n = 3 + (seed as usize % 3);
            let seats = (0..n)
                .map(|i| SeatKind::Bot {
                    opponent: Opponent::ALL[(i + seed as usize) % 5],
                })
                .collect();
            let mut t = Table::solo(seed, seats).unwrap();
            let mut guard = 0;
            while t.phase != poker_core::Phase::MatchOver {
                guard += 1;
                assert!(guard < 2000);
                if t.phase == poker_core::Phase::HandOver {
                    t.next_hand().unwrap();
                    continue;
                }
                let s = t.hand.to_act.unwrap();
                let SeatKind::Bot { opponent } = t.seats[s] else {
                    unreachable!()
                };
                let a = decide_table(&t.view(s), opponent, t.decision_seed());
                t.apply(s, a)
                    .unwrap_or_else(|e| panic!("{opponent:?} {a:?}: {e:?}"));
            }
        }
    }
}
