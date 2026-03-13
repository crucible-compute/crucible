# Crucible

**Run Spark and Flink on Kubernetes. Spot-safe, gang-scheduled, with a REST API for everything.**

Crucible is an open-source compute platform that provides a single `helm install` experience for running Apache Spark batch jobs and Apache Flink streaming jobs on any Kubernetes cluster. It handles the hard parts — external shuffle for spot instance resilience, gang scheduling to prevent resource deadlocks, driver log aggregation, and elastic node scaling — so teams can focus on their workloads, not their infrastructure.

---

## Requirements

**Core goals:** Stable, cheap, highly scalable Spark and Flink execution on Kubernetes with a unified REST API for lifecycle management and log access.

**Non-negotiable:** Zero mandatory dependencies beyond Kubernetes and an object store (S3/GCS/ABFS). Five-minute time to first job on a running cluster. Works on EKS, GKE, and AKS.

**Runtime versions:** Spark 4.x, Flink 2.x, Scala 2.13, latest LTS Java (Java 21). These are the versions Crucible's base images ship and are tested against. User-provided JARs must be built for these versions.

**Spot instance support:** Spark executors run on spot/preemptible instances with zero shuffle data loss. Flink TaskManagers remain on on-demand instances for streaming stability. Drivers and JobManagers always on-demand.

**Gang scheduling:** Atomic placement of driver + minimum executor count (Spark) and JobManager + TaskManagers (Flink) to prevent resource deadlocks under concurrent job load.

**Log aggregation:** Driver and JobManager logs captured via the Kubernetes pod log API, buffered in-memory during execution, flushed as a single object to the configured bucket on job completion. Executor and TaskManager logs captured lazily on completion. Live streaming via SSE for in-flight jobs.

**Job start time optimization:** Pre-cached container images via DaemonSet, warm node pools with configurable minimums, and a slim Crucible-published Spark image with stripped dependencies. Target: under 30 seconds on warm nodes.

**UI access:** No custom UI. The API server acts as a reverse proxy to native UIs (Spark History Server, live Spark driver UI, Flink JobManager UI), providing a single external endpoint for all web access. Auth, routing, and path rewriting are handled centrally.

---

## Stack

### Crucible operator

A single Kubernetes operator written in Rust (using `kube-rs`) reconciling a `CruciblePlatform` CRD. One custom resource describes the desired platform state; the operator provisions and manages all sub-components. Installed via `helm install crucible`.

**Reconciliation model:** The operator uses sub-reconcilers per component (Celeborn, Volcano, Spark History Server, API server, Flink Operator). Each sub-reconciler reports an independent status condition on the `CruciblePlatform` CR (e.g., `CelebornReady`, `VolcanoReady`, `FlinkOperatorReady`). The top-level reconciler enforces dependency ordering: Celeborn and Volcano must be healthy before the Spark job controller accepts new jobs; the Flink Operator must be healthy before Flink jobs are accepted. A sub-reconciler failure is isolated — if the Flink Operator install fails, Spark jobs continue to run normally. The `CruciblePlatform` status reflects the degraded state (e.g., `FlinkOperatorReady: False`) without blocking unrelated components.

**Upgrades:** When the `CruciblePlatform` CR is updated (e.g., Celeborn version bump), the operator performs rolling updates per component. Running jobs are not drained — Celeborn's replication factor 2 ensures shuffle data survives a worker restart, and the StatefulSet's `RollingUpdate` strategy replaces one worker at a time. For breaking changes that require job drainage, the operator sets the platform to a `Draining` state and waits for running jobs to complete before proceeding.

### Spark (custom controller)

Crucible implements its own Spark job controller rather than wrapping the upstream Spark Operator. The `CrucibleSparkJob` CRD triggers driver pod creation with opinionated defaults: Celeborn shuffle manager injected, event log paths configured, Volcano PodGroup created, decommissioning enabled for spot. Executor pods are tracked for status and log capture. This gives Crucible full control over log streaming, shuffle configuration, and job lifecycle without coordination conflicts from a second operator. The CRD requires a `tenant` field that maps to a Volcano queue for resource isolation.

### Flink (managed dependency)

Crucible installs and manages the Apache Flink Kubernetes Operator. The `CrucibleFlinkJob` CRD templates into a `FlinkDeployment` CR with a `flinkConfig` pass-through for Flink-native settings. Crucible watches JobManager pods independently for log capture. Savepoint-based upgrades and checkpoint recovery are delegated to the Flink Operator's mature implementation. The CRD requires a `tenant` field that maps to a Volcano queue for resource isolation.

### Celeborn (external shuffle service)

Apache Celeborn deployed as a StatefulSet with local NVMe SSDs. Decouples shuffle data from executor pod lifetime, making spot interruptions safe. Executors push shuffle blocks to Celeborn workers; downstream stages read from Celeborn regardless of whether the original executor is still alive. Spark-only — Flink uses its own network-based data exchange.

**Replication factor: 2** (opinionated default, not configurable in v0.1). Each shuffle block is written to two Celeborn workers. This ensures that a single Celeborn worker failure (or the node it runs on being spot-reclaimed) does not lose shuffle data. The disk and network cost is acceptable — shuffle data is transient (lives only for the job duration) and NVMe SSDs are cheap relative to the cost of recomputing stages.

**Topology:** Master-worker architecture. A single Celeborn master (Deployment, 1 replica) manages worker registration, partition assignment, and shuffle block location tracking. Celeborn workers (StatefulSet) handle data storage. The master is lightweight and recovers quickly from restarts — workers re-register automatically.

**Disk cleanup:** Celeborn reclaims shuffle data for completed applications via its built-in application expiry mechanism. The Crucible Spark controller notifies Celeborn of application completion (via Celeborn's REST API) when a `CrucibleSparkJob` transitions to COMPLETED or FAILED, triggering immediate cleanup. A time-based fallback (default 1 hour) handles cases where the notification is missed.

**Worker failure during shuffle:** If a Celeborn worker dies mid-shuffle, in-flight pushes to that worker fail and Celeborn's client transparently retries to another worker (enabled by replication factor 2). Downstream reads fall back to the replica. No stage recomputation required unless both replicas are lost simultaneously.

### Volcano (gang scheduling)

Volcano scheduler installed as a managed dependency. Ensures that a job's minimum resource gang (driver + N executors, or JobManager + TaskManagers) is placed atomically. Prevents partial-start deadlocks under contention. Supports queue-based fair sharing across teams.

### Spark History Server

Single-replica deployment reading Spark event logs from object store. Configured with RocksDB hybrid store backend to keep memory footprint low (~2GB heap) while serving large job histories. Serves the full Spark UI retroactively for completed jobs.

### Node autoscaling

Karpenter (EKS), Node Auto-Provisioning (GKE), or Cluster Autoscaler (AKS) — configured externally in v0.1, documented per cloud. Three node pools: on-demand small instances for drivers/JobManagers, spot mixed instances for Spark executors, on-demand medium instances for Flink TaskManagers.

### Object store

Single customer-provided bucket (S3/GCS/ABFS) used for JARs, driver/executor logs, Spark event logs, and Flink checkpoints/savepoints. Lifecycle rules handle retention. Bucket layout:

```
s3://crucible-bucket/
├── jars/
├── logs/{spark|flink}/{job-id}/{driver|executor-N}.log
├── spark-events/
├── flink-checkpoints/{job-id}/
└── flink-savepoints/{job-id}/
```

---

## API Server

The API server is a separate binary from the operator, deployed as its own Deployment. Different scaling characteristics (operator is leader-elected singleton; API server can scale horizontally) and different failure modes (API server restart loses in-memory log buffers; operator restart just re-reconciles).

**Graceful shutdown:** On SIGTERM, the API server flushes all in-memory log buffers to object store before exiting. The pod's `terminationGracePeriodSeconds` is set high enough (default 60s) to allow flush completion. Buffers for completed jobs are flushed first (smallest, already serialized), followed by active job buffers (partial flush, resumable on next API server start via pod log replay from the last flushed offset).

**K8s API connection model:** Each running job's driver/JobManager log stream is a single long-lived HTTP/2 stream to the K8s API server via `kube-rs` (which multiplexes over a shared connection). At 200 concurrent jobs this is well within normal K8s API server capacity. Executor logs are not streamed live — they are captured on pod completion — so connection count scales with active driver/JobManager pods, not total pod count.

---

## REST API

All job types share a unified API surface. The CLI (`crucible submit`, `crucible logs`, `crucible kill`) wraps these endpoints.

```
POST   /api/v1/jobs                          Submit a job (type: spark|flink, tenant required)
GET    /api/v1/jobs                          List jobs (filterable by status, type, tenant, labels)
GET    /api/v1/jobs/{id}                     Job status, metadata, ui_url
GET    /api/v1/jobs/{id}/logs                Driver/JobManager logs (stored)
GET    /api/v1/jobs/{id}/logs?follow=true    Live log stream (SSE)
GET    /api/v1/jobs/{id}/logs?source=executor&executor=N   Executor logs
DELETE /api/v1/jobs/{id}                     Kill (Spark) or stop with savepoint (Flink)
POST   /api/v1/jobs/{id}/savepoint           Trigger savepoint (Flink only)
POST   /api/v1/jobs/{id}/upgrade             Savepoint + redeploy (Flink only)
```

Job status response includes `ui_url` linking to the appropriate native UI.

---

## UI Access

### The problem

Crucible manages three kinds of web UI, all running as ClusterIP services inside the cluster:

The **Spark History Server** is a single persistent service with a stable address, serving the full Spark UI for completed jobs. The **live Spark driver UI** is ephemeral — one per running Spark job, created when the driver starts, destroyed when the job finishes. The **Flink JobManager UI** is semi-persistent — one per streaming job, alive as long as the job runs.

The History Server can be exposed with a static Ingress. The other two are dynamic — services appear and disappear as jobs are submitted and complete. Pre-configured Ingress rules don't work for endpoints that don't exist yet.

### The solution: API server as reverse proxy

The Crucible API server acts as the single external gateway for all UI access. It already tracks every job and its associated pods. When a browser requests a job's UI, the API server looks up the target service and reverse-proxies the request internally.

```
/api/v1/jobs/{id}/ui/*          Proxy to live Spark driver UI or Flink JobManager UI
/api/v1/spark-history/*         Proxy to Spark History Server
```

For live Spark jobs, the API server proxies to the driver pod's Spark UI port. For completed Spark jobs, it proxies to the History Server at the appropriate application path. For Flink jobs, it proxies to the JobManager's REST endpoint which serves Flink's web UI. The UI type is resolved automatically based on job type and status — the client just requests `/api/v1/jobs/{id}/ui/` and gets the right thing.

### Path rewriting

The Spark UI hardcodes absolute paths (`/jobs/`, `/stages/`, `/executors/`) in its HTML responses, which break under any reverse proxy that adds a URL prefix. The API server rewrites these paths in proxied responses, translating `/jobs/` to `/api/v1/jobs/{id}/ui/jobs/` and so on. This is handled once in the proxy layer and works transparently for every job. The Flink UI is a modern SPA that handles base paths more gracefully and requires minimal rewriting.

### Single Ingress

The Helm chart creates one Ingress (or LoadBalancer Service) for the API server. All external traffic — REST API calls, log streaming, and UI access — flows through this single endpoint.

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: crucible-api
spec:
  rules:
  - host: crucible.example.com
    http:
      paths:
      - path: /api/
        pathType: Prefix
        backend:
          service:
            name: crucible-api
            port:
              number: 8080
```

No per-job Ingress objects, no dynamic Ingress controller reconfiguration, no direct exposure of internal services.

### Auth

The API server is the natural place for authentication. All internal UIs are ClusterIP services with no auth of their own — they are unreachable from outside the cluster. The API server gates access before proxying. One auth layer, one configuration point. For v0.1, no built-in auth — the Ingress is assumed to be internal to the user's network. Pluggable auth (API keys, OIDC) is planned for a future release.

### Fallback

`kubectl port-forward` remains available for direct access to any internal service. The reverse proxy is the production path; port-forwarding is the debugging escape hatch.

---

## Multi-Tenancy

### v0.1 — Volcano queues as the tenancy primitive

Each tenant maps to a Volcano queue with CPU/memory quotas. Both `CrucibleSparkJob` and `CrucibleFlinkJob` CRDs have a required `tenant` field. The job controller sets the Volcano queue accordingly. The API server uses the tenant field for filtering on `GET /api/v1/jobs?tenant=<name>`.

```yaml
apiVersion: scheduling.volcano.sh/v1beta1
kind: Queue
metadata:
  name: team-data-eng
spec:
  weight: 4
  capability:
    cpu: "200"
    memory: "800Gi"
```

No auth enforcement in v0.1 — tenant is trust-based — but the data model is in place for future enforcement.

### v0.2 — API-level isolation

Once the API server has auth (API keys or OIDC), each identity is bound to a tenant. The API server enforces: jobs can only be submitted to the caller's tenant queue, job visibility is scoped to the caller's tenant, and operations (kill, logs, savepoint) are restricted to owned jobs. This is a thin middleware layer on top of the existing tenant field.

### v0.3 — Namespace isolation (optional)

For strict environments (regulated industries, multi-business-unit clusters), namespace-per-tenant is supported. Each tenant's jobs run in their own namespace with ResourceQuotas and NetworkPolicies. Celeborn and Volcano remain cluster-scoped shared services.

---

## Error Correlation

**Design principle:** Crucible correlates platform-level failures with job-level failures in status reporting. Users should never have to grep through executor logs to discover that the shuffle service was down.

When a `CrucibleSparkJob` or `CrucibleFlinkJob` fails, the job controller checks the health of relevant platform components at the time of failure and enriches the job's status with platform context:

- **Celeborn unhealthy at time of job failure** → `"error": "Job failed: shuffle service unavailable (1/3 Celeborn workers ready)"` instead of the raw Spark exception.
- **Volcano scheduling timeout** → `"error": "Job pending: insufficient queue quota for tenant team-data-eng (requested 10 CPU, available 2 CPU)"` instead of generic pod scheduling failure.
- **Object store unreachable** → `"error": "Log upload failed: object store unreachable"` on the job status, with the job itself marked COMPLETED (logs are best-effort, not job-critical).
- **Celeborn worker restart during shuffle** → no job error if replication handles it transparently, but a status annotation noting `"warning": "Shuffle resilience event: Celeborn worker celeborn-2 restarted during execution, replicas served reads"`.

This correlation is heuristic — the controller checks component health and matches it against the failure timeline. It does not require Spark or Flink to propagate structured errors. The enriched error is set on the CRD status and returned by the API.

---

## Log Aggregation Detail

### Capture

The API server watches `CrucibleSparkJob` and `CrucibleFlinkJob` CRs for pod status changes. When a driver or JobManager pod enters the Running phase, the API server opens a streaming log connection via the Kubernetes pod log API (`/api/v1/namespaces/{ns}/pods/{pod}/log?follow=true`). Log lines are written to a bounded in-memory ring buffer (configurable, default 50MB per job). A global memory cap of 4GB limits total log buffer usage across all concurrent jobs. When the global cap is reached, the oldest idle buffers (completed jobs that have already flushed to object store) are evicted first; if all buffers are active, new jobs receive a reduced buffer allocation.

### Live streaming

The `GET /api/v1/jobs/{id}/logs?follow=true` endpoint serves from the ring buffer via Server-Sent Events. Recent history is sent first so the client isn't blank on connect, followed by new lines as they arrive. Multiple clients can connect simultaneously — all read from the same buffer with no additional Kubernetes API connections per viewer.

### Persistence

On job completion (or pod termination), the API server writes the buffered log content as a single PUT to object store at `logs/{spark|flink}/{job-id}/driver.log`. One file, one API call. Executor and TaskManager logs are captured the same way — one PUT per pod on job completion.

For long-running Flink jobs, the API server periodically flushes the JobManager log buffer to object store (configurable interval, default 10 minutes) using a multipart upload to keep the PUT count minimal.

### Serving stored logs

Once a job completes, `GET /api/v1/jobs/{id}/logs` reads from object store instead of memory. The switch is transparent to the client. Large log files are streamed back via byte-range reads — the API server never loads an entire log file into memory to serve it.

### Retention

Log retention is managed via object store lifecycle rules (S3 Lifecycle, GCS Object Lifecycle, ABFS Lifecycle Management) configured at the bucket level with a prefix filter on `logs/`. The API server returns a 404 with a descriptive message when logs have expired. Job metadata from the CRD remains available independently of log retention.

### Cost

S3 PUT pricing is $0.005 per 1,000 requests. A job with 100 executors produces 101 PUTs (one driver + 100 executors). At 1,000 jobs/day with 100 executors each, total PUT cost is approximately $0.50/day. Object store storage for logs is negligible at standard rates.
