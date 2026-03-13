# Crucible — Tasks & Milestones

## Conventions

**Cloud credentials:** Integration tests use locally configured `gcloud` and `aws` CLI credentials. No service account keys or credential files are checked in. Tests assume `aws sts get-caller-identity` and `gcloud auth print-access-token` succeed before running.

**Teardown:** Every milestone's integration tests clean up after themselves — delete CRs, pods, PVCs, and any cloud resources (buckets, nodes) created during the run. Teardown runs even on test failure (`if: always()` in CI, cleanup fixtures in test harness). No resources should persist between milestone test runs.

**Living docs:** At the end of every milestone, update:
- `notes.md` — useful commands discovered during development (kubectl snippets, cargo flags, helm overrides, debug invocations, etc.)
- `troubleshooting.md` — problems encountered and their fixes (build errors, Kind gotchas, CRD conflicts, image pull issues, etc.)

These are append-only working documents, not polished docs. They accumulate institutional knowledge as the project progresses.

**Git push:** At the end of every milestone, commit all work and push to the remote. Each milestone should land as one or more commits on `main` before starting the next.

**Tool and credential prerequisites:** When a required tool is missing (e.g. `just`, `helm`, `kind`) or credentials are expired (e.g. `docker login`, `aws` CLI, `gcloud`), ask the user to install or re-authenticate rather than circumventing or running underlying commands directly.

---

## Decisions

**Validation strategy:** CEL validation rules on CRD schemas for structural checks (required fields, enum values, format constraints). Semantic validation (tenant queue exists, platform healthy) handled in-reconciler — sets `status.error` on invalid CRs. No admission webhooks in v0.1; avoids cert-manager dependency and webhook-outage risk.

**Crate layout:** `crucible-types` stays lightweight (CRD structs, state machine, error types — no async runtime). Object store client lives in a separate `crucible-store` crate to isolate heavy dependencies (`aws-sdk-s3`, `tokio`, `bytes`). Workspace: `crucible-types`, `crucible-store`, `crucible-operator`, `crucible-api`, `crucible-cli`.

**Dependency management:** Operator-managed, not Helm sub-charts. Helm installs only the operator + CRDs. The operator deploys all sub-components (Celeborn, Volcano, Flink Operator, History Server) from the `CruciblePlatform` spec via kube-rs. For Volcano and Flink Operator (which have their own CRDs), the operator applies versioned manifest bundles — equivalent to `kubectl apply`. No dual-ownership between Helm and the operator.

**Web framework:** Axum. Shares the tokio runtime and Tower middleware ecosystem with kube-rs. No runtime bridging needed.

**Rust edition and MSRV:** Rust 2024 edition. MSRV = 1.88 (kube 3.0 minimum). Set `rust-version = "1.88"` in workspace `Cargo.toml`.

**Error handling:** `thiserror` at crate boundaries (public API types that other crates consume — maps to HTTP status codes in the API server). `anyhow` for internal plumbing (reconciler internals where errors are logged and retried).

**Dependency versions (workspace `Cargo.toml`):** Use `[workspace.dependencies]` for version inheritance. Pin major/minor, `Cargo.lock` handles patch. Key pins: `kube` 3.0, `k8s-openapi` 0.27 (with `latest` feature), `schemars` 1, `tokio` 1, `axum` 0.8, `serde`/`serde_json` 1, `aws-sdk-s3` latest. Managed dependency versions: Celeborn 0.5.x, Volcano 1.10.x, Flink Kubernetes Operator 1.10.x — pinned in `values.yaml` under `versions.*`.

**CRD API version:** `v1alpha1` for all CRDs (`CruciblePlatform`, `CrucibleSparkJob`, `CrucibleFlinkJob`). Standard K8s convention for pre-stable schemas. Migration to `v1` is a v1.0 concern.

**Container registry:** GHCR (`ghcr.io/crucible-compute/crucible-*`) for published images. `kind load docker-image` for local dev and CI integration tests. Helm chart defaults to GHCR with `image.repository` override.

**Task runner:** `just` (justfile) for non-cargo orchestration (docker build, kind setup, helm lint, image loading). All Rust tasks use `cargo` directly. No Makefile.

**Logging:** `tracing` + `tracing-subscriber` with JSON output in production, pretty-print in dev.

---

## Milestone 0: Project Scaffolding

### 0.1 Rust workspace setup
- Cargo workspace with crates: `crucible-types`, `crucible-store`, `crucible-operator`, `crucible-api`, `crucible-cli`
- `crucible-types`: CRD structs, job state machine, tenant-to-queue mapping, error types (`thiserror`). No async runtime.
- `crucible-store`: object store client (async, `aws-sdk-s3`). Depends on `crucible-types` for path layout.
- Workspace `Cargo.toml` with `[workspace.dependencies]` for version inheritance (see Decisions)
- Rust 2024 edition, `rust-version = "1.88"` (kube 3.0 minimum)
- Dockerfiles for operator and API server (multi-stage builds, scratch/distroless base)
- `.github/workflows/ci.yaml`: `cargo fmt --check`, `cargo clippy`, `cargo test --lib` on every PR
- `justfile` with targets for non-cargo tasks (see 0.4, 0.5)

### 0.2 Helm chart skeleton
- `charts/crucible/` with Chart.yaml, values.yaml, templates directory
- CRD templates (installed before anything else)
- Namespace, ServiceAccount, RBAC (ClusterRole + ClusterRoleBinding for operator)
- RBAC scoped to least privilege: CRD CRUD, pod/service/statefulset lifecycle, lease for leader election, pod/log read for log streaming — no wildcard verbs or resources
- Placeholder templates for each sub-component (operator Deployment, API server Deployment)
- `helm template` and `helm lint` pass in CI

### 0.3 Object store client abstraction (`crucible-store`)
- Trait-based object store client abstracting PUT, GET (byte-range), multipart upload, DELETE
- S3 implementation (via `aws-sdk-s3`) — first and only backend for v0.1. GCS and ABFS stubbed behind feature flags.
- MinIO-compatible for local/CI testing (same S3 implementation with endpoint override)
- Path builder utility enforcing the bucket layout from the overview (`logs/{spark|flink}/{job-id}/...`, `spark-events/`, `flink-checkpoints/`, etc.)
- Unit tests: path construction for all job types and log sources

### 0.4 Base container images
- Dockerfile for Crucible Spark 4.x base image (Java 21, no Scala suffix — Spark 4.x dropped it)
- Dockerfile for Crucible Flink 2.x base image (Java 21, scala_2.12 in tarball name only — Flink 2.0 deprecated Scala API)
- `just images` to build locally, `just push-images` to push to GHCR
- For Kind: `just load-images` wraps `kind load docker-image`
- Images tagged with Crucible version and `latest`
- Required by M2/M4 integration tests — must be buildable before those milestones

### 0.5 Kind + MinIO test harness
- `test/kind-config.yaml` (3-worker cluster)
- `test/fixtures/minio.yaml` (MinIO Deployment + Service + default bucket)
- `test/fixtures/platform.yaml` — canonical `CruciblePlatform` sample CR (v1alpha1) used by all integration tests
- `test/kind-values.yaml` (Helm values for local testing)
- `just test-env` stands up Kind + MinIO + Helm install + loads images
- `just teardown` destroys the Kind cluster
- Verify: `just test-env` works from a clean machine with Docker, Kind, Helm, just installed

**Depends on:** nothing
**Exit criteria:** `cargo build --workspace` succeeds, `helm template` passes, `just test-env` creates a running cluster with Crucible CRDs installed, object store client passes unit tests

---

## Milestone 1: CRDs & Operator Skeleton

### 1.1 CRD definitions (`crucible-types`)
- All CRDs use API version `v1alpha1`
- CEL validation rules on CRD schemas for structural checks (required fields, enum values, format)
- `CruciblePlatform` spec and status structs (using `kube-rs` derive macros)
  - Spec: Celeborn config (workers, storage), Volcano config, History Server config, API server config, object store config
  - Status: conditions per sub-component (`CelebornReady`, `VolcanoReady`, `HistoryServerReady`, `ApiServerReady`, `FlinkOperatorReady`), overall phase
- `CrucibleSparkJob` spec and status
  - Spec: jar, class, args, executor count/resources, driver resources, tenant (required), spark config overrides, labels
  - Status: phase (SUBMITTED, RUNNING, COMPLETED, FAILED, KILLED), driver pod name, executor pod names, ui_url, error (enriched), start/end timestamps
- `CrucibleFlinkJob` spec and status
  - Spec: jar, entry class, parallelism, task managers, tenant (required), flink config pass-through, labels
  - Status: phase (SUBMITTED, RUNNING, SUSPENDED, CANCELLED, FAILED), JobManager pod name, ui_url, error (enriched), last savepoint path, start/end timestamps
- Job state machine types and transition validation
- Unit tests: state machine transitions, invalid transition rejection, CRD serialization round-trip

### 1.2 Operator binary skeleton
- `crucible-operator` binary using `kube-rs` Controller for `CruciblePlatform`
- Top-level reconciler dispatches to sub-reconcilers, aggregates status conditions
- Health endpoint (`/healthz`) for readiness/liveness probes
- Helm template for operator Deployment, ServiceAccount, ClusterRole

### ~~1.2a Leader election~~ → moved to M6.7

### 1.2b Dependency ordering and failure isolation
- Dependency ordering logic: Celeborn + Volcano must be ready before platform reports ready for Spark; FlinkOperator must be ready before platform reports ready for Flink
- Failure isolation: sub-reconciler errors update their own condition, don't block other sub-reconcilers
- Unit tests: verify a failed Flink sub-reconciler does not affect Celeborn/Volcano/HistoryServer conditions

### 1.3 Celeborn sub-reconciler
- Reconcile Celeborn master Deployment (1 replica) and worker StatefulSet (configurable replicas)
- Worker pods: NVMe SSD volume mounts (or emptyDir for Kind), Celeborn config with replication factor 2
- Master pod: Celeborn master config, Service for worker registration
- Report `CelebornReady` condition based on master + minimum worker readiness
- Helm values: `celeborn.workers`, `celeborn.workerStorage`, `celeborn.image`
- Unit tests: generated pod specs, replication factor always 2, storage class selection
- Integration test: create CruciblePlatform, verify Celeborn StatefulSet deploys, workers reach Ready

### 1.4 Volcano sub-reconciler
- Install Volcano 1.10.x CRDs, controller, and scheduler from versioned manifest bundle (operator-managed, applied via kube-rs)
- Create default Volcano queues from platform spec (one per tenant defined)
- Report `VolcanoReady` condition based on Volcano scheduler pod readiness
- Unit tests: queue spec generation from tenant config
- Integration test: verify Volcano scheduler is running, queues are created

### 1.5 History Server sub-reconciler
- Deploy Spark History Server (single replica Deployment)
- Configure: RocksDB hybrid store backend, object store mount for event logs, ~2GB heap
- Service (ClusterIP) for API server to proxy to
- Report `HistoryServerReady` condition
- Helm values: `historyServer.enabled`, `historyServer.image`, `historyServer.resources`
- Integration test: verify History Server pod is running, health endpoint responds

### 1.6 Flink Operator sub-reconciler
- Install Apache Flink Kubernetes Operator 1.10.x from versioned manifest bundle (operator-managed, applied via kube-rs)
- Report `FlinkOperatorReady` condition
- Integration test: Flink Operator pod running, FlinkDeployment CRD available

### 1.7 Platform lifecycle integration tests
- Create platform → all sub-components deploy and report Ready within 120s
- Delete platform → all sub-components cleaned up, no orphans
- Update platform (e.g., Celeborn worker count) → operator reconciles, no downtime
- Sub-reconciler isolation: break Flink Operator config, verify Spark-side remains healthy

**Depends on:** Milestone 0
**Exit criteria:** `CruciblePlatform` CR deploys all sub-components (Celeborn, Volcano, History Server, Flink Operator) on Kind, status conditions are accurate, sub-reconciler isolation verified, platform delete is clean

---

## Milestone 2: Spark Job Controller

### 2.1 Spark job reconciler
- Controller watching `CrucibleSparkJob` CRs
- On create: generate driver pod spec with Celeborn shuffle manager injected, event log path, Volcano PodGroup, decommissioning config, spot tolerations on executors only
- Tenant field → Volcano queue mapping on PodGroup
- Track executor pods via owner references or label selectors
- Status updates: phase transitions, driver/executor pod names, timestamps
- On delete: terminate driver + executor pods, set KILLED status

### 2.2 Job state machine enforcement
- Validate transitions in-reconciler (no webhook — see Decisions)
- Handle edge cases: completion before executors register, driver crash on startup, double-delete
- Enriched error on failure: check Celeborn health, Volcano queue state, surface correlated platform errors

### 2.3 Celeborn integration for Spark
- Inject Celeborn client config into driver and executor pods (spark.shuffle.manager, celeborn.master.endpoints)
- Notify Celeborn master of application completion (REST API call) for shuffle data cleanup
- Unit tests: verify generated spark-defaults.conf contains correct Celeborn settings
- Integration test: submit SparkPi job, verify Celeborn shuffle manager is active (check driver logs for Celeborn registration)

### 2.4 Spark job integration tests
- Submit SparkPi → COMPLETED, event logs in MinIO
- Invalid JAR → FAILED with descriptive error
- Delete mid-execution → pods terminated, KILLED status
- Error correlation: scale Celeborn to 0, submit job, verify enriched error message

**Depends on:** Milestone 1
**Exit criteria:** Spark jobs run end-to-end on Kind with Celeborn shuffle, status reporting is accurate, error correlation works

---

## Milestone 3: API Server

### 3.1 REST API core
- `crucible-api` binary using axum on tokio
- CRUD endpoints: POST/GET/DELETE for jobs, list with filters (status, type, tenant)
- Request validation: required fields, tenant required, type discrimination
- Job submission: create `CrucibleSparkJob` or `CrucibleFlinkJob` CR via kube-rs client
- Job status: read from CR status, include ui_url
- Health endpoint (`/healthz`)
- Helm template: Deployment, Service (ClusterIP), Ingress

### 3.2 Log capture and streaming
- Watch CRs for pod status → open K8s log stream on driver/JM Running
- Ring buffer per job (configurable, default 50MB)
- Global buffer pool with 4GB cap, idle buffer eviction, reduced allocation under pressure
- `GET /logs` → serve from buffer (live) or object store (completed)
- `GET /logs?follow=true` → SSE from ring buffer, recent history on connect, fan-out to multiple clients
- Graceful shutdown: flush all buffers to object store on SIGTERM within terminationGracePeriodSeconds (60s)
- Unit tests: ring buffer bounds, eviction, concurrent read/write, global cap enforcement
- Integration tests: live log streaming during job, logs persist after completion, logs survive API server restart

### 3.3 Log persistence
- On job completion: PUT driver log to object store (`logs/{spark|flink}/{job-id}/driver.log`)
- On executor/TM pod completion: PUT per pod
- Flink periodic flush (configurable, default 10min) via multipart upload
- Stored log serving: byte-range reads, never load full file into memory
- Integration test: verify log files in MinIO match what was streamed live

### 3.4 UI reverse proxy
- `/api/v1/jobs/{id}/ui/*` → proxy to live Spark driver UI or Flink JM UI
- `/api/v1/spark-history/*` → proxy to History Server
- Automatic routing: job type + status determines target (live driver vs History Server for Spark)
- Spark UI path rewriting: `/jobs/` → `/api/v1/jobs/{id}/ui/jobs/` etc. in proxied HTML responses
- Integration tests: running Spark job UI proxied, completed job proxied via History Server, Spark UI link navigation works through proxy

### 3.5 Log buffer recovery after restart
- On API server startup, detect jobs that were RUNNING during the previous instance's lifetime
- Re-open K8s pod log streams for still-running drivers/JobManagers
- For completed jobs whose logs were not flushed (crash before graceful shutdown), replay from the K8s pod log API starting from the last flushed offset
- Integration test: kill API server mid-job, restart, verify log continuity (no gap between pre- and post-restart lines)

### 3.6 API integration tests
- Full API surface: POST returns 201, GET with filters, DELETE terminates, savepoint returns 400 for Spark
- Tenant filtering: only see your tenant's jobs
- Missing tenant → 400
- Invalid job ID → 404

**Depends on:** Milestone 1 for development of 3.1–3.5; Milestone 2 for integration tests (3.6) that need running Spark jobs
**Exit criteria:** Full REST API works, logs stream live and persist, log buffer recovery works across restarts, UI proxy serves Spark UI correctly, all integration tests pass on Kind

---

## Milestone 4: Flink Job Controller

### 4.1 Flink job reconciler
- Controller watching `CrucibleFlinkJob` CRs
- On create: template into `FlinkDeployment` CR with checkpoint path, parallelism, flinkConfig pass-through, tenant → Volcano queue
- Watch JobManager pods independently for log capture (separate from Flink Operator's watch)
- Status updates: phase transitions, JM pod name, last savepoint path
- Savepoint trigger: `POST /api/v1/jobs/{id}/savepoint` → trigger via Flink Operator
- Upgrade: `POST /api/v1/jobs/{id}/upgrade` → savepoint + redeploy
- On delete: graceful stop with savepoint (if configured), then terminate
- Unit tests: FlinkDeployment CR generation, flinkConfig pass-through, state machine
- Integration tests: submit streaming job → RUNNING, trigger savepoint → file in MinIO, delete → graceful stop

### 4.2 Flink UI proxy
- `/api/v1/jobs/{id}/ui/*` routes to Flink JM REST endpoint for Flink job types
- Minimal path rewriting (Flink SPA handles base paths)
- Integration test: Flink dashboard HTML served through proxy

### 4.3 Flink log capture
- JM log streaming via same ring buffer mechanism as Spark drivers
- Periodic flush for long-running jobs (10min default)
- TaskManager log capture: on TM pod completion, PUT log to object store (`logs/flink/{job-id}/taskmanager-{N}.log`)
- TM logs served via `GET /api/v1/jobs/{id}/logs?source=taskmanager&taskmanager=N`
- Integration test: Flink JM logs streamed live and persisted, TM logs available in MinIO after job completion

**Depends on:** Milestone 1 (Flink Operator sub-reconciler in 1.6), Milestone 3 (API server for proxy, log streaming, REST endpoints)
**Exit criteria:** Flink jobs run end-to-end on Kind, savepoints work, UI proxy works, logs captured

---

## Milestone 5: CLI

### 5.1 `crucible` CLI
- `crucible-cli` crate, builds to `crucible` binary
- `crucible submit --type spark|flink --jar ... --class ... --tenant ... [--executors N] [--async]` — `--async` prints job ID and exits immediately (default: blocks until terminal state)
- `crucible status <job-id>`
- `crucible logs <job-id> [--follow] [--source executor --executor N]`
- `crucible kill <job-id>`
- `crucible list [--status ...] [--type ...] [--tenant ...]`
- `crucible wait <job-id> [--phase RUNNING|COMPLETED|...] [--timeout 30m] [--all]` — block until job reaches target phase (default: any terminal state). `--all` waits for all jobs matching filters.
- `crucible timing <job-id> --metric submission-to-first-task|submission-to-running` — report timing metrics for a completed job
- `crucible savepoint <job-id>` (Flink only)
- `crucible upgrade <job-id>` (Flink only)
- Talks to API server via REST, endpoint configurable via `--api-url` or `CRUCIBLE_API_URL`
- Integration test: submit, status, logs, kill, wait via CLI against Kind cluster

**Depends on:** Milestone 3
**Exit criteria:** all CLI commands work against a running Crucible cluster

---

## Milestone 6: Polish & v0.1 Release

### 6.1 Image optimization
- DaemonSet for image pre-caching on worker nodes (base images built in 0.4)
- Measure and document cold vs warm start times
- Optimize image layer ordering for cache efficiency

### 6.2 Metrics and observability
- Prometheus metrics endpoint (`/metrics`) on both operator and API server
- Operator metrics: reconcile duration, reconcile errors, sub-reconciler status, leader election state
- API server metrics: request latency/count by endpoint, active log streams, ring buffer memory usage, global buffer pool utilization, object store PUT latency/errors
- Job metrics: jobs by phase, phase transition latency, error correlation hit rate
- Helm values: `metrics.enabled`, `metrics.port`, optional ServiceMonitor CR for Prometheus Operator
- Unit tests: metric registration, label cardinality bounds

### 6.3 Documentation
- README with quickstart (helm install, submit first job)
- Helm values reference
- API reference
- Architecture overview

### 6.4 CI hardening
- Full integration test suite in CI (Kind + MinIO)
- Container image builds and push to registry on tag
- Helm chart packaging and publish

### 6.5 End-to-end validation
- Run through the full Layer 2 integration test suite
- Manual smoke test on a real EKS cluster (if available)
- Verify: 5-minute time to first job on a running cluster

### 6.7 Leader election (pre-production blocker)
- Leader election via `kube-rs` lease-based mechanism (single active replica)
- Operator starts in standby mode until lease is acquired
- Graceful lease handoff on shutdown (release lease before exit)
- Integration test: run two operator replicas, verify only one is actively reconciling
- Moved from M1.2a — not needed for dev/test but required before production deployment

### 6.6 Cloud test infrastructure (v0.2 prep)
- Terraform/eksctl config for dedicated test EKS cluster (2 on-demand nodes, Karpenter for executor pool)
- `test/aws-values.yaml` for Helm install against EKS
- CI workflow stub (`.github/workflows/cloud-integration.yaml`) — nightly schedule, manual dispatch
- Layer 3 test scaffolding (spot resilience, Karpenter autoscaling, S3 persistence, multi-job contention)

**Depends on:** Milestones 0–5
**Exit criteria:** tagged v0.1 release, Helm chart published, README works end-to-end, metrics endpoint operational

---

## Milestone ordering and parallelism

```
M0 (scaffolding: workspace, helm, object store client, base images, test harness)
 └─► M1 (CRDs + operator + all sub-reconcilers incl. Flink Operator)
      ├─► M2 (Spark controller)
      │    └─► M3 integration tests (3.6) need running Spark jobs
      └─► M3 development (3.1–3.5: API core, log capture, persistence, UI proxy, recovery)
           ├─► M4 (Flink controller)  ← can partially overlap with M3
           ├─► M5 (CLI)               ← can start once M3 REST API is stable
           └─► M6 (polish, metrics, docs, cloud test prep)
```

M3 development (3.1–3.5) can begin after M1 — only M3 integration tests (3.6) require M2.
M4 and M5 can be worked in parallel once M3 is functional. M6 is the tail.
