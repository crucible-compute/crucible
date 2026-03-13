FROM rust:1.92-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin crucible-api

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=builder /app/target/release/crucible-api /crucible-api

EXPOSE 8080
ENTRYPOINT ["/crucible-api"]
