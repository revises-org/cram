<div align="center">

```
   ╭─────────╮
   │  c r a m │
   ╰─────────╯
```

**AI GATEWAY FOR DEVS**

Local gateway that terminates a static bearer token and re-signs requests for
cloud AI platforms — so any editor can talk to them.

[![CI](https://github.com/revises-org/cram/actions/workflows/ci.yml/badge.svg)](https://github.com/revises-org/cram/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/gateway-for-vertex-ai.svg)](https://crates.io/crates/gateway-for-vertex-ai)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

</div>

---

## The problem

Editors and coding agents send one thing: `Authorization: Bearer <key>`, a
static string you paste once.

Cloud AI platforms don't accept that.

| Platform | What it actually wants |
|---|---|
| **Vertex AI** | A service account JWT exchanged for an access token that expires every hour |
| **Bedrock** | SigV4 — every request signed against its own body and timestamp |
| **Azure AI Foundry** | Entra ID or managed identity, back to the token refresh loop |

cram sits on `127.0.0.1`, accepts the static token your editor can store, and
re-signs each request the way the platform expects.

> **Status:** Vertex AI works today. Bedrock and Azure are the direction, not a
> promise — see [Roadmap](#roadmap).

## What you get

**Gemini 3 tool calling that actually works.** Gemini 3 signs its reasoning and
requires the signature back on the next turn. Standard OpenAI clients drop the
non-standard field it arrives in, so the second turn of every tool-calling
conversation fails with `400 INVALID_ARGUMENT: Function call is missing a
thought_signature`. VS Code Copilot, Codex CLI, Continue and LiteLLM have all
hit this. cram carries the signature across turns, statelessly.

**A dashboard that tells you where the money went.** Cloud Billing lags a day
and aggregates everything. cram shows every request as it happens — model,
status, time to first token, and how much of your prompt was cached.

**One binary, ~15 MB of RAM.** No Python, no Node, no runtime to install.

## Quick start

```bash
cargo install cram
cram auth vertex --key-file /path/to/sa.json
cram
```

That's it. The dashboard opens in your browser.

<details>
<summary>Setting up the Google Cloud side</summary>

```bash
gcloud config set project my-project
gcloud services enable aiplatform.googleapis.com

gcloud iam service-accounts create cram
gcloud projects add-iam-policy-binding my-project \
  --member="serviceAccount:cram@my-project.iam.gserviceaccount.com" \
  --role="roles/aiplatform.user"

gcloud iam service-accounts keys create ~/.cram/sa.json \
  --iam-account=cram@my-project.iam.gserviceaccount.com
```

Billing must be enabled on the project, or the API refuses requests regardless
of permissions.

</details>

## What it looks like

```
   ╭─────────╮
   │  c r a m │  0.1.2
   ╰─────────╯

  gateway    http://127.0.0.1:8787
  dashboard  http://127.0.0.1:8787/_cram/
  upstream   vertex · my-project · global

  waiting for requests… (ctrl-c to stop)
```

And in the browser:

```
time      model                          status  duration  ttfb    in      out   reasoning  cached
12:16:54  google/gemini-3.7-flash        200     15.50s    4.03s   9581    1588  82         —
12:17:01  google/gemini-3.1-pro-preview  200     7.06s     6.75s   1676    5     604        —
12:17:30  google/gemini-3.7-flash        429     1.05s     —       —       —     —          —
12:19:21  google/gemini-3.7-flash        200     105.59s   97.78s  11185   1321  0          7642
```

Reading that: row two spent almost all its time thinking, not streaming. Row
three hit a quota limit. Row four waited 98 seconds before the first byte —
that's throttling, not a slow model. Row four also had 68% of its prompt served
from cache.

None of those are visible from a total duration alone.

## Connect your editor

Point any OpenAI-compatible client at `http://127.0.0.1:8787/v1` and use your
gateway key.

<details>
<summary>Zed</summary>

```json
{
  "language_models": {
    "openai_compatible": {
      "cram": {
        "api_url": "http://127.0.0.1:8787/v1",
        "available_models": [
          {
            "name": "gemini-pro",
            "display_name": "Gemini 2.5 Pro",
            "max_tokens": 1048576,
            "reasoning_effort": "medium",
            "capabilities": { "tools": true, "images": true }
          },
          {
            "name": "gemini-flash",
            "display_name": "Gemini Flash",
            "max_tokens": 1048576,
            "capabilities": { "tools": true, "images": true }
          }
        ]
      }
    }
  }
}
```

Enter the API key through Agent Settings so Zed stores it in your OS keychain,
not in `settings.json`.

Zed doesn't read `/v1/models` — models have to be declared here by hand.

</details>

Model names pass through unchanged. `gemini-pro` resolves through your aliases;
anything containing a `/` (`meta/llama-4-scout-maas`) or consisting of digits (a
self-deployed Model Garden endpoint ID) is sent as-is. New models work the day
Google ships them, with no update here.

## Configuration

`~/.cram/config.toml`, or `CRAM_HOME` to move it:

```toml
port = 8787

[vertex]
project = "my-project"
location = "global"          # regional endpoints cost 10% more on Claude 4.5+

[models]
gemini-pro = "google/gemini-2.5-pro"
gemini-flash = "google/gemini-3.7-flash"
```

Credentials live separately in `~/.cram/credentials.toml`, mode 0600.

Environment variables still work and take precedence: `GCP_PROJECT_ID`,
`GCP_LOCATION`, `GATEWAY_API_KEY`, `GOOGLE_APPLICATION_CREDENTIALS`,
`MODEL_ALIASES`, `BIND_ADDR`, `RUST_LOG`.

Order: CLI flag → environment → config file → default.

## CLI

```
cram                    start the gateway (same as cram serve)
cram serve              --port N, --no-open, --quiet
cram dash               open the dashboard
cram auth vertex        --key-file /path/to/sa.json
```

## Security

cram holds credentials that spend money, and sees every prompt you send —
which is to say, your source code.

- **Binds to `127.0.0.1`.** Exposing it on `0.0.0.0` puts an unmetered Vertex
  endpoint on your network. The risk is your quota, not your data.
- **The dashboard is read-only.** There is deliberately no endpoint that writes
  credentials: an unauthenticated write API on localhost is reachable by any web
  page you have open, since CORS blocks reading the response but not sending the
  request. Credentials go through the CLI.
- **Bodies are never logged**, in memory or on disk. Only metadata.
- **Authorization headers are redacted at the logging layer**, not optionally.

## Using it as a library

The binary is a thin wrapper. `cram-vertex` is published separately:

```rust
use cram_vertex::{router, AppState, Config};

let cfg = Config::new("my-project").with_gateway_key("secret");
let state = AppState::discover(cfg).await?;

let app = axum::Router::new()
    .route("/", get(home))
    .nest("/ai", router(state));
```

Implement `TokenSource` to supply your own credentials, or `Observer` to receive
a `CompletionEvent` per request — that's the same seam the dashboard uses.

## Roadmap

| | |
|---|---|
| **0.1.x** | Vertex AI, dashboard, config |
| **0.2** | Bedrock, frame-level request log, SQLite token history |
| **0.3** | Azure AI Foundry, Anthropic Messages route, JetBrains |

Scope is deliberate: only platforms whose auth an editor can't handle. OpenAI,
Anthropic direct, Groq and DeepSeek all take a static key — paste it into your
editor and skip the gateway.

## Known limits

- Anthropic models on Vertex aren't reachable yet; they use a different protocol
  and a separate route.
- `n > 1` is rejected rather than silently mishandled — tool call signatures are
  tracked for one choice.
- Tool call arguments don't stream character by character. They're buffered so
  the signature can be attached correctly, which is invisible in practice since
  a client can't invoke a tool before its arguments are complete.

## Related

`gateway-for-vertex-ai` was the original single-provider version of this. It's
now part of cram.

## License

[Apache 2.0](LICENSE). Copyright 2026 Huy Nguyen Nhu.

---

*Unofficial and community-maintained. Not affiliated with, endorsed by, or
sponsored by Google LLC, Amazon Web Services, or Microsoft. "Vertex AI",
"Bedrock" and "Azure" are trademarks of their respective owners.*
