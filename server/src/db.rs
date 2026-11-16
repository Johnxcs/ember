//! Supabase (Postgres) persistence. Optional: enabled when DATABASE_URL is set.
use crate::{Channel, Msg, PollData};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Row,
};
use std::{collections::HashSet, str::FromStr};

const SCHEMA: &str = include_str!("../schema.sql");

pub async fn connect(url: &str) -> Result<PgPool, sqlx::Error> {
    // statement_cache_capacity(0) keeps this compatible with Supabase's
    // transaction pooler (port 6543), which doesn't support prepared statements.
    let opts = PgConnectOptions::from_str(url)?.statement_cache_capacity(0);
    let pool = PgPoolOptions::new().max_connections(5).connect_with(opts).await?;
    sqlx::raw_sql(SCHEMA).execute(&pool).await?;
    Ok(pool)
}

pub async fn load(pool: &PgPool) -> Result<Vec<Channel>, sqlx::Error> {
    let rows = sqlx::query("select id, name, ttl from channels order by created_at")
        .fetch_all(pool)
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let id: String = r.get("id");
        let ttl: Option<i64> = r.get("ttl");
        let mut ch = Channel { id: id.clone(), name: r.get("name"), ttl: ttl.map(|t| t as u64), heat: 0.0, msgs: vec![] };
        let ms = sqlx::query(
            "select id, author, body, ts, expires, sparks, poll, voters from (
               select * from messages
               where channel_id = $1 and (expires is null or expires > $2)
               order by ts desc limit 200) s
             order by ts",
        )
        .bind(&id)
        .bind(crate::now() as i64)
        .fetch_all(pool)
        .await?;
        for m in ms {
            let expires: Option<i64> = m.get("expires");
            let poll: Option<serde_json::Value> = m.get("poll");
            let voters: Vec<String> = m.get("voters");
            ch.msgs.push(Msg {
                id: m.get("id"),
                channel: id.clone(),
                author: m.get("author"),
                text: m.get("body"),
                ts: m.get::<i64, _>("ts") as u64,
                expires: expires.map(|e| e as u64),
                sparks: m.get::<i32, _>("sparks") as u32,
                poll: poll.and_then(|v| serde_json::from_value::<PollData>(v).ok()),
                voters: voters.into_iter().collect::<HashSet<_>>(),
            });
        }
        out.push(ch);
    }
    Ok(out)
}

pub async fn save_channel(pool: &PgPool, id: &str, name: &str, ttl: Option<u64>) -> Result<(), sqlx::Error> {
    sqlx::query("insert into channels (id, name, ttl) values ($1, $2, $3) on conflict (id) do nothing")
        .bind(id)
        .bind(name)
        .bind(ttl.map(|t| t as i64))
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn save_msg(pool: &PgPool, m: &Msg) -> Result<(), sqlx::Error> {
    let poll = m.poll.as_ref().and_then(|p| serde_json::to_value(p).ok());
    sqlx::query(
        "insert into messages (id, channel_id, author, body, ts, expires, poll)
         values ($1, $2, $3, $4, $5, $6, $7) on conflict (id) do nothing",
    )
    .bind(&m.id)
    .bind(&m.channel)
    .bind(&m.author)
    .bind(&m.text)
    .bind(m.ts as i64)
    .bind(m.expires.map(|e| e as i64))
    .bind(poll)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_sparks(pool: &PgPool, id: &str, sparks: i32) -> Result<(), sqlx::Error> {
    sqlx::query("update messages set sparks = $2 where id = $1").bind(id).bind(sparks).execute(pool).await?;
    Ok(())
}

pub async fn save_vote(pool: &PgPool, id: &str, poll: &serde_json::Value, voter: &str) -> Result<(), sqlx::Error> {
    sqlx::query("update messages set poll = $2, voters = array_append(voters, $3) where id = $1")
        .bind(id)
        .bind(poll)
        .bind(voter)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete vanished messages for good.
pub async fn purge(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("delete from messages where expires is not null and expires <= $1")
        .bind(crate::now() as i64)
        .execute(pool)
        .await?;
    Ok(())
}

/// Reserve a unique username for an account. Returns the account's username,
/// or None if somebody else already owns the one it asked for.
pub async fn claim_username(pool: &PgPool, user_id: &str, wanted: &str) -> Result<Option<String>, sqlx::Error> {
    if let Some(r) = sqlx::query("select username from profiles where user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await?
    {
        return Ok(Some(r.get("username")));
    }
    let r = sqlx::query("insert into profiles (user_id, username) values ($1, $2) on conflict do nothing returning username")
        .bind(user_id)
        .bind(wanted)
        .fetch_optional(pool)
        .await?;
    Ok(r.map(|r| r.get("username")))
}

pub async fn username_free(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("select 1 from profiles where lower(username) = lower($1)")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(r.is_none())
}
