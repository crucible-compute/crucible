# Crucible development tasks
# Rust tasks use cargo directly; this file is for non-cargo orchestration.

set dotenv-load

# Version defaults
spark_version := "4.0.0"
flink_version := "2.0.0"
crucible_version := "0.1.0"

default:
    @just --list

# --- Container images ---

# Build all container images locally
images: images-base images-crucible

# Build base Spark and Flink images
images-base:
    docker build -f docker/spark/Dockerfile \
        --build-arg SPARK_VERSION={{spark_version}} \
        -t crucible-spark:{{spark_version}} \
        -t crucible-spark:latest \
        docker/spark
    docker build -f docker/flink/Dockerfile \
        --build-arg FLINK_VERSION={{flink_version}} \
        -t crucible-flink:{{flink_version}} \
        -t crucible-flink:latest \
        docker/flink

# Build Crucible operator and API server images
images-crucible:
    docker build -f docker/operator.Dockerfile -t crucible-operator:latest .
    docker build -f docker/api.Dockerfile -t crucible-api:latest .

# Push images to GHCR
push-images tag=crucible_version:
    docker tag crucible-operator:latest ghcr.io/crucible-compute/crucible-operator:{{tag}}
    docker tag crucible-api:latest ghcr.io/crucible-compute/crucible-api:{{tag}}
    docker tag crucible-spark:latest ghcr.io/crucible-compute/crucible-spark:{{tag}}
    docker tag crucible-flink:latest ghcr.io/crucible-compute/crucible-flink:{{tag}}
    docker push ghcr.io/crucible-compute/crucible-operator:{{tag}}
    docker push ghcr.io/crucible-compute/crucible-api:{{tag}}
    docker push ghcr.io/crucible-compute/crucible-spark:{{tag}}
    docker push ghcr.io/crucible-compute/crucible-flink:{{tag}}

# Load all images into Kind cluster
load-images:
    kind load docker-image crucible-operator:latest --name crucible
    kind load docker-image crucible-api:latest --name crucible
    kind load docker-image crucible-spark:latest --name crucible
    kind load docker-image crucible-flink:latest --name crucible

# --- Test environment ---

# Create Kind cluster with test configuration
test-env:
    kind create cluster --name crucible --config test/kind-config.yaml
    kubectl apply -f test/fixtures/minio.yaml
    kubectl wait --for=condition=available deployment/minio --timeout=60s
    @just load-images
    helm install crucible charts/crucible -f test/kind-values.yaml

# Destroy Kind cluster
teardown:
    kind delete cluster --name crucible

# --- Tests ---

# Run unit tests
test:
    cargo test --workspace

# Run integration tests (requires running Kind cluster)
# Starts operator locally, runs tests, then stops operator.
test-integration:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Starting operator in background..."
    RUST_LOG=info cargo run --bin crucible-operator &
    OPERATOR_PID=$!
    sleep 3
    echo "Running integration tests..."
    cargo test --package crucible-operator --test platform_lifecycle -- --ignored --test-threads=1 || true
    echo "Stopping operator (PID $OPERATOR_PID)..."
    kill $OPERATOR_PID 2>/dev/null || true
    wait $OPERATOR_PID 2>/dev/null || true

# --- Helm ---

# Lint Helm chart
helm-lint:
    helm lint charts/crucible

# Render Helm templates (dry-run)
helm-template:
    helm template crucible charts/crucible
