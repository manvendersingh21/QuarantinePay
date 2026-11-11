# Private runtime inference

QuarantinePay defaults to local inference with Qwen3-30B-A3B Instruct (4-bit) through Ollama. The validated host has a Tesla V100 32 GB; Ollama reports the model running entirely on GPU with an 8,192-token context. The Rust application keeps model access behind a loopback-only client.

## Data destinations

| Data | Private-mode destination |
| --- | --- |
| Assistant prompts and responses | Local Qwen model on the host GPU |
| Product search queries | Local PostgreSQL mirror; local demo catalog if no import exists |
| Listing text used for injection screening | Local Qwen classifier; malformed classifier responses withhold results |
| Delivery artifacts and deterministic acceptance results | Local verifier and PostgreSQL |
| Optional Elastic semantic screening | Listing sentences embedded by the local Ollama model and compared in a loopback Elasticsearch only; remote endpoints rejected, no proxies or redirects |
| Approved payment amount, currency, recipient, intent reference, and API authentication | PayPal Sandbox |

A precise demo claim is: **“AI reasoning and delivery verification stay on our server. PayPal handles the approved payment.”** Payment integration requires sending approved payment fields and credentials to PayPal. Do not claim that every byte stays on the host.

## Enforced defaults

- `QP_PRIVATE_INFERENCE=true` and `QP_INFERENCE_PROVIDER=local`.
- Local model URLs require a loopback IP literal. Userinfo, query strings, non-root paths, and external hostnames are rejected.
- The local HTTP client disables proxies and redirects. It never falls back to Muse or another remote provider after an error.
- Cloud model tags are rejected. The managed Ollama launch sets `OLLAMA_NO_CLOUD=1` and binds to `127.0.0.1`.
- Private mode never sends user searches to Channel3. An authenticated operator may explicitly import public listings using only the configured `QP_CATALOG_SEED_QUERY`; Channel3 receives that seed and the API key. Listings are screened before local storage, and private user searches use only the local PostgreSQL mirror. Optional Elasticsearch must also use a loopback endpoint and a client that refuses redirects/proxies.
- No payment credentials or signing keys are included in model prompts. Recommendations and classifier output do not authorize payment effects.

Public model weights and runtime packages require an initial download. Once downloaded, private model inference needs no cloud model service. `make local-model-serve` starts the runtime; `make local-model-pull` downloads the selected model. `make local-model-install` installs pinned Ollama 0.40.14 under the ignored `.local` directory. Model weights, runtime binaries, credentials, and local artifacts are excluded from Git.

To import a public catalog, configure `CHANNEL3_API_KEY` and optionally `QP_CATALOG_SEED_QUERY`, then use the authenticated **Import public Channel3 catalog** action. This explicit import sends the fixed seed but not user searches; imported listings are discovery-only. To additionally use Muse for inference, set `QP_PRIVATE_INFERENCE=false`, `QP_INFERENCE_PROVIDER=muse`, and provide its API credentials. Neither option is an automatic fallback.

Agy and Claude were used as development reviewers at the user's request. They received a source snapshot excluding `.env` and `.local`; they are not part of the private runtime. This local inference setting does not control the host's other applications, browser extensions, or development tools.

Local classifier output is supporting detection, not a guarantee that every prompt injection is caught. The signed purchase intent and deterministic gateway checks remain the payment boundary.
