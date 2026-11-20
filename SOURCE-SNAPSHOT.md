# ARTEX Source Snapshot (immutable reference)

- Repository: https://github.com/OverHzn/ARTEX (fork of https://github.com/BaaRai/ARTEX)
- Cloned at: `5cb44fb`
- Full commit SHA: `5cb44fb96965f9604a9a2c282cc713fdedf3c6dd`
- Commit date: 2026-11-14 13:19:38 +0100
- Subject: Merge pull request #226 from BaaRai/feat/versioned-migrations
- Working tree: CLEAN (`git status` empty)
- Tracked files: 791
- Location: `/workspaces/Make-M/ARTEX` (READ-ONLY reference — never modify)

## Manifests snapshotted into research/
- go.mod, go.sum (Go module: github.com/Autumn-27/artex, go 1.26.10)
- config.example.json, .env.example

## Original build/run (from README, verified against source)
- Prereqs: PostgreSQL + LLM key (ANTHROPIC_API_KEY or OPENAI_API_KEY)
- Dev: `./dev.sh` (backend :8787, proxy :8788, web :5173)
- Prod binary: `CGO_ENABLED=0 go build -tags embedui -o artex ./cmd/artex` (after `web/out` -> `server/webui/dist`)
- Docker: `docker compose up -d --build` (postgres + artex, :8787)
- Flags: `-addr` (127.0.0.1:8787), `-data`, `-proxy` (127.0.0.1:8788)
