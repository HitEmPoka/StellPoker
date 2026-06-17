FROM rust:1.79 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY contracts/ ./contracts/
COPY stellar-zk-cards/ ./stellar-zk-cards/
COPY services/ ./services/
COPY vendor/ ./vendor/
RUN cargo build --release -p coordinator

# Build co-noir from source (rust:1.79 has all deps)
FROM rust:1.79 AS conoir-builder
RUN cargo install --git https://github.com/TaceoLabs/co-snarks --branch main co-noir

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/coordinator /usr/local/bin/coordinator
COPY --from=conoir-builder /usr/local/cargo/bin/co-noir /usr/local/bin/co-noir

# Circuit artifacts (compiled locally via ./scripts/compile-circuits.sh)
COPY circuits/deal_valid/target/ /app/circuits/deal_valid/target/
COPY circuits/reveal_board_valid/target/ /app/circuits/reveal_board_valid/target/
COPY circuits/showdown_valid/target/ /app/circuits/showdown_valid/target/

CMD ["coordinator"]
