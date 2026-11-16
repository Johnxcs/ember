# 🔥 Ember

Discord-style chat written in Rust. Server: axum + WebSockets. App: Tauri 2 (Rust shell + web UI) built into an Android APK by GitHub Actions.

## Unique features
- **Vanishing messages**: pick 10s / 1m / 1h per message, or use channels with a default burn time (`#campfire`). A burn bar shows time left.
- **Sparks + Heat**: ✨ a message to feed the channel's heat meter. Hot channels become a "bonfire" and glow; heat cools off over time.
- **Live polls**: `/poll Pizza or tacos? | Pizza | Tacos`
- **Whispers**: `/w name text`, visible only to you and them, never stored.
- **Moods**: `/mood 😎` shows next to your name.
- **Burn channels**: `/channel secret 30` creates a channel where everything vanishes after 30s.

## Supabase database (persistence)
Channels, messages, sparks and poll votes are saved to Supabase Postgres. Vanished messages are deleted for good.
1. Create a project at supabase.com.
2. **Project Settings → Database → Connection string**: copy the **Session pooler** string (works over IPv4). The *Transaction pooler* (port 6543) also works.
3. Run the server with it:
```
cd server
DATABASE_URL='postgresql://postgres.xxxx:PASSWORD@aws-0-REGION.pooler.supabase.com:5432/postgres' cargo run --release
```
The tables are created automatically on first start (`server/schema.sql`; you can also paste it into the Supabase SQL Editor). Row Level Security is enabled with no policies, so Supabase's public REST API can't read your chats; only the server (database owner) can.
Without `DATABASE_URL` the server falls back to in-memory storage.

Keep the database password on the server only (host env vars / secrets). Never put it in the Android app.

## Deploy free on Render
1. Push this repo to GitHub. Sign up at render.com with GitHub.
2. **New → Blueprint**, pick your repo. Render reads `render.yaml`.
3. When asked for `DATABASE_URL`, paste your Supabase Session pooler string. Click **Apply**.
4. After the build, your server is at `https://ember-server.onrender.com`. In the app, connect to `wss://ember-server.onrender.com/ws`.
5. Free services sleep after ~15 min idle. To keep it awake, add a free monitor (UptimeRobot / cron-job.org) that requests `https://ember-server.onrender.com/health` every 5-10 minutes.

## Run the server
```
cd server && cargo run --release     # PORT=8080 by default
```
Host it anywhere that runs a binary (VPS, Fly.io, Railway). Use `wss://` behind a TLS proxy for public servers.

## Build the APK with GitHub
1. Push this folder to a GitHub repo (branch `main`).
2. Open **Actions → Build Android APK** (runs on every push, or click *Run workflow*).
3. Download the `ember-apk` artifact → install `ember.apk`. Tag `v1.0.0` to also attach it to a Release.

The APK is debug-signed so it installs directly. For Play Store, add a release keystore via GitHub Secrets.

## Desktop dev
```
cd app && npm i -g @tauri-apps/cli@^2
convert -size 1024x1024 xc:'#ff6b35' icon.png && tauri icon icon.png
tauri dev
```
(The UI also works in a plain browser: open `app/ui/index.html`.)

## Ideas next
Supabase Auth for real accounts, image uploads, voice rooms (WebRTC), push notifications.
# ember
