.PHONY: build check test verifier-image
build:
	cargo build --workspace
	rustup target add wasm32-unknown-unknown
	cargo build -p quarantinepay-browser --target wasm32-unknown-unknown
	wasm-bindgen --target web --out-dir assets target/wasm32-unknown-unknown/debug/quarantinepay_browser.wasm
check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
test:
	cargo test --workspace
verifier-image:
	docker build -f Dockerfile.verifier -t quarantinepay-verifier:local .
