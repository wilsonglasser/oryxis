/**
 * Oryxis Sync Signaling Server
 * Cloudflare Worker + KV
 *
 * Routes:
 *   POST /register         — register device IP:port
 *   GET  /lookup/:id       — look up peer's IP:port
 *   DELETE /register/:id   — unregister device
 *
 * All requests require: Authorization: Bearer <SIGNALING_TOKEN>
 * Set the token via: wrangler secret put SIGNALING_TOKEN
 */

const TTL = 300; // 5 minutes

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname;

    // CORS headers
    const corsHeaders = {
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Allow-Methods": "GET, POST, DELETE, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type, Authorization",
    };

    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders });
    }

    // Auth check
    const auth = request.headers.get("Authorization") || "";
    const token = auth.replace("Bearer ", "");
    if (!env.SIGNALING_TOKEN || token !== env.SIGNALING_TOKEN) {
      return json({ error: "Unauthorized" }, 401, corsHeaders);
    }

    try {
      // POST /register
      if (request.method === "POST" && path === "/register") {
        const body = await request.json();
        const { device_id, public_key_fp, ip, port } = body;

        if (!device_id || !ip || !port) {
          return json({ error: "Missing fields" }, 400, corsHeaders);
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
        });

        return json({ ok: true, ttl: TTL }, 200, corsHeaders);
      }

      // GET /lookup/:id
      if (request.method === "GET" && path.startsWith("/lookup/")) {
        const deviceId = path.replace("/lookup/", "");
        const value = await env.SYNC_KV.get(`device:${deviceId}`);

        if (!value) {
          return json({ error: "Not found" }, 404, corsHeaders);
        }

        return json(JSON.parse(value), 200, corsHeaders);
      }

      // DELETE /register/:id
      if (request.method === "DELETE" && path.startsWith("/register/")) {
        const deviceId = path.replace("/register/", "");
        await env.SYNC_KV.delete(`device:${deviceId}`);
        return json({ ok: true }, 200, corsHeaders);
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
