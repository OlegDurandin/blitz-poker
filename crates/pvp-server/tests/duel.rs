//! End-to-end: a real server on a random port and artificial clients over
//! WebSocket, signed in with genuinely signed (test-key) VK parameters.

use futures_util::{SinkExt, StreamExt};
use pvp_server::config::Config;
use pvp_server::hub::Hub;
use pvp_server::room::Timings;
use pvp_server::store::Store;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const APP_ID: u64 = 54872714;
const SECRET: &str = "test-secret";
const ORIGIN: &str = "https://prod-app54872714-x.pages-ac.vk-apps.ru";

fn config(db: &str) -> Config {
    Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        app_id: APP_ID,
        app_secret: SECRET.into(),
        db_path: db.into(),
        allowed_origins: vec![".vk-apps.ru".into()],
        launch_max_age_secs: 86_400,
        dev_auth: false,
        guest_auth: true,
        timings: Timings {
            hand_pause_ms: 50,
            runout_pause_ms: 50,
            ..Timings::default()
        },
    }
}

struct Server {
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    ticker: tokio::task::JoinHandle<()>,
}

async fn start(db: &str) -> Server {
    let cfg = config(db);
    let hub = Arc::new(Hub::new(cfg, Store::open(db).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let t = hub.clone();
    let ticker = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            t.tick();
        }
    });
    let task = tokio::spawn(async move {
        axum::serve(listener, pvp_server::server::router(hub))
            .await
            .unwrap();
    });
    Server { addr, task, ticker }
}

impl Server {
    fn stop(self) {
        self.task.abort();
        self.ticker.abort();
    }
}

/// Minimal HTTP/1.1 POST; returns (status, json body).
async fn post(addr: SocketAddr, path: &str, body: &Value, bearer: Option<&str>) -> (u16, Value) {
    let body = body.to_string();
    let auth = bearer
        .map(|b| format!("Authorization: Bearer {b}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{auth}\r\n{body}",
        body.len()
    );
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let status = out[9..12].parse().unwrap();
    let json = out
        .split_once("\r\n\r\n")
        .map(|(_, b)| serde_json::from_str(b).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    (status, json)
}

fn launch(user: u64) -> String {
    pvp_server::vk::signed_query(
        &[
            ("vk_user_id", &user.to_string()),
            ("vk_app_id", &APP_ID.to_string()),
            ("vk_ts", &(pvp_server::hub::now_ms() / 1000).to_string()),
            ("vk_platform", "desktop_web"),
        ],
        SECRET.as_bytes(),
    )
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Client {
    addr: SocketAddr,
    session: String,
    ws: Ws,
    /// Every text frame received.
    seen: Vec<String>,
}

async fn open_ws(addr: SocketAddr, ticket: &str, origin: &str) -> Result<Ws, u16> {
    let mut req = format!("ws://{addr}/ws?ticket={ticket}")
        .into_client_request()
        .unwrap();
    req.headers_mut().insert("Origin", origin.parse().unwrap());
    match tokio_tungstenite::connect_async(req).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => Err(resp.status().as_u16()),
        Err(e) => panic!("{e}"),
    }
}

impl Client {
    async fn login(addr: SocketAddr, user: u64, name: &str) -> Client {
        let (status, body) = post(
            addr,
            "/api/auth",
            &json!({ "launch_params": launch(user), "name": name }),
            None,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let ws = open_ws(addr, body["ticket"].as_str().unwrap(), ORIGIN)
            .await
            .unwrap();
        let mut c = Client {
            addr,
            session: body["session"].as_str().unwrap().into(),
            ws,
            seen: vec![],
        };
        c.until(|m| m["type"] == "welcome").await;
        c
    }

    /// Outside VK: signs in with a browser-kept guest key.
    async fn login_guest(addr: SocketAddr, key: &str, name: &str) -> Client {
        let (status, body) = post(
            addr,
            "/api/auth",
            &json!({ "guest_key": key, "name": name }),
            None,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let ws = open_ws(addr, body["ticket"].as_str().unwrap(), ORIGIN)
            .await
            .unwrap();
        let mut c = Client {
            addr,
            session: body["session"].as_str().unwrap().into(),
            ws,
            seen: vec![],
        };
        c.until(|m| m["type"] == "welcome").await;
        c
    }

    /// Reconnects with a fresh ticket from the session.
    async fn reconnect(&mut self) {
        let (status, body) = post(self.addr, "/api/ticket", &json!({}), Some(&self.session)).await;
        assert_eq!(status, 200);
        self.ws = open_ws(self.addr, body["ticket"].as_str().unwrap(), ORIGIN)
            .await
            .unwrap();
        self.until(|m| m["type"] == "welcome").await;
    }

    async fn send(&mut self, mut msg: Value) {
        msg["v"] = json!(1);
        self.ws
            .send(Message::Text(msg.to_string().into()))
            .await
            .unwrap();
    }

    async fn next(&mut self) -> Value {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), self.ws.next())
                .await
                .expect("a message within 5 s")
                .expect("socket open")
                .expect("frame");
            if let Message::Text(t) = frame {
                self.seen.push(t.to_string());
                return serde_json::from_str(&t).unwrap();
            }
        }
    }

    async fn until(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        loop {
            let m = self.next().await;
            if pred(&m) {
                return m;
            }
        }
    }

    /// Next room snapshot matching `pred`.
    async fn snapshot(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        self.until(|m| m["type"] == "state_snapshot" && pred(&m["room"]))
            .await["room"]
            .clone()
    }
}

async fn room_with_two(addr: SocketAddr) -> (Client, Client, String) {
    let mut host = Client::login(addr, 1001, "Олег").await;
    let mut guest = Client::login(addr, 2002, "Максим").await;
    host.send(json!({ "type": "create_room", "request_id": "r1" }))
        .await;
    let created = host.until(|m| m["type"] == "room_created").await;
    let room_id = created["room_id"].as_str().unwrap().to_string();
    // Retrying create with the same request id yields the same room.
    host.send(json!({ "type": "create_room", "request_id": "r1" }))
        .await;
    assert_eq!(
        host.until(|m| m["type"] == "room_created").await["room_id"],
        room_id.as_str()
    );

    guest
        .send(json!({ "type": "peek_room", "room_id": room_id }))
        .await;
    let preview = guest.until(|m| m["type"] == "room_preview").await;
    assert_eq!(preview["preview"]["host_name"], "Олег");
    assert_eq!(preview["preview"]["joinable"], true);

    guest
        .send(json!({ "type": "join_room", "room_id": room_id }))
        .await;
    guest.snapshot(|r| r["status"] == "ready").await;
    let seen_by_host = host.snapshot(|r| r["status"] == "ready").await;
    assert_eq!(seen_by_host["opponent"]["name"], "Максим");
    (host, guest, room_id)
}

async fn start_match(host: &mut Client, guest: &mut Client, room_id: &str) -> (Value, Value) {
    host.send(json!({ "type": "set_ready", "room_id": room_id, "ready": true }))
        .await;
    guest
        .send(json!({ "type": "set_ready", "room_id": room_id, "ready": true }))
        .await;
    // Ready twice: must not start a second match.
    guest
        .send(json!({ "type": "set_ready", "room_id": room_id, "ready": true }))
        .await;
    let h = host.snapshot(|r| r["status"] == "playing").await;
    let g = guest.snapshot(|r| r["status"] == "playing").await;
    assert_eq!(h["game"]["match_id"], g["game"]["match_id"]);
    (h, g)
}

fn passive(view: &Value) -> Value {
    if view["legal"]["check"] == true {
        json!({ "type": "check" })
    } else {
        json!({ "type": "call" })
    }
}

#[tokio::test]
async fn rejects_forged_identity_and_foreign_origins() {
    let s = start(":memory:").await;
    let forged = launch(1001).replace("vk_user_id=1001", "vk_user_id=1002");
    let (status, body) = post(
        s.addr,
        "/api/auth",
        &json!({ "launch_params": forged }),
        None,
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (401, Some("bad_launch_params"))
    );
    let (status, _) = post(s.addr, "/api/auth", &json!({ "dev_user_id": 5 }), None).await;
    assert_eq!(status, 401, "dev logins are off");
    let (status, _) = post(s.addr, "/api/ticket", &json!({}), Some("not-a-session")).await;
    assert_eq!(status, 401);

    let (_, body) = post(
        s.addr,
        "/api/auth",
        &json!({ "launch_params": launch(7) }),
        None,
    )
    .await;
    let ticket = body["ticket"].as_str().unwrap();
    assert_eq!(
        open_ws(s.addr, ticket, "https://evil.example").await.err(),
        Some(403)
    );
    assert!(open_ws(s.addr, ticket, ORIGIN).await.is_ok());
    assert_eq!(
        open_ws(s.addr, ticket, ORIGIN).await.err(),
        Some(401),
        "tickets are single-use"
    );
    s.stop();
}

/// Two people play a whole match. Neither ever receives the other's hidden
/// cards; a duplicated action is applied once; a third person can't join.
#[tokio::test]
async fn full_duel_between_two_clients() {
    let s = start(":memory:").await;
    let (mut host, mut guest, room_id) = room_with_two(s.addr).await;

    let mut stranger = Client::login(s.addr, 3003, "Третий").await;
    stranger
        .send(json!({ "type": "join_room", "room_id": room_id }))
        .await;
    let err = stranger.until(|m| m["type"] == "server_error").await;
    assert_eq!(err["code"], "room_full");

    let (mut h, mut g) = start_match(&mut host, &mut guest, &room_id).await;
    let mut duplicate_checked = false;
    let mut hands_checked = 0;
    for _ in 0..600 {
        if h["status"] == "finished" && g["status"] == "finished" {
            break;
        }
        // Hidden cards: during a hand the two holes differ and each client's
        // traffic never names the other's cards.
        let (hv, gv) = (&h["game"]["view"], &g["game"]["view"]);
        if hv["phase"] == "in_hand"
            && gv["phase"] == "in_hand"
            && hv["hand_number"] == gv["hand_number"]
        {
            assert!(hv["opponent_hole"].is_null() && gv["opponent_hole"].is_null());
            // Cards repeat across hands, so look at the current hand only.
            let current = |snap: &Value| {
                let mut s = snap.clone();
                s["game"]["view"]["history"] = json!([]);
                s.to_string()
            };
            let (host_text, guest_text) = (current(&h), current(&g));
            for c in gv["hole"].as_array().unwrap() {
                assert!(!host_text.contains(&c.to_string()), "host saw {c}");
            }
            for c in hv["hole"].as_array().unwrap() {
                assert!(!guest_text.contains(&c.to_string()), "guest saw {c}");
            }
            hands_checked += 1;
        }
        let (me, other, snap) = if h["game"]["clock"]["my_turn"] == true {
            (&mut host, &mut guest, h.clone())
        } else if g["game"]["clock"]["my_turn"] == true {
            (&mut guest, &mut host, g.clone())
        } else {
            // Between hands: wait for the next deal on both sides.
            h = host.snapshot(|_| true).await;
            g = guest.snapshot(|_| true).await;
            continue;
        };
        let game = &snap["game"];
        let action = json!({
            "type": "player_action",
            "room_id": room_id,
            "match_id": game["match_id"],
            "action_id": format!("a{}", game["state_version"]),
            "expected_state_version": game["state_version"],
            "action": passive(&game["view"]),
        });
        me.send(action.clone()).await;
        let ack = me.until(|m| m["type"] == "action_accepted").await;
        assert_eq!(ack["duplicate"], false);
        if !duplicate_checked {
            me.send(action).await; // network retry
            let ack = me
                .until(|m| m["type"] == "action_accepted" || m["type"] == "action_rejected")
                .await;
            assert_eq!(
                (ack["type"].as_str(), ack["duplicate"].as_bool()),
                (Some("action_accepted"), Some(true))
            );
            duplicate_checked = true;
        }
        let _ = other;
        let version = ack["state_version"].as_u64().unwrap();
        let at_least = move |r: &Value| {
            r["game"]["state_version"].as_u64().unwrap_or(0) >= version || r["status"] == "finished"
        };
        h = host.snapshot(at_least).await;
        g = guest.snapshot(at_least).await;
    }
    assert_eq!(h["status"], "finished");
    assert!(hands_checked > 0);
    let (hr, gr) = (&h["game"]["view"]["result"], &g["game"]["view"]["result"]);
    assert_eq!(hr["final_stacks"][0], gr["final_stacks"][1]);
    let total: u64 = hr["final_stacks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(total, 1200);
    assert_eq!(
        h["score"]["me"].as_u64().unwrap()
            + h["score"]["opponent"].as_u64().unwrap()
            + h["score"]["draws"].as_u64().unwrap(),
        1
    );

    // Rematch only after both agree.
    host.send(json!({ "type": "request_rematch", "room_id": room_id }))
        .await;
    let offer = guest
        .snapshot(|r| r["rematch"]["requested_by_opponent"] == true)
        .await;
    assert_eq!(offer["status"], "finished");
    guest
        .send(json!({ "type": "accept_rematch", "room_id": room_id }))
        .await;
    let again = guest.snapshot(|r| r["status"] == "playing").await;
    assert_ne!(again["game"]["match_id"], h["game"]["match_id"]);
    s.stop();
}

/// A player drops mid-match and comes back: the opponent sees "reconnecting",
/// and the returning player gets the current state, not a new seat.
#[tokio::test]
async fn reconnect_restores_the_match() {
    let s = start(":memory:").await;
    let (mut host, mut guest, room_id) = room_with_two(s.addr).await;
    let (h, _) = start_match(&mut host, &mut guest, &room_id).await;
    let version = h["game"]["state_version"].clone();

    guest.ws.close(None).await.unwrap();
    let away = host.snapshot(|r| r["opponent"]["connected"] == false).await;
    assert!(away["opponent"]["reconnect_deadline_ms"].as_u64().is_some());

    guest.reconnect().await;
    let back = guest.snapshot(|r| r["status"] == "playing").await;
    assert_eq!(back["game"]["state_version"], version);
    assert_eq!(back["you_are_host"], false);
    host.snapshot(|r| r["opponent"]["connected"] == true).await;

    // The same account opening a second socket replaces the first one.
    let mut second = Client::login(s.addr, 2002, "Максим").await;
    let replaced = guest.until(|m| m["type"] == "server_error").await;
    assert_eq!(replaced["code"], "session_replaced");
    let mine = second.snapshot(|r| r["status"] == "playing").await;
    assert_eq!(
        mine["you_are_host"], false,
        "still the guest seat, not a new player"
    );
    s.stop();
}

#[tokio::test]
async fn stale_and_out_of_turn_actions_are_rejected() {
    let s = start(":memory:").await;
    let (mut host, mut guest, room_id) = room_with_two(s.addr).await;
    let (h, g) = start_match(&mut host, &mut guest, &room_id).await;
    let (waiting, snap) = if h["game"]["clock"]["my_turn"] == true {
        (&mut guest, &g)
    } else {
        (&mut host, &h)
    };
    let game = &snap["game"];
    waiting
        .send(json!({
            "type": "player_action", "room_id": room_id, "match_id": game["match_id"], "action_id": "x1",
            "expected_state_version": game["state_version"], "action": { "type": "call" },
        }))
        .await;
    let r = waiting.until(|m| m["type"] == "action_rejected").await;
    assert_eq!(r["code"], "not_your_turn");
    waiting
        .send(json!({
            "type": "player_action", "room_id": room_id, "match_id": game["match_id"], "action_id": "x2",
            "expected_state_version": 999, "action": { "type": "call" },
        }))
        .await;
    assert_eq!(
        waiting.until(|m| m["type"] == "action_rejected").await["code"],
        "stale_state"
    );
    waiting
        .send(json!({ "type": "surrender", "room_id": room_id }))
        .await;
    let done = waiting.snapshot(|r| r["status"] == "finished").await;
    assert_eq!(done["game"]["view"]["result"]["outcome"], "loss");
    assert_eq!(
        done["game"]["view"]["result"]["forfeit"], "human",
        "the forfeit is mine (I am always 'human')"
    );
    s.stop();
}

/// The server stops mid-match and starts again from its database: the match
/// continues at the same state version and nobody has lost.
#[tokio::test]
async fn match_survives_a_server_restart() {
    let db = std::env::temp_dir().join(format!("pvp-test-{}.sqlite", pvp_server::ids::match_id()));
    let db = db.to_str().unwrap().to_string();
    let s = start(&db).await;
    let (mut host, mut guest, room_id) = room_with_two(s.addr).await;
    let (h, _) = start_match(&mut host, &mut guest, &room_id).await;
    let version = h["game"]["state_version"].clone();
    s.stop();
    drop((host, guest));

    let s = start(&db).await;
    let mut host = Client::login(s.addr, 1001, "Олег").await;
    let mut guest = Client::login(s.addr, 2002, "Максим").await;
    let h = host
        .snapshot(|r| r["status"] == "playing" && r["opponent"]["connected"] == true)
        .await;
    assert_eq!(h["game"]["state_version"], version);
    assert!(h["game"]["view"]["result"].is_null(), "no false result");
    guest
        .send(json!({ "type": "request_snapshot", "room_id": room_id }))
        .await;
    guest.snapshot(|r| r["status"] == "playing").await;
    s.stop();
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{db}{ext}"));
    }
}

/// "Играть с другом" again returns the host's open room, so a link already
/// sent keeps working and a friend who joined meanwhile is found there.
#[tokio::test]
async fn new_invite_reuses_the_open_room() {
    let s = start(":memory:").await;
    let mut host = Client::login(s.addr, 1001, "Олег").await;
    let mut ids = vec![];
    for i in 0..5 {
        host.send(json!({ "type": "create_room", "request_id": format!("r{i}") }))
            .await;
        ids.push(
            host.until(|m| m["type"] == "room_created").await["room_id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    assert!(ids.iter().all(|id| id == &ids[0]), "{ids:?}");
    host.snapshot(|r| r["status"] == "waiting").await;

    let mut guest = Client::login(s.addr, 2002, "Максим").await;
    guest
        .send(json!({ "type": "peek_room", "room_id": ids[0] }))
        .await;
    let preview = guest.until(|m| m["type"] == "room_preview").await;
    assert_eq!(preview["preview"]["joinable"].as_bool(), Some(true));
    guest
        .send(json!({ "type": "join_room", "room_id": ids[0] }))
        .await;
    guest.snapshot(|r| r["status"] == "ready").await;

    host.send(json!({ "type": "create_room", "request_id": "r9" }))
        .await;
    let fresh = host.until(|m| m["type"] == "room_created").await["room_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(fresh, ids[0]);
    host.snapshot(|r| r["status"] == "ready").await;
    s.stop();
}

#[tokio::test]
async fn friend_joins_by_room_code() {
    let s = start(":memory:").await;
    let mut host = Client::login(s.addr, 1001, "Олег").await;
    host.send(json!({ "type": "create_room", "request_id": "r1" }))
        .await;
    let created = host.until(|m| m["type"] == "room_created").await;
    assert_eq!(created["reused"], false);
    let room_id = created["room_id"].as_str().unwrap().to_string();
    let lobby = host.snapshot(|r| r["status"] == "waiting").await;
    let code = lobby["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 6);

    // "Играть с другом" again: the same room, flagged as still open.
    host.send(json!({ "type": "create_room", "request_id": "r2" }))
        .await;
    let again = host.until(|m| m["type"] == "room_created").await;
    assert_eq!(again["room_id"], room_id.as_str());
    assert_eq!(again["reused"], true);

    let mut guest = Client::login(s.addr, 2002, "Максим").await;
    let spaced = format!("{} {}", &code[..3], &code[3..]);
    guest
        .send(json!({ "type": "find_room", "code": spaced }))
        .await;
    let preview = guest.until(|m| m["type"] == "room_preview").await;
    assert_eq!(preview["preview"]["room_id"], room_id.as_str());
    assert_eq!(preview["preview"]["host_name"], "Олег");
    assert_eq!(preview["preview"]["joinable"], true);
    guest
        .send(json!({ "type": "join_room", "room_id": room_id }))
        .await;
    guest.snapshot(|r| r["status"] == "ready").await;

    // A seated room is no longer found by its code.
    let mut third = Client::login(s.addr, 3003, "Аня").await;
    third
        .send(json!({ "type": "find_room", "code": code }))
        .await;
    let miss = third.until(|m| m["type"] == "server_error").await;
    assert_eq!(miss["code"], "code_not_found");

    // Guessing is capped per user.
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for _ in 0..19 {
        third
            .send(json!({ "type": "find_room", "code": wrong }))
            .await;
        third.until(|m| m["type"] == "server_error").await;
    }
    third
        .send(json!({ "type": "find_room", "code": wrong }))
        .await;
    let capped = third.until(|m| m["type"] == "server_error").await;
    assert_eq!(capped["code"], "too_many_attempts");
    s.stop();
}

/// Without VK a player can join a friend's room by its code, but not host.
#[tokio::test]
async fn guest_joins_by_code_but_cannot_host() {
    let s = start(":memory:").await;
    let key = "g".repeat(43);
    let mut guest = Client::login_guest(s.addr, &key, "Маша").await;
    guest
        .send(json!({ "type": "create_room", "request_id": "g1" }))
        .await;
    let denied = guest.until(|m| m["type"] == "server_error").await;
    assert_eq!(denied["code"], "guest_cannot_host");

    let mut host = Client::login(s.addr, 1001, "Олег").await;
    host.send(json!({ "type": "create_room", "request_id": "r1" }))
        .await;
    let room_id = host.until(|m| m["type"] == "room_created").await["room_id"]
        .as_str()
        .unwrap()
        .to_string();
    let code = host.snapshot(|r| r["status"] == "waiting").await["code"]
        .as_str()
        .unwrap()
        .to_string();

    guest
        .send(json!({ "type": "find_room", "code": code }))
        .await;
    let preview = guest.until(|m| m["type"] == "room_preview").await;
    assert_eq!(preview["preview"]["room_id"], room_id.as_str());
    guest
        .send(json!({ "type": "join_room", "room_id": room_id }))
        .await;
    guest.snapshot(|r| r["status"] == "ready").await;
    let lobby = host.snapshot(|r| r["status"] == "ready").await;
    assert_eq!(lobby["opponent"]["name"], "Маша (гость)");

    // The same browser key later is the same player: the seat is still theirs.
    let mut again = Client::login_guest(s.addr, &key, "Маша").await;
    again
        .send(json!({ "type": "peek_room", "room_id": room_id }))
        .await;
    let back = again.until(|m| m["type"] == "room_preview").await;
    assert_eq!(back["preview"]["already_member"], true);

    let (status, body) = post(s.addr, "/api/auth", &json!({ "guest_key": "short" }), None).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("bad_guest_key"))
    );
    s.stop();
}
