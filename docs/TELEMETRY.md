# Telemetry

**Status:** design — not implemented. No telemetry code exists in the app today.

Opt-in, off by default, anonymous, coarse. Two questions justify it:

1. **Which providers do people actually configure?** Specifically, the Watch4.me
   attach rate (the funnel KPI) and the multi-provider ratio (does the
   "single pane of glass" thesis hold?).
2. **Are people on a version that still works?** Provider APIs change; we need to
   know which app versions are live before we can tell anyone their integration
   broke.

Everything below is scoped to those two questions. Anything that doesn't serve
them doesn't get collected.

---

## The claim, and what backs it

This is the part to get right, because the phrasing is load-bearing and it is
easy to drift into saying more than is true.

**Canonical user-facing copy** — use this verbatim:

> Telemetry is opt-in and off by default. When enabled, your app sends anonymous,
> coarse data to `scrub.opaqueresearch.com`, a separate service that strips source
> IP addresses before forwarding to uptimebar.app. uptimebar.app never receives
> your IP address. We do not log IPs at the relay, and we do not share them with
> the uptimebar.app service. Both are operated by Opaque Research LLC — this is a
> policy and operational control, not a cryptographic guarantee. If you want a
> mathematical guarantee that we cannot see your IP, that requires a neutral
> third-party relay (Oblivious HTTP), which we've deferred for cost reasons.

**Short form**, also shippable as-is:

> We run a privacy service on fly.io, enforcing privacy and stripping source IP
> addresses.

**Do NOT ship this phrasing:**

> ~~We only get the payload, not your source IP.~~

It states a policy choice in the language of a physical limit. The relay is our
own Fly account — we *can* read the IP there, in about four clicks. "NEVER sees"
and "we can't see" are physical-impossibility language and they are false here.
"We do not log" and "we do not share" are policy language and they are true.

Same wall. One sentence survives scrutiny.

### Why the last sentence stays in

Naming the limitation is what makes the rest credible. It costs nothing — this is
a free app, and nobody churns over being told exactly what we can and can't
promise. Cutting it is how the whole disclosure stops being believable.

### Claims → mechanism

| Claim | Mechanism | Kind |
|---|---|---|
| "Opt-in, off by default" | consent gate in settings; no send without it | technical |
| "uptimebar.app never receives your IP address" | relay strips `Fly-Client-IP` / `X-Forwarded-For` before forwarding | technical |
| "We do not log IPs at the relay" | relay app code does not log them | technical (revocable by a code change) |
| "We do not share IPs with uptimebar.app" | policy; separate service, separate host, separate codebase | **directive** |
| "Anonymous" | per-period random UUID, no stored mapping | technical |
| "Coarse" | bucketed counts, fixed schema, no `country` | technical |

The two rows that are policy rather than mechanism are the honest cost of not
paying for OHTTP. Say so; don't dress them up.

---

## Architecture

```
┌─────────────── Tauri app (macOS / Windows) ───────────────┐
│  consent gate (default OFF)                               │
│  payload builder → plaintext JSON                         │
└───────────────────────┬───────────────────────────────────┘
                        │ HTTPS (TLS)
                        ▼
        ┌── scrub.opaqueresearch.com — Caddy on Fly.io ────┐
        │  github.com/opaqueresearch/scrub                 │
        │  receives Fly-Client-IP / X-Forwarded-For        │
        │    (Fly injects these; not disableable)          │
        │  strips them before forwarding                   │
        │  logs nothing about the request at all           │
        │  dumb forwarder — no storage, no processing      │
        │  public repo + trust page serving live config    │
        └───────────────────────┬──────────────────────────┘
                                │ HTTPS (TLS)
                                ▼
        ┌──────────── hel3 (Caddy → ingest) ───────────────┐
        │  sees payload; connection originates from relay  │
        │  no client IP present in the request             │
        │  rollup to aggregates; short raw retention       │
        └──────────────────────────────────────────────────┘
```

**The invariant:** hel3 never receives a client connection. Every telemetry
request arrives from the relay.

**What this is:** separation of *infrastructure*, backed by directive control.
Different service, different host, different codebase, policy says don't join
them.

**What this is not:** separation of *parties*. Both are Opaque Research LLC. We
are the account holder on both. It's a recognized, defensible control, and one
whose limits we state rather than paper over. It works precisely because we
describe it as what it is.

### The relay: `scrub`

Built and public: **[github.com/opaqueresearch/scrub](https://github.com/opaqueresearch/scrub)**

Vanilla Caddy, 5-line Dockerfile, pinned base image. Deliberately small — the
trust argument is "you can read what it does in a minute," and an `xcaddy` build
with third-party modules would destroy that.

It serves a trust page at `/` and the **live config** at `/Caddyfile` (from disk,
not a copy that could drift). `UPSTREAM_URL` defaults to
`https://api.uptimebar.app` in the published config, so a reader sees the real
destination rather than a blank — while still being overridable at runtime
without a rebuild. The repo says plainly that a runtime override would be
invisible to an auditor; that's inherent to runtime config, and naming it is the
point.

**The logging config is the whole product, and the obvious version is wrong.**
Three separate paths can emit a client IP:

| Path | Why it leaks | Control |
|---|---|---|
| Access log | `remote_ip`, `client_ip`, `remote_port` | `output discard` — never logged |
| Error log | carries the *same* request object; `level ERROR` gates severity, not request context | `request delete` (object-level) + `exclude http.log.error` |
| Panics | Go's stdlib interpolates the IP **into the message string** — unfilterable | `exclude http.stdlib` |

Two traps worth knowing: loggers are **tee'd**, so a filtered logger doesn't stop
`default` from emitting the same request — the exclude list is load-bearing. And
**`delete` on a wrong field path silently no-ops**, indistinguishable from
success. Hence `discard` and object-level deletes over a list of leaf paths that
each have to be exactly right.

**Verified against Caddy 2.10.2**, not just reasoned about: 25 requests across 5
spoofed IPs and 4 code paths (200/403/405/404), injected via both
`X-Forwarded-For` and `Fly-Client-IP`. Zero IPs in output. The 502 error line
that appeared before `http.log.error` was excluded is gone.

**Protections:** method allowlist, shared header (a speed bump — UptimeBar is
open-source, the token is extractable, and you cannot authenticate an anonymous
client), 4KB body cap, timeouts, concurrency ceiling. No per-IP rate limiting —
it needs an `xcaddy` build, requires retaining IPs, and NAT makes it drop honest
traffic.

**Residual we can't close:** a fatal unrecovered crash dumps goroutine stacks to
stderr via Go's runtime, below anything a config controls. Unlikely, not
dismissable, and stated in the repo.

### No payload encryption

Both hops are TLS. There is no third party in the middle to hide the payload
from — encrypting it would be defending against ourselves. Plaintext JSON over
TLS on both hops.

This changes if we ever move to a neutral third-party relay (see
[Deferred: OHTTP](#deferred-ohttp)) — at that point the relay operator is someone
we *do* need the payload opaque to, and app-layer encryption (or OHTTP's HPKE
encapsulation) becomes load-bearing.

### Why Fly, and why not the alternatives

- **Fly.io** — already in the mix, CNAME-able from Porkbun, no new vendor. The
  relay is a dumb forwarder; any provider can do it, so "already have it" wins.

  **Fly injects `Fly-Client-IP` and `X-Forwarded-For` into every incoming request
  and this cannot be disabled at the platform level.** So the relay app
  *necessarily receives* the client IP, and stripping is something our code does
  — not a platform guarantee. This is not a reason to pick a different provider:
  every reverse proxy works this way, because seeing the client IP is how one
  functions. It is a reason to be precise in the copy (see
  [The claim](#the-claim-and-what-backs-it)).

  Fly's platform logging captures stdout/stderr only — no independent edge log
  recording IPs outside our control. So what the relay prints is what Fly
  retains, and that is entirely our code's behavior.
- **Cloudflare** — free tier requires nameserver delegation for the zone. We keep
  DNS at Porkbun. CNAME setup is Enterprise-only. Out.
- **A VPS we spin up** (Hetzner or otherwise) — buys nothing over just not
  logging the IP at hel3. Same trust boundary, extra box to patch. Out.
- **Adding a new CDN** (bunny.net, CloudFront) — new vendor, new bill, new auth
  surface, zero capability gain over Fly. Out.

The relay must not be *co-located with the ingest*. That's the whole point of the
directive control — separate infrastructure is what the policy is about.

---

## Payload

Plaintext JSON over TLS. Every field is coarse. **All keys are always present** —
see [Fixed schema](#fixed-schema-why-every-key-is-always-sent).

```json
{
  "period_id": "e2b1…random-uuid",
  "period": "2026-07",
  "app_version": "0.5.0",
  "os": "macos",
  "os_major": "15",
  "providers_configured": {
    "uptimerobot": true,
    "healthchecks": false,
    "betterstack": true,
    "watch4me": true
  },
  "monitor_counts_bucketed": {
    "uptimerobot": "6-20",
    "healthchecks": "0",
    "betterstack": "1-5",
    "watch4me": "1-5"
  }
}
```

### Fields

| Field | Why | Notes |
|---|---|---|
| `period_id` | distinct active installs per period | fresh random UUID per period; **no derivation from a persistent secret, no stored mapping** |
| `period` | bucket the counts | month granularity |
| `app_version` | which versions are live; compat gating | |
| `os` / `os_major` | platform mix; coarse | major only, never full build |
| `providers_configured` | **the funnel KPI** — Watch4.me attach rate, multi-provider ratio | fixed key set, always all four |
| `monitor_counts_bucketed` | scale distribution per provider | fixed key set; bucketed, never exact |

**Bucket boundaries:** `0`, `1-5`, `6-20`, `21-50`, `50+`.

**Provider set (fixed, exactly these four):** `uptimerobot`, `healthchecks`,
`betterstack`, `watch4me`.

Uptime Kuma is excluded — indefinitely paused, and being removed from the
codebase entirely (#33). Adding a fifth provider later is a **schema version
bump**, not an additive change, because the fixed set is what makes key-presence
leak nothing.

### Fixed schema: why every key is always sent

If we only sent configured providers, the *presence and absence of keys* would
answer the exact question we're protecting — "which services did they configure?"
— without reading a single value. Absent keys are unencrypted metadata.

So every payload carries every provider key, every time. Unconfigured providers
send `false` and `"0"`. No structural variance between users, nothing to read off
the shape.

This is why the provider set is closed and a new provider needs a version bump.

### What is NOT collected

Never, and no exceptions:

- API keys or tokens (any provider)
- Monitor names, URLs, hostnames, IDs
- Exact monitor counts (bucketed only)
- Token scope (RO/RW) — interesting to us, not needed for either question
- Which monitors are up/down/paused/muted
- Account identifiers of any kind
- `country` — see below

### No `country` field

Dropped deliberately. It's the only field that would need the source IP to
derive, and keeping it would give us a standing reason to want IPs at the relay.
Without it, the relay's IP visibility is incidental to TCP rather than a thing
we're extracting value from — which makes "we don't log it" far more credible.

Country would also be a re-identification risk in small-population regions,
combined with `os` + `app_version` + provider mix.

---

## Identity and linkability

`period_id` is a **fresh random UUID per period**, generated client-side.

- **Not** derived from an install ID, machine ID, or any persistent secret.
- **No** stored mapping, client-side or server-side.
- Rotation carries **timing jitter** so synchronized rotation at a period boundary
  can't be used to relink periods.

**What this buys:** distinct active installs per period. Periods are unlinkable
even to us — there is no mapping to subpoena, leak, or change our minds about.

**What it costs, deliberately:** no retention cohorts, ever. We cannot tell you
whether the installs active in July are the same ones active in August. That is a
real capability traded away on purpose. Revisit only as a conscious trade, not
by accident.

`uuid` v4 is already a dependency (`src-tauri/Cargo.toml`).

---

## Consent

- **Default OFF.** No payload leaves the app until the user opts in.
- **Layered disclosure** at first run: one sentence + a link to the detail. Not a
  buried privacy policy.
- **"Show me what's sent"** in settings — dumps the literal JSON payload that
  would be transmitted. Cheap to build and it disarms most objections.
- **Opt-out works and is visible** in settings, not buried.

Opt-in is the right call for this app even though it costs most of the data
(single-digit opt-in rates are typical). The product's pitch is trustworthiness;
opt-out telemetry undercuts the pitch for data we don't critically need.

### Version checking is independent of telemetry

The app must be able to check for a version/compat update **without** sending
telemetry. That request carries `app_version` only, no provider data, no
`period_id`. Users who decline telemetry still get told when their integration
breaks.

This means the provider breakdown is missing for opt-outs. That is honest data
loss and we accept it.

**The compat check routes through scrub too.** Decided, and it matters: if it
went straight to `api.uptimebar.app`, then opting out of telemetry would mean
your IP reaches uptimebar.app on *every* version check — undoing the architecture
for precisely the users who cared most about it. Same relay, same strip, one
trust story, no asterisk in the copy.

The cost is real and worth naming: scrub is now on the critical path for a
feature every user depends on, not just for opt-in telemetry. A scrub outage
breaks version checks. That argues for the app degrading quietly when the compat
check fails — a version check is not worth an error dialog — and for scrub
staying as boring as it currently is.

---

## Retention

**Raw events: 30 days. Then rolled up to aggregate counts and dropped.**
Aggregates are kept indefinitely — they're the product.

First, the thing that makes this cheap: **a period is one calendar month, not a
day.** An install that opts in sends roughly *one payload per month*. There is no
per-hour or per-day data anywhere; the payload has no time dimension finer than
which month it was sent in. So "30 days of raw data" is one row per opted-in
install, not a time series.

Why 30 days is the right number rather than a compromise:

- Periods are monthly, so a raw row past its period boundary has already been
  counted into that month's aggregate.
- `period_id` never repeats and has no stored mapping, so an old raw row can't be
  linked to anything — not to a later period, not to an install, not to a person.
  It is already spent.
- "A month" is legible to someone reading the disclosure. "Short" invites
  suspicion, and vague words never get tightened later.

We're not giving anything up. The per-period-UUID design already destroyed the
value of keeping raw rows; the retention window just makes that explicit.

**Ordering constraint, not an implementation detail:** the rollup job is what
*enforces* retention. If ingest ships before the rollup exists, raw rows
accumulate against a promise we aren't keeping. Ingest and rollup ship together
or not at all.

Retention here is policy, not platform-enforced (an OHTTP/Analytics-Engine style
setup would enforce it structurally). Write the job; don't rely on remembering.

---

## Deferred: OHTTP

[RFC 9458](https://www.rfc-editor.org/rfc/rfc9458.html) Oblivious HTTP is the only
arrangement that makes "we cannot see where your data came from" a *mechanism*
rather than a promise. A neutral third-party relay sees the client IP but cannot
read the payload; we decrypt the payload but never see the IP. Neither party has
both halves.

**Deferred for cost and complexity.** This is a free app with no revenue. A paid
privacy tier isn't viable — nobody pays for privacy on a free uptime app; they
click opt-out.

Research findings, current as of 2026-07-16, if this is ever revisited:

- **The Rust crate is ready.** `ohttp` + `bhttp` 0.8.0, actively maintained
  (Martin Thomson / Mozilla — an author of the RFC), final RFC 9458 + RFC 9292,
  not drafts. **Pure-Rust `rust-hpke` is the default feature** — NSS is opt-in, so
  no bindgen and no system libraries. The NSS path explicitly does not support
  Windows/macOS default library locations and would have been a blocker; the
  default path is not.
  - `ohttp = { version = "0.8.0", default-features = false, features = ["client", "rust-hpke"] }`
  - **Caveat:** upstream CI is `ubuntu-latest` only — no Windows/macOS runner.
    Windows viability is *inferred* from the pure-Rust dependency graph, not
    verified. Smoke-test early.
  - Pre-1.0 with active IETF churn — pin exact versions.
  - `Server` is single-keypair by design; gateway key rotation is ours to build.
- **Both vendors sell exactly the shape we'd want** — relay-only, forwarding to a
  gateway *we* operate and hold the key for. Neither wants to run the gateway;
  Fastly states the principle outright ("if any single entity were to operate the
  two double-blinded OHTTP services in conjunction… this would create a
  fundamental compromise").
- **Fastly** — sales contact required (`sales@fastly.com`), pricing unpublished,
  billed on bandwidth + per-10k-requests. Public OHTTP customers are Google
  Privacy Sandbox and Mozilla telemetry. Unverified whether they'd onboard a solo
  developer — **that's a two-question email, and it's the only real unknown.**
- **Cloudflare Privacy Gateway** — closed beta since 2022, Enterprise-only, never
  reached GA in four years. Not deprecated but clearly deprioritized. Write it off.
- **oblivious.network** — self-serve, $15/mo entry (2.62M req). Unverified on
  arbitrary-gateway support, SLA, track record (docs 404'd). Note the trust
  question: a small LLC's neutrality is an assertion, where Cloudflare's and
  Fastly's is backed by having more to lose.
- **orelay.dev** — free, explicitly testing-only, useful for building a gateway
  against without picking a vendor.
- **Divvi Up (ISRG)** — runs a *gateway*, not a relay; tells its own subscribers
  they must "arrange for a trusted party to run an OHTTP relay server, such as
  Fastly or Cloudflare; it's not possible for Divvi Up to operate the relay while
  providing privacy." Independent confirmation that the two-party split is
  structural, not a product limitation.
- **Tor2Web-style gateways are not a shortcut.** Without a client-side Tor
  circuit, the gateway sees the real IP *and* reads plaintext — a stranger holding
  both halves. Strictly worse than every option here.

**If OHTTP is ever adopted**, the payload gains app-layer encryption or HPKE
encapsulation (see [No payload encryption](#no-payload-encryption)), and the
canonical copy's last sentence gets replaced by the stronger claim.

---

## Open questions

- ~~**Does Fly's platform log client IP independently of our app?**~~
  **Resolved.** Fly's platform logging captures a process's stdout/stderr and
  nothing more — there is no independent edge log recording IPs behind our back.
  So the claim rests entirely on what our relay prints, which is our code and
  auditable. Fly stays.

  Fly *does* inject `Fly-Client-IP` / `X-Forwarded-For` into every request and
  this **cannot be disabled at the platform level** — so the relay unavoidably
  *receives* the IP. Stripping is our code's job, not a platform guarantee. This
  is true of every reverse proxy anywhere; seeing the client IP is how one works.
- ~~Relay hostname~~ **Decided: `scrub.opaqueresearch.com`**
  ([repo](https://github.com/opaqueresearch/scrub)). CNAME at Porkbun → Fly; DNS
  stays at Porkbun, no nameserver delegation.
- ~~Retention window~~ **Decided: 30 days raw, then rollup.** Aggregates kept
  indefinitely. See [Retention](#retention) — a period is a calendar month, so
  this is one row per opted-in install, not a time series.
- ~~Does the compat check route through scrub?~~ **Decided: yes.** Otherwise
  opting out of telemetry would send your IP to uptimebar.app on every version
  check. See [Version checking is independent of
  telemetry](#version-checking-is-independent-of-telemetry).

Still open — none block this doc, all are implementation against systems that
don't exist yet:

- **Rollup job** — where it runs, on what schedule. Note the ordering constraint
  in [Retention](#retention): it ships *with* ingest, not after.
- **Consent UX copy** — the layered first-run text. Requirements are specified in
  [Consent](#consent); the strings should be written against a real settings pane,
  not blind.
- **Compat feed design** — routing is settled (through scrub), but the feed itself
  — provider API version tracking, app update notification, what the response
  looks like — needs its own doc.
- **scrub deployment** — `fly deploy`, the staging leak test (CI can't inject
  `Fly-Client-IP`; that's Fly's proxy), and `SCRUB_TOKEN`.

---

## Implementation notes

Nothing here exists yet. When it does:

- Consent state belongs in `config.rs` (non-secret, `tauri-plugin-store`).
- `reqwest` is already a dependency (rustls-tls).
- `uuid` v4 is already a dependency.
- The payload builder should be pure and testable — take state, return JSON — so
  "show me what's sent" and the actual send call the same code. If they can
  diverge, they will, and the disclosure becomes a lie.
