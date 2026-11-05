FROM rust:1 AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --workspace
RUN rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli --version 0.2.130 --locked
RUN cargo build --release -p quarantinepay-browser --target wasm32-unknown-unknown && wasm-bindgen --target web --out-dir assets target/wasm32-unknown-unknown/release/quarantinepay_browser.wasm
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /src/target/release/quarantinepay-server /usr/local/bin/
COPY --from=builder /src/target/release/quarantinepay-verifier /usr/local/bin/
COPY --from=builder /src/assets ./assets
RUN mkdir .local && chown -R 65532:65532 /app
USER 65532:65532
ENV QP_BIND=0.0.0.0:3000 QP_VERIFIER_BIN=/usr/local/bin/quarantinepay-verifier QP_VERIFIER_MODE=trusted-process
EXPOSE 3000
CMD ["quarantinepay-server"]
