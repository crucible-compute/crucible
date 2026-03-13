# Notes — useful commands discovered during development

## Cargo

```bash
# Run all unit tests
cargo test --workspace

# Run integration tests (requires Kind cluster)
cargo test --package crucible-operator --test platform_lifecycle -- --ignored --test-threads=1

# Check formatting
cargo fmt --all --check

# Clippy
cargo clippy --workspace --all-targets
```

## Docker

```bash
# Build all images
just images

# Build only operator/api images
just images-crucible

# Build only base Spark/Flink images
just images-base
```

## Kind / Helm

```bash
# Stand up full test environment (Kind + MinIO + Helm install + load images)
just test-env

# Tear down
just teardown

# Load rebuilt images into Kind (without recreating cluster)
just load-images

# Upgrade Helm release after chart changes
helm upgrade crucible charts/crucible -f test/kind-values.yaml

# Restart operator after rebuilding image + loading into Kind
kubectl rollout restart deployment/crucible-operator

# Lint Helm chart
just helm-lint

# Render templates (dry-run)
just helm-template
```

## Debugging

```bash
# Operator logs (JSON in-cluster)
kubectl logs -l app.kubernetes.io/name=crucible-operator --tail=50

# Check platform status conditions
kubectl get crucibleplatform <name> -o json | python3 -m json.tool | grep -A 30 '"status"'

# List all operator-managed resources
kubectl get deploy,sts,svc -l app.kubernetes.io/managed-by=crucible-operator

# Check CRDs are installed
kubectl get crd | grep crucible
```

## Kube-rs 3.0 specifics

```bash
# rustls crypto provider must be installed before Client::try_default()
# In main.rs and integration tests:
rustls::crypto::ring::default_provider().install_default()
```
