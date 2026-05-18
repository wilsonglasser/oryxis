/**
 * Oryxis Sync Signaling + Relay Server
 * Cloudflare Worker + KV
 *
 * Discovery routes:
 *   POST   /register             — register device IP:port (TTL 300s)
 *                                  Body: { device_id, public_key_fp?,
 *                                          public_key (32B hex),
 *                                          ip, port, signed_at,
 *                                          signature (64B hex) }
 *                                  TOFU: public_key is pinned on first
 *                                  register; later registers must use
 *                                  the same key or the worker returns
 *                                  403 (anti-hijack inside one bearer).
 *   GET    /lookup/:id           — look up peer's IP:port
 *   DELETE /register/:id         — unregister device
 *                                  Headers: X-Pubkey, X-Signed-At,
 *                                           X-Signature (auth fields)
 *                                  TOFU: must match the pinned pubkey.
 *
 * Relay routes (Phase D — fallback transport when QUIC direct fails):
 *   POST   /relay/:recipient_id/inbox   — enqueue a frame for recipient
 *     Headers: X-Sender-Id: <uuid>
 *     Body:    raw bytes (max 256KB) — the bincode-encoded SyncMessage,
 *              already E2E-encrypted by the client
 *   GET    /relay/:recipient_id/inbox[?wait_ms=30000]
 *                                — consume the oldest frame for me,
 *                                  long-poll up to wait_ms when empty
 *     Response 200: body = raw bytes, header `X-Sender-Id` echoes the
 *                   sender so the client can demux multi-peer streams
 *     Response 204: no message landed within wait_ms
 *
 * All requests require: Authorization: Bearer <SIGNALING_TOKEN>
 * Set the token via: wrangler secret put SIGNALING_TOKEN
 *
 * Self-hosting: this file is the entire relay; deploy with
 * `wrangler deploy`. Both signaling and relay live in one KV namespace
 * (`SYNC_KV`) keyed by prefix.
 */

const TTL = 300; // 5 minutes — applies to both register entries and relay queue items.
const MAX_FRAME_BYTES = 256 * 1024;
const MAX_WAIT_MS = 30_000;
const POLL_INTERVAL_MS = 500;

// Must match `oryxis_sync::crypto::REGISTER_SIGN_LABEL` and the Rust
// relay (`oryxis-relay::main::REGISTER_SIGN_LABEL`) byte-for-byte;
// the worker, the relay, and the client all build the same canonical
// payload from these labels so any signature verifies in all three.
const REGISTER_SIGN_LABEL = "oryxis-register-v1";
const UNREGISTER_SIGN_LABEL = "oryxis-unregister-v1";
const REGISTER_TIMESTAMP_SKEW_SECS = 60;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname;

    const corsHeaders = {
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Allow-Methods": "GET, POST, DELETE, OPTIONS",
      "Access-Control-Allow-Headers":
        "Content-Type, Authorization, X-Sender-Id, X-Pubkey, X-Signed-At, X-Signature",
      "Access-Control-Expose-Headers": "X-Sender-Id",
    };

    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders });
    }

    const auth = request.headers.get("Authorization") || "";
    const token = auth.replace("Bearer ", "");
    if (!env.SIGNALING_TOKEN || token !== env.SIGNALING_TOKEN) {
      return json({ error: "Unauthorized" }, 401, corsHeaders);
    }

    try {
      // ── Discovery ──

      if (request.method === "POST" && path === "/register") {
        const body = await request.json();
        const { device_id, public_key_fp, public_key, ip, port, signed_at, signature } = body;
        if (!device_id || !ip || !port || !public_key || !signature || typeof signed_at !== "number") {
          return json({ error: "Missing fields" }, 400, corsHeaders);
        }
        if (!isValidUuid(device_id)) {
          return json({ error: "Bad device_id" }, 400, corsHeaders);
        }
        if (!isFreshTimestamp(signed_at)) {
          return json({ error: "Stale signed_at" }, 400, corsHeaders);
        }
        const pubkeyBytes = hexToBytes(public_key, 32);
        const sigBytes = hexToBytes(signature, 64);
        if (!pubkeyBytes || !sigBytes) {
          return json({ error: "Bad hex" }, 400, corsHeaders);
        }
        const payload = new TextEncoder().encode(
          `${REGISTER_SIGN_LABEL}\n${device_id}\n${ip}\n${port}\n${signed_at}`
        );
        const sigOk = await verifyEd25519(pubkeyBytes, payload, sigBytes);
        if (!sigOk) {
          return json({ error: "Bad signature" }, 400, corsHeaders);
        }
        // TOFU: if an entry exists, the new pubkey must match. Fetch
        // metadata, not the value, since we only need the pinned key.
        const existing = await env.SYNC_KV.getWithMetadata(`device:${device_id}`);
        if (existing && existing.metadata && existing.metadata.public_key && existing.metadata.public_key !== public_key) {
          return json({ error: "Pubkey mismatch" }, 403, corsHeaders);
        }
        const value = JSON.stringify({
          device_id,
          public_key_fp: public_key_fp || "",
          ip,
          port,
          registered_at: new Date().toISOString(),
        });
        await env.SYNC_KV.put(`device:${device_id}`, value, {
          expirationTtl: TTL,
          metadata: { public_key },
        });
        return json({ ok: true, ttl: TTL }, 200, corsHeaders);
      }

      if (request.method === "GET" && path.startsWith("/lookup/")) {
        const deviceId = path.replace("/lookup/", "");
        if (!isValidUuid(deviceId)) {
          return json({ error: "Bad device_id" }, 400, corsHeaders);
        }
        const value = await env.SYNC_KV.get(`device:${deviceId}`);
        if (!value) {
          return json({ error: "Not found" }, 404, corsHeaders);
        }
        return json(JSON.parse(value), 200, corsHeaders);
      }

      if (request.method === "DELETE" && path.startsWith("/register/")) {
        const deviceId = path.replace("/register/", "");
        if (!isValidUuid(deviceId)) {
          return json({ error: "Bad device_id" }, 400, corsHeaders);
        }
        const pubkeyHex = request.headers.get("X-Pubkey") || "";
        const signedAtStr = request.headers.get("X-Signed-At") || "";
        const sigHex = request.headers.get("X-Signature") || "";
        if (!pubkeyHex || !signedAtStr || !sigHex) {
          return json({ error: "Missing auth headers" }, 400, corsHeaders);
        }
        const signedAt = parseInt(signedAtStr, 10);
        if (!Number.isFinite(signedAt) || !isFreshTimestamp(signedAt)) {
          return json({ error: "Stale signed_at" }, 400, corsHeaders);
        }
        const pubkeyBytes = hexToBytes(pubkeyHex, 32);
        const sigBytes = hexToBytes(sigHex, 64);
        if (!pubkeyBytes || !sigBytes) {
          return json({ error: "Bad hex" }, 400, corsHeaders);
        }
        const payload = new TextEncoder().encode(
          `${UNREGISTER_SIGN_LABEL}\n${deviceId}\n${signedAt}`
        );
        const sigOk = await verifyEd25519(pubkeyBytes, payload, sigBytes);
        if (!sigOk) {
          return json({ error: "Bad signature" }, 400, corsHeaders);
        }
        // TOFU enforcement: if an entry exists, the caller's pubkey
        // must match the pinned key. Missing entry => 200 (idempotent).
        const existing = await env.SYNC_KV.getWithMetadata(`device:${deviceId}`);
        if (existing && existing.metadata && existing.metadata.public_key && existing.metadata.public_key !== pubkeyHex) {
          return json({ error: "Pubkey mismatch" }, 403, corsHeaders);
        }
        await env.SYNC_KV.delete(`device:${deviceId}`);
        return json({ ok: true }, 200, corsHeaders);
      }

      // ── Relay ──

      const relayMatch = path.match(/^\/relay\/([^/]+)\/inbox$/);
      if (relayMatch) {
        const recipientId = relayMatch[1];
        if (!isValidUuid(recipientId)) {
          return json({ error: "Bad recipient" }, 400, corsHeaders);
        }

        if (request.method === "POST") {
          const senderId = request.headers.get("X-Sender-Id") || "";
          if (!isValidUuid(senderId)) {
            return json({ error: "Missing X-Sender-Id" }, 400, corsHeaders);
          }
          const body = await request.arrayBuffer();
          if (body.byteLength === 0) {
            return json({ error: "Empty body" }, 400, corsHeaders);
          }
          if (body.byteLength > MAX_FRAME_BYTES) {
            return json({ error: "Too large" }, 413, corsHeaders);
          }
          // Composite key: prefix + recipient (for queue listing) +
          // a sortable, near-monotonic timestamp + random suffix so
          // two concurrent POSTs to the same recipient don't collide.
          const ts = Date.now().toString().padStart(13, "0");
          const rnd = Math.random().toString(36).slice(2, 10);
          const key = `relay:${recipientId}:${ts}-${rnd}`;
          await env.SYNC_KV.put(key, body, {
            expirationTtl: TTL,
            metadata: { sender_id: senderId },
          });
          return new Response(null, { status: 204, headers: corsHeaders });
        }

        if (request.method === "GET") {
          const waitMs = Math.min(
            parseInt(url.searchParams.get("wait_ms") || "0", 10) || 0,
            MAX_WAIT_MS
          );
          const deadline = Date.now() + waitMs;
          const prefix = `relay:${recipientId}:`;
          while (true) {
            const list = await env.SYNC_KV.list({ prefix, limit: 1 });
            if (list.keys.length > 0) {
              const key = list.keys[0].name;
              const { value, metadata } =
                await env.SYNC_KV.getWithMetadata(key, "arrayBuffer");
              await env.SYNC_KV.delete(key);
              if (value) {
                const senderId =
                  (metadata && metadata.sender_id) || "";
                return new Response(value, {
                  status: 200,
                  headers: {
                    ...corsHeaders,
                    "Content-Type": "application/octet-stream",
                    "X-Sender-Id": senderId,
                  },
                });
              }
              // Race with another consumer; keep looping until deadline.
            }
            if (Date.now() >= deadline) {
              return new Response(null, { status: 204, headers: corsHeaders });
            }
            await sleep(POLL_INTERVAL_MS);
          }
        }
      }

      return json({ error: "Not found" }, 404, corsHeaders);
    } catch (err) {
      return json({ error: err.message }, 500, corsHeaders);
    }
  },
};

function json(data, status, headers) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { ...headers, "Content-Type": "application/json" },
  });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// Tolerant UUID v1-v5 check; we just want to reject obviously bad
// input (path traversal, huge keys) without imposing a strict format.
function isValidUuid(s) {
  return (
    typeof s === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s)
  );
}

function isFreshTimestamp(signedAt) {
  const now = Math.floor(Date.now() / 1000);
  return Math.abs(now - signedAt) <= REGISTER_TIMESTAMP_SKEW_SECS;
}

/**
 * Decode an even-length lowercase hex string into a Uint8Array of
 * exactly `len` bytes. Returns null on any malformed input so the
 * caller can emit a uniform 400 without leaking which check failed.
 */
function hexToBytes(s, len) {
  if (typeof s !== "string" || s.length !== len * 2 || !/^[0-9a-fA-F]+$/.test(s)) {
    return null;
  }
  const out = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    out[i] = parseInt(s.substr(i * 2, 2), 16);
  }
  return out;
}

/**
 * Verify a 64-byte Ed25519 signature over `payload` against a 32-byte
 * raw public key. Uses `crypto.subtle` with `name: "Ed25519"`, which
 * Cloudflare Workers ship since 2023. Any importKey/verify failure
 * (malformed key, wrong sig, runtime without Ed25519) returns false
 * so the handler always closes with a clean 400.
 */
async function verifyEd25519(pubkeyBytes, payload, sigBytes) {
  try {
    const key = await crypto.subtle.importKey(
      "raw",
      pubkeyBytes,
      { name: "Ed25519" },
      false,
      ["verify"]
    );
    return await crypto.subtle.verify({ name: "Ed25519" }, key, sigBytes, payload);
  } catch (_e) {
    return false;
  }
}
