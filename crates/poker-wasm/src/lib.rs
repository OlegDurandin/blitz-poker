//! WASM bridge for the web client.
//!
//! One JSON request in, one JSON response out. The match snapshot (`state`)
//! is opaque to the client: it stores it and passes it back verbatim; the
//! client renders only `view`, the human seat's projection.
//!
//! ABI (no wasm-bindgen needed):
//! 1. `alloc(len)` → pointer; the host writes UTF-8 request bytes there.
//! 2. `call(ptr, len)` → response length; frees the request buffer.
//! 3. Read the response from `out_ptr()`.

use bot::Personality;
use poker_core::{
    EngineError, HISTORY_SCHEMA_VERSION, HandRecord, Match, MatchKind, Opponent, PlayerAction,
    PlayerView, ReplayFrame, Seat, replay,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Request {
    /// `seed` is a 64-bit hex string (JSON numbers can't carry 64 bits).
    NewMatch {
        seed: String,
        /// Defaults to Alex for clients from before 1.2.
        #[serde(default)]
        opponent: Opponent,
    },
    NewTutorial,
    /// Load and validate a stored snapshot.
    View {
        state: Box<Match>,
    },
    Act {
        state: Box<Match>,
        action: PlayerAction,
    },
    BotAct {
        state: Box<Match>,
    },
    Next {
        state: Box<Match>,
    },
    /// The human's hand records for archiving, including an unfinished hand.
    History {
        state: Box<Match>,
    },
    /// Step-by-step public table states of one archived hand.
    Replay {
        hand: Box<HandRecord>,
    },
}

#[derive(Serialize)]
struct Success {
    ok: bool,
    state: Match,
    view: PlayerView,
    #[serde(skip_serializing_if = "Option::is_none")]
    bot_action: Option<PlayerAction>,
}

#[derive(Serialize)]
struct Failure<'a> {
    ok: bool,
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
}

fn fail(code: &str, message: String) -> String {
    serde_json::to_string(&Failure {
        ok: false,
        error: ErrorBody { code, message },
    })
    .expect("error serializes")
}

#[derive(Serialize)]
struct HistoryResponse {
    ok: bool,
    schema: u32,
    hands: Vec<HandRecord>,
}

#[derive(Serialize)]
struct ReplayResponse {
    ok: bool,
    frames: Vec<ReplayFrame>,
}

/// Brings an older compatible snapshot up to date, then checks it.
fn load(state: &mut Match) -> Result<(), EngineError> {
    // Duels are run by the PvP server only; a client never holds one.
    if state.kind == MatchKind::Duel {
        return Err(EngineError::CorruptSave("duels are played on the server"));
    }
    state.upgrade();
    state.validate()
}

fn run(request: Request) -> Result<(Match, Option<PlayerAction>), EngineError> {
    match request {
        Request::NewMatch { seed, opponent } => {
            let seed = u64::from_str_radix(seed.trim_start_matches("0x"), 16)
                .map_err(|_| EngineError::CorruptSave("seed must be hex"))?;
            Ok((Match::against(seed, opponent), None))
        }
        Request::NewTutorial => Ok((Match::new_tutorial(), None)),
        Request::View { mut state } => {
            load(&mut state)?;
            Ok((*state, None))
        }
        Request::Act { mut state, action } => {
            load(&mut state)?;
            state.apply(Seat::Human, action)?;
            Ok((*state, None))
        }
        Request::BotAct { mut state } => {
            load(&mut state)?;
            let personality = match state.kind {
                MatchKind::Standard => Personality::Standard(state.opponent),
                MatchKind::Tutorial => Personality::Tutorial,
                MatchKind::Duel => unreachable!("rejected by load"),
            };
            let action = bot::decide(&state.view(Seat::Bot), personality, state.decision_seed());
            state.apply(Seat::Bot, action)?;
            Ok((*state, Some(action)))
        }
        Request::Next { mut state } => {
            load(&mut state)?;
            state.next_hand()?;
            Ok((*state, None))
        }
        Request::History { .. } | Request::Replay { .. } => unreachable!("handled in `handle`"),
    }
}

/// Pure entry point, usable natively for tests.
pub fn handle(request: &str) -> String {
    let request: Request = match serde_json::from_str(request) {
        Ok(r) => r,
        Err(e) => return fail("bad_request", e.to_string()),
    };
    let request = match request {
        Request::History { mut state } => {
            return match load(&mut state) {
                Ok(()) => serde_json::to_string(&HistoryResponse {
                    ok: true,
                    schema: HISTORY_SCHEMA_VERSION,
                    hands: state.hand_records(Seat::Human, true),
                })
                .expect("history serializes"),
                Err(e) => fail(e.code(), e.to_string()),
            };
        }
        Request::Replay { hand } => {
            return match replay(&hand) {
                Ok(frames) => serde_json::to_string(&ReplayResponse { ok: true, frames })
                    .expect("frames serialize"),
                Err(e) => fail("corrupt_save", e.0.to_string()),
            };
        }
        other => other,
    };
    match run(request) {
        Ok((state, bot_action)) => {
            let view = state.view(Seat::Human);
            serde_json::to_string(&Success {
                ok: true,
                state,
                view,
                bot_action,
            })
            .expect("state serializes")
        }
        Err(e) => fail(e.code(), e.to_string()),
    }
}

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
/// `ptr`/`len` must come from a previous `alloc(len)` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// # Safety
/// `ptr` must come from `alloc(len)` and hold `len` initialized bytes. The
/// buffer is consumed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn call(ptr: *mut u8, len: usize) -> usize {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let response = match std::str::from_utf8(&bytes) {
        Ok(text) => handle(text),
        Err(e) => fail("bad_request", e.to_string()),
    };
    OUT.with(|out| {
        let mut out = out.borrow_mut();
        *out = response.into_bytes();
        out.len()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.with(|out| out.borrow().as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn send(req: Value) -> Value {
        serde_json::from_str(&handle(&req.to_string())).unwrap()
    }

    #[test]
    fn plays_a_match_through_json() {
        let mut res = send(json!({"cmd": "new_match", "seed": "00ff00ff00ff00ff"}));
        assert_eq!(res["ok"], true);
        let mut guard = 0;
        while res["view"]["phase"] != "match_over" {
            guard += 1;
            assert!(guard < 400);
            let state = res["state"].clone();
            let view = &res["view"];
            res = if view["phase"] == "hand_over" {
                send(json!({"cmd": "next", "state": state}))
            } else if view["to_act"] == "bot" {
                let r = send(json!({"cmd": "bot_act", "state": state}));
                assert!(r["bot_action"].is_object());
                r
            } else {
                let action = if view["legal"]["check"] == true {
                    "check"
                } else {
                    "call"
                };
                send(json!({"cmd": "act", "state": state, "action": {"type": action}}))
            };
            assert_eq!(res["ok"], true, "{res}");
        }
        assert!(res["view"]["result"]["outcome"].is_string());
    }

    #[test]
    fn rejects_bad_input_with_codes() {
        let res = send(json!({"cmd": "nope"}));
        assert_eq!(res["error"]["code"], "bad_request");

        let start = send(json!({"cmd": "new_tutorial"}));
        let r = send(
            json!({"cmd": "act", "state": start["state"], "action": {"type": "raise_to", "amount": 5}}),
        );
        assert_eq!(r["error"]["code"], "invalid_amount");
        let r = send(json!({"cmd": "bot_act", "state": start["state"]}));
        assert_eq!(r["error"]["code"], "not_your_turn");

        let mut old = start["state"].clone();
        old["schema"] = json!(0);
        assert_eq!(
            send(json!({"cmd": "view", "state": old}))["error"]["code"],
            "incompatible_save"
        );
        let mut cheat = start["state"].clone();
        cheat["stacks"] = json!([5000, 600]);
        assert_eq!(
            send(json!({"cmd": "view", "state": cheat}))["error"]["code"],
            "corrupt_save"
        );
    }

    #[test]
    fn history_and_replay_through_json() {
        let start = send(json!({"cmd": "new_match", "seed": "0000000000000042"}));
        let state = start["state"].clone();
        let fold = if start["view"]["to_act"] == "human" {
            send(json!({"cmd": "act", "state": state, "action": {"type": "fold"}}))
        } else {
            send(json!({"cmd": "bot_act", "state": state}))
        };
        let next = send(json!({"cmd": "next", "state": fold["state"]}));
        let h = send(json!({"cmd": "history", "state": next["state"]}));
        assert_eq!(h["ok"], true, "{h}");
        assert_eq!(h["schema"], 1);
        let hands = h["hands"].as_array().unwrap();
        assert_eq!(hands.len(), 2, "finished hand 1 plus hand 2 in progress");
        assert!(hands[0]["outcome"].is_object());
        assert!(hands[1]["outcome"].is_null());

        let r = send(json!({"cmd": "replay", "hand": hands[0]}));
        assert_eq!(r["ok"], true, "{r}");
        let frames = r["frames"].as_array().unwrap();
        assert_eq!(frames[0]["event"]["type"], "start");
        assert_eq!(frames.last().unwrap()["event"]["type"], "award");

        let mut broken = hands[0].clone();
        broken["start_stacks"] = json!([1, 1]);
        let r = send(json!({"cmd": "replay", "hand": broken}));
        assert_eq!(r["error"]["code"], "corrupt_save");

        // A 1.0 save: history entries without start_stacks / holes.
        let mut old = next["state"].clone();
        for entry in old["history"].as_array_mut().unwrap() {
            entry.as_object_mut().unwrap().remove("start_stacks");
            entry.as_object_mut().unwrap().remove("holes");
        }
        let loaded = send(json!({"cmd": "view", "state": old}));
        assert_eq!(loaded["ok"], true, "{loaded}");
        assert_eq!(
            loaded["view"]["history"][0]["start_stacks"],
            json!([600, 600])
        );
        assert!(loaded["view"]["history"][0]["hole"].is_null());
    }

    #[test]
    fn new_match_records_the_opponent() {
        let res =
            send(json!({"cmd": "new_match", "seed": "0000000000000007", "opponent": "tight"}));
        assert_eq!(res["view"]["opponent"], "tight");
        let res = send(json!({"cmd": "new_match", "seed": "0000000000000007"}));
        assert_eq!(
            res["view"]["opponent"], "alex",
            "older clients send no opponent"
        );
        let bad = send(json!({"cmd": "new_match", "seed": "07", "opponent": "robot"}));
        assert_eq!(bad["error"]["code"], "bad_request");
    }

    #[test]
    fn human_view_hides_bot_cards() {
        let res = send(json!({"cmd": "new_tutorial"}));
        assert_eq!(res["view"]["hole"], json!(["As", "Kc"]));
        assert!(res["view"]["opponent_hole"].is_null());
        assert_eq!(res["view"]["kind"], "tutorial");
    }
}
