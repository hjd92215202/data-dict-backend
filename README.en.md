# Data Dictionary Backend (data-dict-backend)

This repository implements the backend for a Data Standard Management System. It's built with Rust and Axum, providing features such as standard field management, word-root (term) management, tokenization suggestions, and vector-based semantic search using Qdrant as the vector store and fastembed for embeddings.

## Quick Start
- Set environment variables and run:

```bash
export DATABASE_URL=postgres://user:pass@localhost:5432/dbname
export QDRANT_URL=http://localhost:6334
export RUST_LOG=info
cargo run --release
```

On Windows use `set`/`setx` or configure variables in your IDE.

## Project Layout (high level)
- Entry: [src/main.rs](src/main.rs#L1)
- HTTP Handlers (routes): [src/handlers](src/handlers/mod.rs#L1)
- Business logic (service layer): [src/services](src/services/mod.rs#L1)
- Data models: [src/models](src/models/mod.rs#L1) (examples: [src/models/field.rs](src/models/field.rs#L1), [src/models/user.rs](src/models/user.rs#L1))
- Middleware: [src/middleware/auth.rs](src/middleware/auth.rs#L1)
- Qdrant configuration: [qdrant/config.yaml](qdrant/config.yaml#L1)
- Offline embedding cache: `model/fastembed_cache`

## Architecture Overview
- `axum` provides HTTP routing; `tokio` provides async runtime.
- Layering: Handler (HTTP) → Service (business logic) → Models (DB entities).
- Persistence: Postgres via `sqlx`. Vector store: Qdrant via `qdrant-client`. Embeddings: `fastembed` offline model.
- Tokenization uses `jieba-rs`. At startup, the system loads standard word roots and synonyms into Jieba to improve matching.

## Core Modules & Key Logic

1) Startup & Initialization (`src/main.rs`)
- Initialize logging and environment, create `PgPool`.
- Load offline embedding model (`fastembed::TextEmbedding`) into shared `AppState`.
- Ensure Qdrant collections (`word_roots`, `standard_fields`) exist and perform cold-start sync:
  - `sync_roots_to_qdrant()` — embed and upload word-root vectors.
  - `sync_fields_to_qdrant()` — embed and upload standard-field vectors.

2) Routing & Authorization (`src/main.rs` / `src/middleware/auth.rs`)
- Routes are namespaced as `/api/auth`, `/api/public`, `/api/admin`. Admin routes are protected by the `guard` middleware which validates JWT and requires role `admin`.

3) Tokenization Suggestions (Handlers → Services)
- Endpoint: `/api/admin/suggest` implemented in `src/handlers/mapping_handler.rs`.
- Service: `mapping_service::suggest_field_name(pool, input)`:
  - First tries exact/full-term or synonym match (SQL ILIKE).
  - If no match, tokenizes input with Jieba (accurate mode) and searches each token in the standard word-root table, returning `Segment` list.

4) Semantic Similarity Search (Handlers → Qdrant)
- Endpoint: `/api/public/similar-roots`.
- Flow: embed input via shared model (`state.embed_model.lock()`), call Qdrant `search_points` on `word_roots`, return payload and scores.

5) Users & Auth (`src/handlers/auth_handler.rs`, `src/models/user.rs`)
- Passwords hashed with `argon2`. At startup the server ensures a default admin account `admin/admin` exists (`ensure_default_admin`).
- JWT is signed/verified via `jsonwebtoken`; the middleware reads `Authorization: Bearer <token>`.

6) Data Models (examples)
- `StandardField` (`src/models/field.rs`): includes `composition_ids: Vec<i32>` linking to word-root IDs, `associated_terms`, etc.
- `User` (`src/models/user.rs`): user entity and JWT `Claims`.

## Important Functions (quick index)
- Startup & state: [src/main.rs](src/main.rs#L235)
  - `main()` — starts service, loads model, initializes Qdrant, syncs vectors ([src/main.rs](src/main.rs#L235)).
  - `sync_roots_to_qdrant()` — embed & upload word-root vectors ([src/main.rs](src/main.rs#L95)).
  - `sync_fields_to_qdrant()` — embed & upload standard-field vectors ([src/main.rs](src/main.rs#L145)).
  - `ensure_default_admin()` — create default admin user if missing ([src/main.rs](src/main.rs#L66)).
- Tokenization suggestion:
  - Handler: [src/handlers/mapping_handler.rs](src/handlers/mapping_handler.rs#L34)::`suggest_mapping`
  - Service: [src/services/mapping_service.rs](src/services/mapping_service.rs#L11)::`suggest_field_name`
- Semantic search:
  - Handler: [src/handlers/mapping_handler.rs](src/handlers/mapping_handler.rs#L53)::`search_similar_roots`
  - Uses Qdrant `search_points` / `upsert_points` via `qdrant-client`.
- Middleware guard: [src/middleware/auth.rs](src/middleware/auth.rs#L14)::`guard`
- Field-related:
  - Create field: [src/handlers/field_handler.rs](src/handlers/field_handler.rs#L14)::`create_field`
  - User search: [src/handlers/field_handler.rs](src/handlers/field_handler.rs#L224)::`search_field`
- Word-root related:
  - Create root: [src/handlers/word_root_handler.rs](src/handlers/word_root_handler.rs#L51)::`create_root`
  - Batch import: [src/handlers/word_root_handler.rs](src/handlers/word_root_handler.rs#L121)::`batch_create_roots`
- Tasks:
  - Submit task: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L21)::`submit_task`
  - List tasks: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L45)::`list_tasks`
  - Complete task: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L61)::`complete_task`
  - Count unprocessed: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L75)::`count_unprocessed_tasks`

## Config & External Dependencies
- Required environment variables: `DATABASE_URL`, `QDRANT_URL` (defaults to `http://localhost:6334`), `RUST_LOG`.
- See `qdrant/config.yaml` for recommended production configuration (on_disk_payload, memmap, WAL tuning).

## Development Notes
- Ensure `model/fastembed_cache` is present for offline model loading.
- Embedding model is wrapped in a global `Mutex`; avoid holding the lock during long IO operations. The code uses short lock scopes.
- Using `on_disk_payload: true` in Qdrant reduces memory usage for large metadata.

## Common Commands
```bash
# Run
cargo run

# Build release
cargo build --release
```

## References
- Entry: [src/main.rs](src/main.rs)
- Handlers: [src/handlers/mod.rs](src/handlers/mod.rs)
- Mapping service: [src/services/mapping_service.rs](src/services/mapping_service.rs)
- Field model example: [src/models/field.rs](src/models/field.rs)
- Middleware: [src/middleware/auth.rs](src/middleware/auth.rs)
- Qdrant config: [qdrant/config.yaml](qdrant/config.yaml)

---