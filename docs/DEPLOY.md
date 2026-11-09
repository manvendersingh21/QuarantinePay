# Deploying QuarantinePay

## Public demo via Cloudflare quick tunnel (current deployment)

The live demo runs the full stack on the GPU development machine: the Rust server, PostgreSQL, the local Qwen3 model and real PayPal Sandbox. It is published through a Cloudflare quick tunnel:

```bash
scripts/public-tunnel.sh start    # prints https://<random>.trycloudflare.com
scripts/public-tunnel.sh status
scripts/public-tunnel.sh stop
```

- The server stays bound to `127.0.0.1:3000`. Only the tunnel is public, and every mutation still needs the administrator password (bearer token, or session cookie plus the Origin check).
- The script sets `QP_PUBLIC_BASE` to the tunnel URL. The browser Origin check, `Secure` cookies and the PayPal return URL all depend on it.
- A quick tunnel is temporary: its URL changes whenever the tunnel restarts, and it serves only while the machine and processes run. Re-run `start` and update any shared link afterwards.
- Verified on 2026-11-09: `scripts/sandbox-demo.cjs` ran both stories through the public HTTPS URL against live PayPal Sandbox in 152 s. The injected delivery's authorization `64L774879U300943H` is **VOIDED** and the compliant delivery's authorization `0EV5186317836644V` is **CAPTURED**, both confirmed by fresh PayPal API reads.

# Deploying to Render (alternative, not deployed)

Status: **not yet deployed or tested on Render.** The Blueprint (`render.yaml`) and these steps come from the code's configuration behaviour and Render's documented Blueprint features. Check plan names and prices against Render's current pricing page before applying. Docker builds were also not run in the authoring workspace.

## What you get

- One Docker web service built from the repository `Dockerfile`. It runs the Rust server and the delivery verifier, and serves the UI and WASM assets.
- One managed PostgreSQL 16 database on Render's private network. `DATABASE_URL` is injected from its internal connection string.
- One 1 GB persistent disk at `/app/.local` for the Ed25519 signing key.
- `QP_MODE=replay` by default (simulated payments).

## What you do not get on Render

- **No GPU and no local model.** Private inference only accepts a loopback model URL (`127.0.0.1`), and nothing listens there on Render. The buyer planner and delivery analyst fall back to the rule parser and rule-only analysis, and the task and order label it (`planner.source = "rules"`). Payment safety does not depend on the model. To show model-based planning, run locally with Ollama (README Quickstart step 2). Pointing private mode at a remote model host is rejected by design. The only remote option is the opt-in Muse provider: `QP_PRIVATE_INFERENCE=false`, `QP_INFERENCE_PROVIDER=muse`, `MUSE_API_KEY`. That sends prompts and delivery text to a third party, so the private-inference claims no longer apply. The Muse path for the structured planner and analyst is implemented but **has not been tested**.
- **No Elasticsearch or Channel3** unless configured. Both are optional and unrelated to settlement.

## Step by step

1. **Push the repository** to GitHub (or GitLab) with `render.yaml` at the root. Never commit `.env` or `.local/`.
2. In the Render Dashboard, choose **New → Blueprint**, select the repository and review the plan. It creates `quarantinepay` (web) and `quarantinepay-db` (Postgres).
3. **Fill in the `sync: false` variables** when prompted:
   - `QP_ADMIN_PASSWORD`: at least 24 characters, for example `openssl rand -hex 24`. Store it in a password manager. It is both the login password and the API bearer token.
   - `QP_PUBLIC_BASE`: the service's exact origin, for example `https://quarantinepay.onrender.com`, with no trailing slash. If you don't know the final hostname yet, enter a placeholder, deploy, copy the URL from the service page, then update this variable and redeploy. The server compares the browser `Origin` header with this value for login and every form post. A mismatch shows "origin rejected". An `https://` base also marks the session cookie `Secure`.
   - `PAYPAL_MERCHANT_ID`: leave it empty in replay mode. An empty value is treated as unset, so the replay merchant is used.
   - `PAYPAL_CLIENT_ID`, `PAYPAL_CLIENT_SECRET`, `PAYPAL_WEBHOOK_ID`: leave them unset (or delete them) in replay mode.
4. **Apply** and wait for the Docker build. It compiles the release workspace and installs `wasm-bindgen-cli`, so expect a long first build.
5. Check `https://<service>.onrender.com/health`. It should return `{"status":"ok"}`. Then sign in at `/` and open `/agent`. Migrations run automatically on startup.

### Port binding

The server binds to `QP_BIND` when it is set and non-empty. Otherwise it binds to `0.0.0.0:$PORT` when `PORT` is set. The Dockerfile sets `QP_BIND=0.0.0.0:3000`, which would win over Render's `PORT`. The Blueprint therefore pins **both** `QP_BIND=0.0.0.0:10000` and `PORT=10000` (Render's default port) explicitly. That way the bind address doesn't depend on whether Render passes an empty variable through. If you change one, change both. Environment values are not shell-expanded, so `QP_BIND=0.0.0.0:$PORT` would not work.

### Signing key and the persistent disk

The server creates `.local/signing-key` (relative to `/app`) on first start. That key signs purchase intents and browser sessions. The worker also **re-verifies every in-flight order's signed intent** with it on each pass.

- **With the disk** (paid instance type, as in the Blueprint), the key survives deploys and restarts.
- **Without a disk** (for example on a free instance), every deploy or restart generates a new key. Sessions end (sign in again), unconsumed intents are rejected, and **in-flight orders stop being processed**: they are neither captured nor voided by the app. In Sandbox mode, finish or void every open order before redeploying. If one is stranded, void its authorization in the PayPal Sandbox dashboard, or let it expire, and treat it as needing reconciliation.
- **Unverified risk:** the image runs as UID 65532, and on startup the server does `chmod 700 .local`. If the mounted disk is owned by root and not writable by that UID, startup fails with a permission error in the deploy logs. Check the first deploy's logs for this.
- To migrate, back up `.local/signing-key` from the old host and restore it on the new one. Treat it like a private key.

### Free instances

A free web service spins down when idle. While it is asleep, the background worker is not running: deadlines, voids and captures wait until the next request wakes it. Free instances also cannot attach a disk (see above), and free Postgres expires after 30 days. For judging, use a paid web instance and a paid database. For a short replay-only preview, free is acceptable.

## Switching to PayPal Sandbox

1. Create a Sandbox **business** account and a separate Sandbox **buyer** account in the PayPal Developer Dashboard, plus a REST app on the business account.
2. Set `PAYPAL_CLIENT_ID`, `PAYPAL_CLIENT_SECRET` and `PAYPAL_MERCHANT_ID` (the 13-character merchant ID of the same business account). Then set `QP_MODE=sandbox` and redeploy. Startup fails fast if any of these are missing or malformed. Production PayPal endpoints are never used.
3. Optional webhooks: register `https://<service>.onrender.com/api/webhooks/paypal` in the REST app and set `PAYPAL_WEBHOOK_ID`. Webhooks are reconciliation hints only, and settlement does not depend on them.
4. PayPal returns the buyer to `QP_PUBLIC_BASE/paypal/return` after approval. Then press **Authorize** in the app.
5. Replay and Sandbox data are isolated by mode. Switching modes hides the other mode's orders but does not delete them.

## Security notes

- The database has an empty `ipAllowList`, so it is reachable only from Render's private network.
- Keep `QP_ADMIN_PASSWORD` and the PayPal secrets only in Render's environment settings. Rotate them if they ever appear in logs, screenshots or a recording.
- This is a single-administrator prototype. Do not put it behind a public link with a shared password and real personal data.
