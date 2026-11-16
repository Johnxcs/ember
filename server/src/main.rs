//! Ember server: Discord-style chat with vanishing messages, Sparks + channel Heat,
//! live polls, whispers and moods. All state is in memory.
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

mod db;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

#[derive(Clone, Serialize, Deserialize)]
struct PollOpt {
    label: String,
    votes: u32,
}
#[derive(Clone, Serialize, Deserialize)]
struct PollData {
    question: String,
    options: Vec<PollOpt>,
}
#[derive(Clone, Serialize)]
struct Msg {
    id: String,
    channel: String,
    author: String,
    text: String,
    ts: u64,
    expires: Option<u64>, // vanishing messages
    sparks: u32,
    poll: Option<PollData>,
    #[serde(skip)]
    voters: HashSet<String>,
}
struct Channel {
    id: String,
    name: String,
    ttl: Option<u64>, // default vanish time for this channel
    heat: f32,        // rises with activity, decays over time
    msgs: Vec<Msg>,
}
impl Channel {
    fn new(name: &str, ttl: Option<u64>) -> Self {
        Channel { id: Uuid::new_v4().simple().to_string()[..8].into(), name: name.into(), ttl, heat: 0.0, msgs: vec![] }
    }
    fn info(&self) -> Value {
        json!({"id": self.id, "name": self.name, "ttl": self.ttl, "heat": self.heat})
    }
}
struct Inner {
    channels: Vec<Channel>,
    users: HashMap<String, String>, // name -> mood
}
#[derive(Clone)]
struct Out {
    to: Option<String>,
    json: String,
}
#[derive(Clone)]
struct App {
    inner: Arc<Mutex<Inner>>,
    bus: broadcast::Sender<Out>,
    db: Option<sqlx::PgPool>, // Supabase / Postgres (optional)
}
impl App {
    /// Fire-and-forget DB write so chat never waits on the database.
    fn persist<F, Fut>(&self, f: F)
    where
        F: FnOnce(sqlx::PgPool) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), sqlx::Error>> + Send + 'static,
    {
        if let Some(p) = self.db.clone() {
            tokio::spawn(async move {
                if let Err(e) = f(p).await {
                    eprintln!("db error: {e}");
                }
            });
        }
    }
    fn emit(&self, to: Option<&str>, v: Value) {
        let _ = self.bus.send(Out { to: to.map(String::from), json: v.to_string() });
    }
}
fn presence(i: &Inner) -> Value {
    let mut u: Vec<Value> = i.users.iter().map(|(n, m)| json!({"name": n, "mood": m})).collect();
    u.sort_by_key(|x| x["name"].as_str().unwrap_or("").to_string());
    json!({"t": "presence", "users": u})
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Client {
    Join { name: String },
    Send { channel: String, text: String, ttl: Option<u64> },
    Spark { channel: String, msg: String },
    Poll { channel: String, question: String, options: Vec<String> },
    Vote { channel: String, msg: String, option: usize },
    Mood { mood: String },
    NewChannel { name: String, ttl: Option<u64> },
    Whisper { to: String, text: String },
}

async fn handle(app: &App, me: &str, cmd: Client) {
    let mut g = app.inner.lock().await;
    match cmd {
        Client::Join { .. } => {}
        Client::Send { channel, text, ttl } => {
            let text = text.trim().to_string();
            if text.is_empty() || text.chars().count() > 2000 {
                return;
            }
            let Some(ch) = g.channels.iter_mut().find(|c| c.id == channel) else { return };
            let ttl = ttl.or(ch.ttl).map(|t| t.clamp(5, 86400));
            let m = Msg {
                id: Uuid::new_v4().to_string(),
                channel: ch.id.clone(),
                author: me.into(),
                text,
                ts: now(),
                expires: ttl.map(|t| now() + t),
                sparks: 0,
                poll: None,
                voters: HashSet::new(),
            };
            ch.heat = (ch.heat + 0.5).min(40.0);
            ch.msgs.push(m.clone());
            let mc = m.clone();
            app.persist(move |p| async move { db::save_msg(&p, &mc).await });
            if ch.msgs.len() > 200 {
                ch.msgs.remove(0);
            }
            app.emit(None, json!({"t": "message", "msg": m}));
        }
        Client::Spark { channel, msg } => {
            let Some(ch) = g.channels.iter_mut().find(|c| c.id == channel) else { return };
            ch.heat = (ch.heat + 2.0).min(40.0);
            let Some(m) = ch.msgs.iter_mut().find(|m| m.id == msg) else { return };
            m.sparks += 1;
            let (id, n) = (msg.clone(), m.sparks as i32);
            app.persist(move |p| async move { db::set_sparks(&p, &id, n).await });
            app.emit(None, json!({"t": "spark", "channel": channel, "msg": msg, "count": m.sparks}));
        }
        Client::Poll { channel, question, options } => {
            let q = question.trim().to_string();
            let options: Vec<PollOpt> = options
                .into_iter()
                .map(|o| o.trim().to_string())
                .filter(|o| !o.is_empty())
                .take(6)
                .map(|label| PollOpt { label, votes: 0 })
                .collect();
            if q.is_empty() || options.len() < 2 {
                return;
            }
            let Some(ch) = g.channels.iter_mut().find(|c| c.id == channel) else { return };
            let m = Msg {
                id: Uuid::new_v4().to_string(),
                channel: ch.id.clone(),
                author: me.into(),
                text: q.clone(),
                ts: now(),
                expires: ch.ttl.map(|t| now() + t),
                sparks: 0,
                poll: Some(PollData { question: q, options }),
                voters: HashSet::new(),
            };
            ch.heat = (ch.heat + 1.0).min(40.0);
            ch.msgs.push(m.clone());
            let mc = m.clone();
            app.persist(move |p| async move { db::save_msg(&p, &mc).await });
            app.emit(None, json!({"t": "message", "msg": m}));
        }
        Client::Vote { channel, msg, option } => {
            let Some(ch) = g.channels.iter_mut().find(|c| c.id == channel) else { return };
            let Some(m) = ch.msgs.iter_mut().find(|m| m.id == msg) else { return };
            let Some(p) = m.poll.as_mut() else { return };
            if option >= p.options.len() || !m.voters.insert(me.to_string()) {
                return;
            }
            p.options[option].votes += 1;
            let (id, pv, voter) = (msg.clone(), serde_json::to_value(&*p).unwrap_or_default(), me.to_string());
            app.persist(move |pool| async move { db::save_vote(&pool, &id, &pv, &voter).await });
            app.emit(None, json!({"t": "poll", "channel": channel, "msg": msg, "poll": p}));
        }
        Client::Mood { mood } => {
            let mood: String = mood.trim().chars().take(24).collect();
            g.users.insert(me.to_string(), mood);
            let p = presence(&g);
            app.emit(None, p);
        }
        Client::NewChannel { name, ttl } => {
            let name: String = name
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .take(24)
                .collect();
            if name.is_empty() || g.channels.len() >= 30 {
                return;
            }
            let ch = Channel::new(&name, ttl.map(|t| t.clamp(5, 86400)));
            let info = ch.info();
            let (cid, cname, cttl) = (ch.id.clone(), ch.name.clone(), ch.ttl);
            app.persist(move |p| async move { db::save_channel(&p, &cid, &cname, cttl).await });
            g.channels.push(ch);
            app.emit(None, json!({"t": "channel", "channel": info}));
        }
        Client::Whisper { to, text } => {
            let text = text.trim().to_string();
            if text.is_empty() || !g.users.contains_key(&to) {
                app.emit(Some(me), json!({"t": "error", "text": format!("No one named {to} is online")}));
                return;
            }
            let ev = json!({"t": "whisper", "from": me, "to": to, "text": text, "ts": now()});
            app.emit(Some(&to), ev.clone());
            app.emit(Some(me), ev);
        }
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(app): State<App>) -> impl IntoResponse {
    ws.on_upgrade(move |s| client(s, app))
}

async fn client(socket: WebSocket, app: App) {
    let (mut tx, mut rx) = socket.split();
    let mut bus = app.bus.subscribe();
    let mut name: Option<String> = None;
    loop {
        tokio::select! {
            incoming = rx.next() => {
                let Some(Ok(m)) = incoming else { break };
                let Message::Text(txt) = m else { continue };
                let Ok(cmd) = serde_json::from_str::<Client>(&txt) else { continue };
                match (&name, cmd) {
                    (None, Client::Join { name: n }) => {
                        let base: String = n.split_whitespace().collect::<Vec<_>>().join("_").chars().take(20).collect();
                        if base.is_empty() { continue }
                        let mut g = app.inner.lock().await;
                        let mut nm = base.clone();
                        let mut i = 1;
                        while g.users.contains_key(&nm) { i += 1; nm = format!("{base}{i}"); }
                        g.users.insert(nm.clone(), "🙂".into());
                        let history: HashMap<String, &Vec<Msg>> =
                            g.channels.iter().map(|c| (c.id.clone(), &c.msgs)).collect();
                        let pres = presence(&g);
                        let w = json!({
                            "t": "welcome", "you": nm, "now": now(),
                            "channels": g.channels.iter().map(|c| c.info()).collect::<Vec<_>>(),
                            "history": history, "users": pres["users"],
                        });
                        drop(g);
                        if tx.send(Message::Text(w.to_string())).await.is_err() { break }
                        app.emit(None, pres);
                        name = Some(nm);
                    }
                    (Some(me), cmd) => handle(&app, me, cmd).await,
                    _ => {}
                }
            }
            ev = bus.recv() => match ev {
                Ok(o) => {
                    if o.to.is_none() || o.to == name {
                        if tx.send(Message::Text(o.json)).await.is_err() { break }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    }
    if let Some(me) = name {
        let mut g = app.inner.lock().await;
        g.users.remove(&me);
        let p = presence(&g);
        drop(g);
        app.emit(None, p);
    }
}

#[tokio::main]
async fn main() {
    let (bus, _) = broadcast::channel(256);
    // Supabase = Postgres. Set DATABASE_URL to persist; leave unset to run in memory.
    let db = match std::env::var("DATABASE_URL") {
        Ok(url) => match db::connect(&url).await {
            Ok(p) => {
                println!("Connected to Supabase/Postgres");
                Some(p)
            }
            Err(e) => {
                eprintln!("DB connect failed ({e}); running in memory");
                None
            }
        },
        Err(_) => {
            println!("DATABASE_URL not set; running in memory");
            None
        }
    };
    let mut channels = match &db {
        Some(p) => db::load(p).await.unwrap_or_else(|e| {
            eprintln!("DB load failed: {e}");
            vec![]
        }),
        None => vec![],
    };
    if channels.is_empty() {
        channels = vec![
            Channel::new("general", None),
            Channel::new("campfire", Some(60)), // everything here burns after 60s
            Channel::new("random", None),
        ];
        if let Some(p) = &db {
            for c in &channels {
                let _ = db::save_channel(p, &c.id, &c.name, c.ttl).await;
            }
        }
    }
    let app = App {
        inner: Arc::new(Mutex::new(Inner { channels, users: HashMap::new() })),
        bus,
        db,
    };

    // Background ticker: vanish expired messages, decay channel heat.
    tokio::spawn({
        let app = app.clone();
        async move {
            let mut iv = tokio::time::interval(Duration::from_secs(1));
            let mut n = 0u32;
            loop {
                iv.tick().await;
                n += 1;
                let mut gone = vec![];
                let mut levels = serde_json::Map::new();
                {
                    let mut g = app.inner.lock().await;
                    let t = now();
                    for ch in g.channels.iter_mut() {
                        let cid = ch.id.clone();
                        ch.msgs.retain(|m| {
                            if m.expires.map_or(false, |e| e <= t) {
                                gone.push((cid.clone(), m.id.clone()));
                                false
                            } else {
                                true
                            }
                        });
                        if n % 5 == 0 {
                            ch.heat *= 0.9;
                            levels.insert(ch.id.clone(), json!((ch.heat * 10.0).round() / 10.0));
                        }
                    }
                }
                for (c, m) in gone {
                    app.emit(None, json!({"t": "gone", "channel": c, "msg": m}));
                }
                if n % 5 == 0 {
                    app.emit(None, json!({"t": "heat", "levels": levels}));
                }
                if n % 30 == 0 {
                    app.persist(|p| async move { db::purge(&p).await });
                }
            }
        }
    });

    let router = Router::new()
        .route("/", get(|| async { "Ember server is running" }))
        .route("/health", get(|| async { "ok" }))
        .route("/ws", get(ws_handler))
        .with_state(app);
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    println!("Ember listening on :{port}");
    axum::serve(listener, router).await.unwrap();
}
