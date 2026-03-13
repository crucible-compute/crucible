# Crucible — Testing Strategy

Four layers, each catching different classes of bugs. Layers 1 and 2 are required for v0.1. Layers 3 and 4 are added as the project matures.

---

## Layer 1: Unit Tests

**Scope:** Pure logic with no external dependencies. No Kubernetes API, no object store, no network.

**What to test:**

- Pod spec generation — given a `CrucibleSparkJob` CR with specific config, verify the operator produces the correct driver pod spec: Celeborn shuffle manager injected, event log path set, Volcano PodGroup annotations present, spot tolerations on executors but not drivers, decommissioning config enabled.
- Flink CR templating — given a `CrucibleFlinkJob` CR, verify the resulting `FlinkDeployment` CR has the correct checkpoint path, parallelism, and `flinkConfig` pass-through values.
- Log buffer — ring buffer bounds, eviction under memory pressure, concurrent reader/writer safety, correct flush-to-bytes serialization.
- API request validation — malformed job submissions rejected, required fields enforced (including `tenant`), Spark vs Flink type discrimination. Jobs submitted without a tenant are rejected.
- Job state machine — verify transitions (SUBMITTED → RUNNING → COMPLETED/FAILED for Spark, SUBMITTED → RUNNING → SUSPENDED/CANCELLED for Flink), reject invalid transitions, handle edge cases (completion before any executor registers, double-delete).
- Tenant-to-queue mapping — given a `tenant` field value, verify the correct Volcano queue name is produced.
- Spark UI path rewriting — given proxied HTML containing `/jobs/`, `/stages/`, `/executors/`, verify rewrite to `/api/v1/jobs/{id}/ui/jobs/` etc.
- Object store path construction — verify bucket layout matches spec for all job types, log sources, and checkpoint/savepoint paths.
- Log buffer global cap — verify that when total buffer allocation reaches 4GB, the oldest idle (already-flushed) buffers are evicted, and new jobs receive reduced allocations rather than OOMing the API server.
- Error correlation — given a job failure event and a mock platform health snapshot (e.g., Celeborn 1/3 workers ready), verify the enriched error message includes "shuffle service unavailable" rather than the raw Spark exception. Same for Volcano scheduling timeouts and object store failures.
- Sub-reconciler independence — verify that a Flink Operator sub-reconciler failure does not block the Celeborn or Spark sub-reconciler status conditions.

**How to run:**

```bash
cargo test --lib
```

Mock the K8s client interface and object store client. No containers, no network.

**CI:** Every PR. Must pass in under 30 seconds.

---

## Layer 2: Integration Tests (Kind + MinIO)

**Scope:** Real Kubernetes API server, real pod lifecycle, S3-compatible object store. No cloud provider, no real cost.

**Infrastructure:**

```bash
# Multi-node Kind cluster
cat <<EOF > /tmp/kind-config.yaml
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
- role: control-plane
- role: worker
- role: worker
- role: worker
EOF
kind create cluster --config /tmp/kind-config.yaml

# MinIO for S3-compatible object store
kubectl apply -f test/fixtures/minio.yaml
# Creates: minio Service on port 9000, default bucket "crucible-test"

# Install Crucible with test config
helm install crucible ./charts/crucible \
  --set objectStore.endpoint=http://minio.default.svc:9000 \
  --set objectStore.bucket=crucible-test \
  --set objectStore.accessKey=minioadmin \
  --set objectStore.secretKey=minioadmin \
  --set celeborn.workers=1 \
  --set historyServer.enabled=true \
  --set api.replicas=1
```

**Test cases:**

### Platform lifecycle

- Create a `CruciblePlatform` CR, verify the operator deploys Celeborn StatefulSet (correct replica count, storage class), Volcano controller and scheduler, Spark History Server (RocksDB config, object store mount), and the API server. All pods reach Ready within 120 seconds.
- Delete the `CruciblePlatform` CR, verify all sub-components are cleaned up. No orphaned pods, services, or PVCs.
- Update the `CruciblePlatform` CR (e.g. change Celeborn worker count from 1 to 2), verify the operator reconciles without downtime.

### Spark job lifecycle

- Submit a `CrucibleSparkJob` running SparkPi, verify driver pod created with correct labels and Celeborn config, executors launched, job status transitions to COMPLETED, event logs written to MinIO at `spark-events/` prefix.
- Submit a job with an invalid JAR path, verify status transitions to FAILED with a descriptive error message.
- Submit a job and delete it mid-execution, verify driver and executor pods are terminated, status transitions to KILLED.

### Flink job lifecycle

- Submit a `CrucibleFlinkJob` running a trivial streaming job (e.g. socket word count), verify JobManager and TaskManagers created, job enters RUNNING state.
- Trigger a savepoint via `POST /api/v1/jobs/{id}/savepoint`, verify savepoint file appears in MinIO at the expected path.
- Delete a Flink job, verify graceful stop with savepoint (if configured), pods terminated.

### Log capture

- Submit a Spark job, poll `GET /api/v1/jobs/{id}/logs` during execution, verify driver log lines appear (should contain Spark initialization messages).
- Open `GET /api/v1/jobs/{id}/logs?follow=true` via SSE, verify live lines stream in while the job runs.
- After job completion, verify logs are persisted in MinIO at `logs/spark/{job-id}/driver.log`.
- Verify `GET /api/v1/jobs/{id}/logs` reads from MinIO (not memory) after completion — restart the API server pod and verify logs are still served.

### UI proxy

- For a running Spark job, `GET /api/v1/jobs/{id}/ui/` returns HTML containing Spark UI content (check for known strings like "Spark Jobs", "Stages").
- For a completed Spark job, same endpoint proxies to History Server — verify HTML is served and contains the completed application's data.
- For a running Flink job, same endpoint proxies to the Flink dashboard — verify HTML contains Flink UI content.
- Verify Spark UI path rewriting works — follow a link from the proxied page (e.g. click into a stage), verify it resolves correctly through the proxy.

### Gang scheduling

- Configure Volcano with a single queue limited to 4 CPUs total.
- Submit two Spark jobs each requesting a driver (0.5 CPU) + 2 executors (1 CPU each) = 2.5 CPU per job.
- Verify both jobs are scheduled (total 5 CPU exceeds limit, but gang minimum should be tuned to fit).
- Submit a third job, verify it stays in PENDING state until one of the first two completes.
- Verify no deadlock — the pending job eventually runs.

### Platform sub-reconciler isolation

- Create a `CruciblePlatform` CR with Flink disabled. Verify Celeborn, Volcano, History Server, and API server all deploy and report Ready conditions. `FlinkOperatorReady` condition should be absent or N/A.
- Introduce a broken Flink Operator config (e.g., invalid image), verify `FlinkOperatorReady: False` on the platform status while all other conditions remain `True`. Submit a Spark job, verify it runs successfully despite the Flink failure.

### Error correlation

- Scale Celeborn workers to 0 replicas. Submit a Spark job, wait for it to fail. Verify the job status includes an enriched error referencing shuffle service unavailability, not just the raw Spark stage failure.
- Configure a Volcano queue with 0 CPU capacity. Submit a Spark job targeting that queue. Verify the job status includes "insufficient queue quota" rather than a generic scheduling timeout.

### Celeborn replication

- Submit a shuffle-heavy Spark job (e.g., word count with repartition). While the job is running, delete one Celeborn worker pod. Verify the job completes successfully — Celeborn replication factor 2 ensures the surviving replica serves reads.
- After the killed worker pod restarts, verify it re-registers with the Celeborn master and the StatefulSet returns to full replica count.

### API server graceful shutdown

- Submit a Spark job, wait for it to start producing logs. Kill the API server pod (kubectl delete). Wait for the replacement pod to start. Verify that `GET /api/v1/jobs/{id}/logs` still returns the pre-restart log lines (flushed to object store during shutdown or re-captured from the K8s pod log API).

### Tenant isolation

- Submit a Spark job with `tenant: team-alpha`, verify the resulting PodGroup targets the `team-alpha` Volcano queue.
- Submit jobs for two different tenants, verify `GET /api/v1/jobs?tenant=team-alpha` returns only team-alpha's jobs.
- Submit a job without a `tenant` field, verify 400 rejection.

### API surface

- `POST /api/v1/jobs` with Spark type and tenant returns 201 with job ID.
- `POST /api/v1/jobs` without tenant returns 400.
- `GET /api/v1/jobs` returns list, filterable by `?status=running`, `?type=spark`, and `?tenant=<name>`.
- `GET /api/v1/jobs/{id}` returns status including `ui_url`.
- `DELETE /api/v1/jobs/{id}` returns 200 and job is terminated.
- `POST /api/v1/jobs/{id}/savepoint` returns 400 for Spark jobs (Flink only).
- Invalid job ID returns 404.

**How to run:**

```bash
# Full suite
cargo test --test integration -- --test-threads=1

# Single test
cargo test --test integration -- spark_job_lifecycle
```

**CI:** Every PR. Kind cluster setup adds ~30 seconds. Full suite should complete in 5-10 minutes. Requires a CI runner with Docker (GitHub Actions large runner or self-hosted).

```yaml
# .github/workflows/integration.yaml
on: [pull_request, push]

jobs:
  integration:
    runs-on: ubuntu-latest-8core
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: helm/kind-action@v1
        with:
          config: test/kind-config.yaml
      - name: Install Crucible
        run: |
          kubectl apply -f test/fixtures/minio.yaml
          kubectl wait --for=condition=Ready pod -l app=minio --timeout=60s
          helm install crucible ./charts/crucible -f test/kind-values.yaml
          kubectl wait --for=condition=Ready pod -l app=crucible-api --timeout=120s
      - name: Run integration tests
        run: cargo test --test integration -- --test-threads=1
      - name: Collect logs on failure
        if: failure()
        run: |
          kubectl logs -l app=crucible-operator --tail=200
          kubectl logs -l app=crucible-api --tail=200
          kubectl get cruciblesparkjobs,crucibleflinkjobs -o yaml
```

---

## Layer 3: Cloud Integration Tests (EKS + Real S3)

**Scope:** Real cloud behavior — spot instances, Karpenter node provisioning, S3 persistence, actual image pull latencies. Tests things that cannot be faked locally.

**Infrastructure:** A dedicated test EKS cluster in a test AWS account. Minimal footprint: 2 on-demand nodes for platform components, everything else scales to zero via Karpenter when idle. Approximate idle cost: $3-5/day.

```bash
# Cluster assumed to exist (provisioned via Terraform/eksctl, not per-run)
aws eks update-kubeconfig --name crucible-test --region us-west-2

# Install/upgrade Crucible with AWS-specific config
helm upgrade --install crucible ./charts/crucible -f test/aws-values.yaml
```

**Test cases:**

### Spot resilience

Submit a Spark job with 10 spot executors running a shuffle-heavy workload (e.g. TPC-DS query). Mid-job, terminate a spot node via the AWS CLI. Verify Celeborn preserves shuffle data and the job completes without full stage recomputation.

```bash
NODE=$(kubectl get pods -l spark-role=executor \
  -o jsonpath='{.items[0].spec.nodeName}')
INSTANCE=$(aws ec2 describe-instances \
  --filters "Name=private-dns-name,Values=$NODE" \
  --query 'Reservations[0].Instances[0].InstanceId' --output text)

aws ec2 terminate-instances --instance-ids $INSTANCE

# Wait for job completion — should succeed despite node loss
crucible wait $JOB_ID --timeout 30m
crucible status $JOB_ID  # Expect: COMPLETED
```

### Karpenter autoscaling

Starting from an idle cluster (executor pool scaled to zero nodes), submit a Spark job requesting 20 executors. Measure wall-clock time from submission to first task execution. Verify Karpenter provisions nodes from the correct pool (spot, correct instance families). After job completion, verify nodes are reclaimed within the configured TTL.

```bash
# Record node count before
NODES_BEFORE=$(kubectl get nodes -l karpenter.sh/nodepool=spark-executors --no-headers | wc -l)

crucible submit --type spark --jar s3://crucible-test/jars/spark-pi.jar \
  --class org.apache.spark.examples.SparkPi --executors 20

# Wait for running
crucible wait $JOB_ID --phase RUNNING --timeout 10m

# Verify new nodes provisioned
NODES_AFTER=$(kubectl get nodes -l karpenter.sh/nodepool=spark-executors --no-headers | wc -l)
[ $NODES_AFTER -gt $NODES_BEFORE ] || fail "No new nodes provisioned"

# Wait for completion
crucible wait $JOB_ID --timeout 30m

# Wait for scale-down (Karpenter ttlSecondsAfterEmpty)
sleep 120
NODES_FINAL=$(kubectl get nodes -l karpenter.sh/nodepool=spark-executors --no-headers | wc -l)
[ $NODES_FINAL -le $NODES_BEFORE ] || fail "Nodes not reclaimed"
```

### Image pull performance

Measure cold start (fresh node, no image cache) vs warm start (image cached via DaemonSet). This is a benchmark tracked over time, not a pass/fail gate.

```bash
# Cold: taint an existing node to force Karpenter to provision a new one
crucible submit --type spark --jar s3://crucible-test/jars/spark-pi.jar \
  --class org.apache.spark.examples.SparkPi --executors 2
COLD_START=$(crucible timing $JOB_ID --metric submission-to-first-task)

# Warm: run again on same nodes (image cached)
crucible submit --type spark --jar s3://crucible-test/jars/spark-pi.jar \
  --class org.apache.spark.examples.SparkPi --executors 2
WARM_START=$(crucible timing $JOB_ID --metric submission-to-first-task)

echo "Cold: ${COLD_START}s, Warm: ${WARM_START}s"
```

### S3 log persistence

Submit a Spark job, verify logs land in S3 at the expected path, verify content matches what was served via the API during execution.

```bash
crucible submit --type spark --jar s3://crucible-test/jars/spark-pi.jar \
  --class org.apache.spark.examples.SparkPi
crucible wait $JOB_ID

# Verify log file exists in S3
aws s3 ls s3://crucible-test/logs/spark/$JOB_ID/driver.log || fail "Log not found in S3"

# Verify event log exists
aws s3 ls s3://crucible-test/spark-events/ | grep $JOB_ID || fail "Event log not found"

# Verify History Server can serve the completed job
curl -sf "http://crucible.test/api/v1/spark-history/history/$JOB_ID/jobs/" || fail "History Server cannot serve job"
```

### Multi-job contention

Submit 10 Spark jobs simultaneously, each requesting 5 executors. Verify Volcano queues them according to gang scheduling constraints, no deadlocks occur, and all 10 eventually complete.

```bash
for i in $(seq 1 10); do
  crucible submit --type spark --jar s3://crucible-test/jars/spark-pi.jar \
    --class org.apache.spark.examples.SparkPi --executors 5 --async
done

# Wait for all to complete (generous timeout)
crucible wait --all --timeout 60m

# Verify all completed
COMPLETED=$(crucible list --status completed | wc -l)
[ $COMPLETED -eq 10 ] || fail "Only $COMPLETED/10 jobs completed"
```

**When to run:** Nightly on main branch and manually before releases. Not on every PR — too slow and too expensive.

```yaml
# .github/workflows/cloud-integration.yaml
on:
  schedule:
    - cron: '0 4 * * *'
  workflow_dispatch: {}

jobs:
  cloud-tests:
    runs-on: [self-hosted, aws]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Configure kubectl
        run: aws eks update-kubeconfig --name crucible-test --region us-west-2
      - name: Install Crucible
        run: helm upgrade --install crucible ./charts/crucible -f test/aws-values.yaml
      - name: Run cloud integration tests
        run: cargo test --test cloud -- --test-threads=1 --timeout 5400
      - name: Cleanup
        if: always()
        run: kubectl delete cruciblesparkjobs,crucibleflinkjobs --all --timeout=5m
```

---

## Layer 4: Chaos and Soak Tests

**Scope:** Reliability under sustained load and unexpected failures. Catches memory leaks, resource exhaustion, recovery bugs, and edge cases that only appear over time.

**When to run:** Before any release tagged as stable. Run manually with monitoring.

### Soak test

Submit a continuous stream of Spark jobs (one every 30 seconds) for 24 hours. Monitor for:

- Memory growth in the operator and API server pods (log buffers not freed, K8s watch connections accumulating). Verify the API server's total log buffer usage stays within the 4GB global cap.
- Celeborn disk usage growing without bound (shuffle data not cleaned up after jobs complete).
- History Server RocksDB storage growing without bound.
- Node count creeping up (Karpenter not reclaiming nodes after jobs finish).
- File descriptor leaks (K8s API log stream connections not closed).

```bash
# Simple soak driver
for i in $(seq 1 2880); do  # 30s × 2880 = 24h
  crucible submit --type spark --jar s3://crucible-test/jars/spark-pi.jar \
    --class org.apache.spark.examples.SparkPi --executors 5 --async
  sleep 30
done
```

Monitor via `kubectl top pods` and Prometheus metrics (if available). Record resource usage at hourly intervals. Flag any monotonically increasing metric.

### Chaos test

Run the soak test while randomly killing components. The system should recover from every failure without manual intervention.

```bash
# Chaos loop — runs alongside the soak test
while true; do
  SLEEP=$((RANDOM % 300 + 60))  # 1-6 minutes between events
  sleep $SLEEP

  ACTION=$((RANDOM % 5))
  case $ACTION in
    0)
      echo "Killing a Celeborn worker"
      kubectl delete pod -l app=crucible-celeborn --field-selector=status.phase=Running \
        | head -1
      ;;
    1)
      echo "Killing the API server"
      kubectl delete pod -l app=crucible-api --field-selector=status.phase=Running \
        | head -1
      ;;
    2)
      echo "Killing the operator"
      kubectl delete pod -l app=crucible-operator
      ;;
    3)
      echo "Draining a worker node"
      NODE=$(kubectl get nodes -l karpenter.sh/nodepool=spark-executors \
        --no-headers -o name | shuf | head -1)
      kubectl drain $NODE --ignore-daemonsets --delete-emptydir-data --timeout=60s
      sleep 30
      kubectl uncordon $NODE
      ;;
    4)
      echo "Killing a random executor pod"
      kubectl delete pod -l spark-role=executor --field-selector=status.phase=Running \
        | head -1
      ;;
  esac
done
```

**Verify after 24 hours:**

- All soak-submitted jobs eventually reached COMPLETED or FAILED (no stuck RUNNING/PENDING jobs).
- Celeborn has correct worker count (recovered from kills). Disk usage is stable — shuffle data for completed jobs has been cleaned up.
- Celeborn replication held — no jobs failed due to shuffle data loss despite worker kills.
- API server is responsive and serving logs. Memory usage is within the 4GB global buffer cap.
- Operator is running and reconciling. Sub-reconciler status conditions are all True.
- No zombie nodes lingering in the cluster.
- Object store has logs for all completed jobs.
- Error correlation worked for chaos-induced failures — jobs that failed during Celeborn worker kills have enriched error messages in their status.

---

## Summary

| Layer | Catches | Runs on | Trigger | Duration | Required for |
|---|---|---|---|---|---|
| Unit | Logic bugs, spec generation, state machine | Any machine | Every PR | <30s | v0.1 |
| Integration (Kind + MinIO) | K8s lifecycle, log capture, UI proxy, gang scheduling | CI runner with Docker | Every PR | 5-10 min | v0.1 |
| Cloud (EKS + S3) | Spot behavior, autoscaling, S3 persistence, multi-job contention | Test EKS cluster | Nightly + pre-release | 30-60 min | v0.2 |
| Chaos / Soak | Memory leaks, recovery, resource exhaustion | Test EKS cluster | Pre-release | 24h | First stable release |
