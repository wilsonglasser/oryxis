# Self-hosting the Oryxis signaling + relay server

Oryxis sync needs a signaling/relay server for cross-network use.
**We don't operate one.** You deploy your own.

For LAN-only sync (same Wi-Fi / VPN subnet), no server is needed.
mDNS handles discovery; QUIC handles transport.

## Why two roles in one server?

- **Signaling** answers `device_id -> ip:port`. Required for any
  cross-network sync. Even when QUIC direct works, peers need to
  find each other first.
- **Relay** carries actual sync traffic. Used as fallback when
  QUIC direct fails (symmetric NAT, double NAT, blocked UDP).

Both speak the same HTTP API, share the same auth token, and live
in the same binary / Worker. You can deploy either form.

## What the server sees

Opaque bytes. Sync payloads are sealed with ChaCha20-Poly1305 using
a key derived at pairing time (X25519 DH) and never leaves the two
paired devices. The relay sees ciphertext only; it doesn't even
know which entity types are flowing.

The bearer token (`ORYXIS_RELAY_TOKEN` / `SIGNALING_TOKEN`) is
"who can talk to this relay" — it gates access to the server
itself, not the content. Treat it as a long shared secret (32+
random chars). All paired clients use the same value.

---

## Option 1: Cloudflare Workers (recommended, free tier covers it)

Cloudflare's free tier covers typical Oryxis usage (Workers free
tier includes 100k requests/day; Durable Objects is on the free
tier with its own monthly allowance). Check the current numbers at
[Workers pricing](https://developers.cloudflare.com/workers/platform/pricing/)
and [Durable Objects pricing](https://developers.cloudflare.com/durable-objects/platform/pricing/);
a pair of devices syncing every few hours stays well inside both.
Bandwidth is free for Workers.

```bash
# In the Oryxis repo
cd signaling-worker
npm install -g wrangler
wrangler login

# Create the KV namespace and copy its ID into wrangler.jsonc.
# KV holds the relay queue (`relay:*`); discovery state lives in
# the DeviceRegistry Durable Object provisioned automatically by
# the deploy step below (migration `v1` in wrangler.jsonc).
wrangler kv namespace create SYNC_KV

# Set the shared token (matches Settings > Sync > Signaling token)
wrangler secret put SIGNALING_TOKEN

wrangler deploy
```

The first `wrangler deploy` after a fresh clone runs the `v1`
Durable Objects migration declared in `wrangler.jsonc`, which
provisions the `DeviceRegistry` class. No extra command needed.

Wrangler prints the deployed URL
(`https://oryxis-signaling.<your-subdomain>.workers.dev`).
Paste it into **Settings > Sync > Advanced > Signaling Server** in
the app, paste the token below it, click Save.

## Option 2: Docker (own VPS / NAS / homelab)

```bash
docker run -d \
  --name oryxis-relay \
  --restart unless-stopped \
  -p 8080:8080 \
  -e ORYXIS_RELAY_TOKEN=<long-random-string> \
  ghcr.io/wilsonglasser/oryxis-relay:latest
```

Behind a reverse proxy (recommended, gives you TLS for free):

```nginx
# /etc/nginx/sites-available/oryxis-relay
server {
    listen 443 ssl http2;
    server_name relay.example.com;

    # ... certbot block ...

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        # Long-poll: the GET inbox waits up to 30s, so keep the
        # upstream timeout above that.
        proxy_read_timeout 60s;
        proxy_buffering off;
    }
}
```

Then point the app at `https://relay.example.com`.

## Option 3: Bare binary + systemd

Download the binary for your platform from the latest
[`relay-v*`](https://github.com/wilsonglasser/oryxis/releases) release,
or build from source:

```bash
cargo install --path crates/oryxis-relay
# or
cargo build --release -p oryxis-relay
# binary lives at target/release/oryxis-relay
```

Systemd unit:

```ini
# /etc/systemd/system/oryxis-relay.service
[Unit]
Description=Oryxis sync relay
After=network.target

[Service]
Type=simple
User=oryxis
ExecStart=/usr/local/bin/oryxis-relay --port 8080
Environment=ORYXIS_RELAY_TOKEN=<long-random-string>
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now oryxis-relay
```

Front it with nginx / Caddy as in Option 2.

---

## Verifying it works

From any machine:

```bash
TOKEN=<your-token>
URL=https://your-relay/

# Healthz (binary only; Worker doesn't expose this)
curl ${URL}healthz

# Register a fake device
curl -X POST ${URL}register \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"device_id":"11111111-1111-1111-1111-111111111111","ip":"1.2.3.4","port":9000}'
# -> {"ok":true,"ttl":300}

# Look it up
curl ${URL}lookup/11111111-1111-1111-1111-111111111111 \
  -H "Authorization: Bearer ${TOKEN}"
# -> {"device_id":"...","ip":"1.2.3.4","port":9000,...}
```

In the app, **Settings > Sync** shows the heartbeat counter
incrementing each time the device re-registers (every ~3 min).

## Bandwidth

The relay only ships ciphertext sync payloads. A vault with 50
connections + 5 keys + 20 snippets fits in ~5 KB encoded. A normal
day's incremental sync is well under 1 MB per device-pair.

Cloudflare Workers free tier: 100k requests/day, free egress.
A single VPS with 1 TB/mo bandwidth covers thousands of users.

## Security

The relay runs three layers of defense:

1. **Bearer token** — required on every request. Pick a long random
   string and share it only between your devices.
2. **Application-layer auth** — peers sign an Ed25519 challenge
   bound to the TLS exporter at every connection; an attacker with
   the token but no paired key still can't impersonate a peer.
3. **End-to-end encryption** — payloads sealed with
   ChaCha20-Poly1305 using a key derived at pairing via X25519 DH;
   the relay never sees plaintext.

Even a fully compromised relay can: (a) refuse service, (b) learn
that two devices are communicating and how often. It cannot: read
vault content, forge sync records, or impersonate a paired device.

## Rotating the token

1. Stop the relay.
2. Generate a new random token.
3. Update `SIGNALING_TOKEN` (Worker) or `ORYXIS_RELAY_TOKEN`
   (binary).
4. Update the same value in **Settings > Sync > Signaling token**
   on every paired device.
5. Restart the relay.

Devices that don't get the new token will fail with HTTP 401 in the
sync log; the app surfaces this in the sync status.
