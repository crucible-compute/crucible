# CLAUDE.md — Project conventions for Claude Code

## Build & test

```bash
# Build everything
cargo build --workspace

# Run unit tests (fast, no cluster needed)
cargo test --workspace

# Run integration tests (requires Kind cluster via `just test-env`)
cargo test --package crucible-operator --test platform_lifecycle -- --ignored --test-threads=1

# Format check
cargo fmt --all --check

# Lint
cargo clippy --workspace --all-targets

# Helm lint
helm lint charts/crucible
```

## Project structure

- `crates/crucible-types` — CRD structs, state machine, error types. No async runtime.
- `crates/crucible-store` — Object store client (aws-sdk-s3). Async.
- `crates/crucible-operator` — Kubernetes operator binary. kube-rs Controller.
- `crates/crucible-api` — REST API server (axum). Skeleton.
- `crates/crucible-cli` — CLI binary (clap). Skeleton.
- `charts/crucible/` — Helm chart (CRDs, RBAC, operator + API deployments).
- `docker/` — Dockerfiles for operator, API, Spark base, Flink base.
- `test/` — Kind config, MinIO fixture, sample CRs, Helm values for local testing.
- `justfile` — Non-cargo task runner (images, Kind, Helm).

## Key conventions

- Rust 2024 edition, MSRV 1.88
- kube-rs 3.0, k8s-openapi 0.27, schemars 1
- All CRD structs use `#[serde(rename_all = "camelCase")]`
- rustls crypto provider must be initialized before kube client creation
- Server-side apply (`Patch::Apply`) for idempotent reconciliation
- `thiserror` at crate boundaries, `anyhow` for internal plumbing
- Integration tests are `#[ignore]` — skipped by `cargo test`, run explicitly
- Living docs: `notes.md` (commands), `troubleshooting.md` (problems & fixes)
