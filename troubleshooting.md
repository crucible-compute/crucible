# Troubleshooting — problems encountered and fixes

## kube-rs 3.0 requires schemars v1

**Symptom:** Compile error — trait bound `JsonSchema` not satisfied, version mismatch.

**Cause:** kube 3.0 depends on schemars 1.x, not 0.8.x.

**Fix:** Change `schemars = "0.8"` to `schemars = "1"` in workspace `Cargo.toml`.

## kube-rs 3.0 requires MSRV 1.88

**Symptom:** Compile errors with Rust < 1.88.

**Cause:** kube 3.0.1 raised MSRV to 1.88 (from the originally planned 1.85).

**Fix:** Set `rust-version = "1.88"` in workspace `Cargo.toml`. Ensure `rustup` has 1.88+.

## rustls CryptoProvider panic

**Symptom:** `Could not automatically determine the process-level CryptoProvider from Rustls crate features.`

**Cause:** kube 3.0 uses rustls without a default crypto provider. Must be explicitly installed.

**Fix:** Add `rustls = "0.23"` to dependencies and call before any kube client usage:
```rust
rustls::crypto::ring::default_provider()
    .install_default()
    .expect("failed to install rustls crypto provider");
```
This applies to both the operator binary (`main.rs`) and integration test binaries.

## CRD 422 validation errors — snake_case vs camelCase

**Symptom:** `422 Unprocessable Entity` when operator applies resources. K8s rejects fields like `worker_storage` because CRD schema expects `workerStorage`.

**Cause:** Rust serde defaults to snake_case field names. K8s CRD schemas (generated from the same structs) expect camelCase.

**Fix:** Add `#[serde(rename_all = "camelCase")]` to all CRD spec/status/config structs in `crucible-types`.

## RBAC missing serviceaccounts permission

**Symptom:** Volcano and Flink sub-reconcilers fail silently. Platform status shows `"status": "False"` for VolcanoReady and FlinkOperatorReady with error messages like "reconciling Volcano scheduler" and "creating Flink Operator service account".

**Cause:** The operator's ClusterRole didn't include `serviceaccounts` in the core API group resources.

**Fix:** Add `serviceaccounts` to the RBAC rules in `charts/crucible/templates/rbac.yaml`.

## clap `env` feature required for `#[arg(env = "...")]`

**Symptom:** Compile error in crucible-cli — `env` attribute not recognized.

**Fix:** Add `env` to clap features: `clap = { version = "4", features = ["derive", "env"] }`.

## Spark 4.0 archive URL — no Scala suffix

**Symptom:** Docker build fails downloading Spark tarball.

**Cause:** Spark 4.x no longer ships scala-suffixed binaries.

**Fix:** Use `spark-4.0.0-bin-hadoop3.tgz` (not `spark-4.0.0-bin-hadoop3-scala2.13.tgz`).

## Flink 2.0 tarball still uses scala_2.12

**Symptom:** Docker build fails downloading Flink tarball with `scala_2.13`.

**Cause:** Flink 2.0 deprecated the Scala API but the release tarball filename still uses `scala_2.12`.

**Fix:** Set `SCALA_VERSION=2.12` in `docker/flink/Dockerfile`.

## Docker Hub 401 during image builds

**Symptom:** `401 Unauthorized` pulling base images.

**Cause:** Docker Hub login expired.

**Fix:** Re-run `docker login`.

## Spark driver pod ImagePullBackOff in Kind

**Symptom:** Driver pod stuck in `ImagePullBackOff`. Operator keeps job in `Submitted` phase.

**Cause:** Pod's `imagePullPolicy` defaults to `Always`, but Kind-loaded images aren't on a registry.

**Fix:** Set `IMAGE_PULL_POLICY=Never` as an env var on the operator pod. The Helm chart passes `image.operator.pullPolicy` as this env var. Ensure `test/kind-values.yaml` has `pullPolicy: Never`.

## Spark driver using local[*] instead of k8s://

**Symptom:** Spark jobs run single-process, no executor pods created.

**Cause:** Driver was configured with `--master local[*]` instead of Spark-on-K8s mode.

**Fix:** Use `--master k8s://https://kubernetes.default.svc` and ensure the driver pod has a ServiceAccount with RBAC to create/watch/delete pods and services. The operator now creates per-job SA + Role + RoleBinding automatically.
