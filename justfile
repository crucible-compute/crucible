# Crucible development tasks
# Rust tasks use cargo directly; this file is for non-cargo orchestration.

default:
    @just --list

# Build all container images locally
images:
    docker build -f docker/operator.Dockerfile -t crucible-operator:latest .
    docker build -f docker/api.Dockerfile -t crucible-api:latest .

# Push images to GHCR
push-images tag="latest":
    docker tag crucible-operator:latest ghcr.io/crucible-compute/crucible-operator:{{tag}}
    docker tag crucible-api:latest ghcr.io/crucible-compute/crucible-api:{{tag}}
    docker push ghcr.io/crucible-compute/crucible-operator:{{tag}}
    docker push ghcr.io/crucible-compute/crucible-api:{{tag}}

# Load images into Kind cluster
load-images:
    kind load docker-image crucible-operator:latest --name crucible
    kind load docker-image crucible-api:latest --name crucible

# Create Kind cluster with test configuration
test-env:
    kind create cluster --name crucible --config test/kind-config.yaml
    @just load-images
    helm install crucible charts/crucible -f test/kind-values.yaml

# Destroy Kind cluster
teardown:
    kind delete cluster --name crucible

# Lint Helm chart
helm-lint:
    helm lint charts/crucible

# Render Helm templates (dry-run)
helm-template:
    helm template crucible charts/crucible
