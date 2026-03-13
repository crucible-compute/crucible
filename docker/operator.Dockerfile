FROM rust:1.92-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin crucible-operator

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=builder /app/target/release/crucible-operator /crucible-operator

ENTRYPOINT ["/crucible-operator"]
