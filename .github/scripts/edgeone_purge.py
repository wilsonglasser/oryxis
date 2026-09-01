#!/usr/bin/env python3
"""Drop paths from the EdgeOne cache that fronts dl-cn.oryxis.app.

The mirror bucket writes `releases/*.json` with `max-age=300`, and the
Cloudflare side honours it, but an EdgeOne node caches on its own TTL and
keeps serving what it has. `latest.json` is the one file every client asks
for on an update check, so it is the one that stays hot and therefore the
one that goes stale: a release can be live on GitHub and on dl.oryxis.app
while mainland China is still told about the previous version. Purging it
is the only thing that moves it.

Stdlib only, and the TC3 signature by hand, deliberately: `tccli` pulls a
large dependency tree into a job whose whole purpose is to make four HTTP
calls, and its endpoint handling for international accounts is one more
thing that can be configured wrong in a place nobody looks until a release
is out.

Usage: edgeone_purge.py <url> [<url> ...]
Reads TENCENT_SECRET_ID, TENCENT_SECRET_KEY and EDGEONE_ZONE_ID.
"""

import hashlib
import hmac
import json
import os
import sys
import time
from datetime import datetime, timezone
from urllib import request as urlrequest
from urllib.error import HTTPError, URLError

HOST = "teo.intl.tencentcloudapi.com"
SERVICE = "teo"
ACTION = "CreatePurgeTask"
VERSION = "2022-09-01"
# `delete` drops the node copy outright. `invalidate`, the API default,
# revalidates against the origin and keeps the cached body on a 304, which
# is exactly the answer a correctly-cached-but-stale object gives.
METHOD = "delete"


def _hmac(key: bytes, msg: str) -> bytes:
    return hmac.new(key, msg.encode(), hashlib.sha256).digest()


def canonical_request(payload: str, headers: list[tuple[str, str]]) -> str:
    """The canonical form the signature is taken over.

    `headers` are the signed ones, in the order they are signed (lower
    case, sorted by name, which is the caller's job because it is also the
    order that goes in `SignedHeaders`).
    """
    canonical_headers = "".join(f"{name}:{value}\n" for name, value in headers)
    return "\n".join(
        [
            "POST",
            "/",
            "",
            canonical_headers,
            ";".join(name for name, _ in headers),
            hashlib.sha256(payload.encode()).hexdigest(),
        ]
    )


def string_to_sign(canonical: str, scope: str, ts: int) -> str:
    return "\n".join(
        [
            "TC3-HMAC-SHA256",
            str(ts),
            scope,
            hashlib.sha256(canonical.encode()).hexdigest(),
        ]
    )


def _authorization(secret_id: str, secret_key: str, payload: str, ts: int) -> str:
    """A TC3-HMAC-SHA256 header for this one request shape."""
    date = datetime.fromtimestamp(ts, timezone.utc).strftime("%Y-%m-%d")
    headers = [
        ("content-type", "application/json"),
        ("host", HOST),
        ("x-tc-action", ACTION.lower()),
    ]
    scope = f"{date}/{SERVICE}/tc3_request"
    to_sign = string_to_sign(canonical_request(payload, headers), scope, ts)
    signing_key = _hmac(_hmac(_hmac(f"TC3{secret_key}".encode(), date), SERVICE), "tc3_request")
    signature = hmac.new(signing_key, to_sign.encode(), hashlib.sha256).hexdigest()
    return (
        f"TC3-HMAC-SHA256 Credential={secret_id}/{scope}, "
        f"SignedHeaders={';'.join(name for name, _ in headers)}, Signature={signature}"
    )


def selftest() -> int:
    """Check the signing construction against Tencent's own worked example.

    The published example masks its SecretKey, so the final signature
    cannot be reproduced; what it does publish are the two intermediate
    digests, and those cover the part that actually goes wrong (the field
    order and the blank lines inside the canonical request). A credential
    error announces itself on the first call; a malformed canonical request
    is a signature mismatch that reads exactly like a wrong key.
    """
    payload = '{"Limit": 1, "Filters": [{"Values": ["unnamed"], "Name": "instance-name"}]}'
    canonical = canonical_request(
        payload,
        [
            ("content-type", "application/json; charset=utf-8"),
            ("host", "cvm.tencentcloudapi.com"),
        ],
    )
    checks = [
        (
            "payload digest",
            hashlib.sha256(payload.encode()).hexdigest(),
            "99d58dfbc6745f6747f36bfca17dee5e6881dc0428a0a36f96199342bc5b4907",
        ),
        (
            "canonical request digest",
            hashlib.sha256(canonical.encode()).hexdigest(),
            "2815843035062fffda5fd6f2a44ea8a34818b0dc46f024b8b3786976a3adda7a",
        ),
    ]
    failed = False
    for name, got, want in checks:
        ok = got == want
        failed |= not ok
        print(f"{'ok  ' if ok else 'FAIL'} {name}: {got}")
    return 1 if failed else 0


def main() -> int:
    targets = sys.argv[1:]
    if targets == ["--selftest"]:
        return selftest()
    if not targets:
        print("nothing to purge", file=sys.stderr)
        return 2
    try:
        secret_id = os.environ["TENCENT_SECRET_ID"]
        secret_key = os.environ["TENCENT_SECRET_KEY"]
        zone_id = os.environ["EDGEONE_ZONE_ID"]
    except KeyError as missing:
        print(f"missing {missing.args[0]}", file=sys.stderr)
        return 2

    payload = json.dumps(
        {"ZoneId": zone_id, "Type": "purge_url", "Method": METHOD, "Targets": targets},
        separators=(",", ":"),
    )
    ts = int(time.time())
    req = urlrequest.Request(
        f"https://{HOST}",
        data=payload.encode(),
        method="POST",
        headers={
            "Authorization": _authorization(secret_id, secret_key, payload, ts),
            "Content-Type": "application/json",
            "Host": HOST,
            "X-TC-Action": ACTION,
            "X-TC-Timestamp": str(ts),
            "X-TC-Version": VERSION,
        },
    )
    try:
        with urlrequest.urlopen(req, timeout=30) as resp:
            body = json.loads(resp.read().decode())
    except (HTTPError, URLError) as e:
        print(f"purge request failed: {e}", file=sys.stderr)
        return 1

    # The API answers 200 with an Error member rather than an HTTP status,
    # so a request that "worked" still has to be read.
    response = body.get("Response", {})
    if "Error" in response:
        err = response["Error"]
        print(f"purge refused: {err.get('Code')} {err.get('Message')}", file=sys.stderr)
        return 1
    print(f"purged {len(targets)} url(s), job {response.get('JobId')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
