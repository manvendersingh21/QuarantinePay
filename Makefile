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

.PHONY: local-model-install local-model-serve local-model-pull
local-model-install:
	bash scripts/local-model.sh install
local-model-serve:
	bash scripts/local-model.sh serve
local-model-pull:
	bash scripts/local-model.sh pull

.PHONY: elastic-start elastic-stop elastic-status
elastic-start:
	bash scripts/elastic.sh start
elastic-stop:
	bash scripts/elastic.sh stop
elastic-status:
	bash scripts/elastic.sh status
