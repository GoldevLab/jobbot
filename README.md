# JobBot

Resuma dashboard + Rust worker that discovers jobs on [web3.career](https://web3.career), drafts application answers with **ADK OpenRouter** (free model), and applies on **Recruitee** boards through your local **Chrome CDP** session.

## Setup

```bash
git clone https://github.com/GoldevLab/jobbot.git
cd jobbot
cp .env.example .env
# put OPENROUTER_API_KEY in .env (never commit it)
chmod +x scripts/chrome-cdp.sh
```

Depends on [GoldevLab/resuma](https://github.com/GoldevLab/resuma) via git. For a sibling local checkout, copy `.cargo/config.toml.example` → `.cargo/config.toml`.

If sqlx says `migration N was previously applied but has been modified`, you edited an already-applied SQL file. Prefer new migrations (`002_…`) only; for local recovery, restore the file or update the checksum in `_sqlx_migrations` (backup `data/jobbot.db` first).

1. Start Chrome CDP (dedicated profile — your main Chrome can stay open). **Log into Google once** in that window:

```bash
./scripts/chrome-cdp.sh
```

2. In another terminal **from `apps/jobbot`** (not falseworld/vrmedia):

```bash
cd ~/Documentos/apps/jobbot
resuma dev
# equivalent: cargo run
```

If `resuma dev` complains about `public`, that folder must exist (it does now). Open the URL Resuma prints (often `http://127.0.0.1:3000`).

Each discover also seeds the Tether Norway Recruitee role for the apply path.

## Smoke checks

```bash
cargo test -q
./scripts/chrome-cdp.sh
cargo run -- --smoke
```

Expected: OpenRouter OK, jobs in DB (incl. Tether), Chrome opens the Tether careers page.

## Env

| Variable | Purpose |
|----------|---------|
| `OPENROUTER_API_KEY` | OpenRouter key |
| `OPENROUTER_MODEL` | default `openai/gpt-oss-20b:free` |
| `JOBBOT_CHROME_CDP` | `http://127.0.0.1:9222` |
| `JOBBOT_CHROME_PROFILE` | Chrome user-data-dir |
| `JOBBOT_CV_PATH` | Absolute path to PDF CV |
| `JOBBOT_AUTO_APPLY` | seed preference; toggle also in Settings |
| `JOBBOT_RATE_LIMIT_SECS` | pause between worker cycles |

## Flow

`discover` → `score` (LLM) → `draft` (LLM, human-ish tone) → `apply` (Chrome CDP)

**Auto-apply ATS (Rust/CDP only — no vendor SDKs):**

| Host | Module |
|------|--------|
| Recruitee / `careers.tether` | `sources/recruitee.rs` |
| Greenhouse (`boards.greenhouse.io`, …) | `sources/greenhouse.rs` |
| Ashby (`*.ashbyhq.com`) | `sources/ashby.rs` |

Lever / unknown hosts keep the draft as **manual**. Submit confirmation requires page text and/or URL+form signal — a bare click is **not** marked `applied`. Captchas → `failed` for manual fix.

## Profile coach (parallel)

Separate tokio worker + UI at `/profile`. Improves **GitHub** (public API snapshot), **LinkedIn** (from Settings notes — no scrape), and **general** cross-profile consistency.

- Own flag: `profile_worker_running` (Run/Stop on Profile page)
- Own tables: `profile_suggestions`, `profile_events` (apply `events` / `jobs` untouched)
- No Chrome — never fights the apply CDP session
- Paste LinkedIn About into **Settings → Profile notes**, then **Analyze now**

Optional: `JOBBOT_PROFILE_AUTO_START=1`

## Fly.io (always-on search)

Keeps discovering / scoring / drafting jobs after you shut the PC.
**Fly login** (e.g. `info@synergiart.com`) is only hosting/billing.
**Applicant identity** is always Golfredo (`golfredo.pf@gmail.com` in settings + your CV) — never the employer account.

```bash
cd ~/Documentos/apps/jobbot
chmod +x scripts/fly-deploy.sh
./scripts/fly-deploy.sh
```

- `JOBBOT_AUTO_START=1` — worker runs without clicking Run
- `JOBBOT_AUTO_APPLY=false` on Fly (no Chrome CDP); apply locally if needed
- Dashboard: `https://golfredo-jobbot.fly.dev/`

## Security

- `.env` is gitignored. Rotate the OpenRouter key if it was pasted in chat.
- Never put employer/boss contact data in JobBot settings.
