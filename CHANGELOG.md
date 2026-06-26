# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0.0] - 2026-06-26

### Added
- Smart routing engine: classify request complexity (Simple/Medium/Complex) and route to tiered models with deterministic fallback chain
- Token budget management: daily/monthly cloud token tracking with 429 responses on exhaustion, debounced persistence to `~/.tinyfusion/budget.json`
- `GET /v1/budget` API endpoint for real-time budget status
- `GET /v1/config` now returns `fusion.models`, `fusion.routing`, and `fusion.budget` configuration
- `ModelEntry` supports `tier`, `is_local`, and `chat_path` fields for per-model routing and provider adapter configuration
- `RoutingConfig` and `BudgetConfig` structures for v2 configuration
- Dashboard budget card with daily/monthly usage progress bars (auto-refreshes every 5s)
- Models page displays Smart Routing Models registry with tier badges, LOCAL indicators, and per-model connection testing
- Session persistence: `save_snapshot()` now called on session create, state change, transition, and removal
- Config v1-to-v2 backward compatibility with startup warnings for missing v2 fields

### Changed
- `handle_passthrough()` now preserves the full original request body, only rewriting the `model` field (previously dropped `tools`, `temperature`, `max_tokens`, etc.)
- `decide_route()` only applies smart routing when the client model field matches a registered fusion model, preserving OpenAI-compatible passthrough semantics for direct model names like `gpt-4o`
- Panel dispatcher and judge synthesizer now use `build_chat_url_with_path()` to support custom API paths (Zhipu, etc.)
- Preset resolution uses alphabetically sorted keys for deterministic behavior across restarts
- Metrics file renamed from `fusion_metrics.jsonl` to `metrics.jsonl`
- `FusionMetrics` type renamed to `RequestMetrics`
- `sniffer` uses caller-provided error keywords with default fallback (DRY)
- Session display name truncation uses char-boundary-safe slicing (prevents panic on non-ASCII)
- Legacy models (workers/judge/executor) shown in collapsible section on Models page

### Fixed
- Budget bypass: smart routing fallthrough no longer skips budget checks via legacy executor path
- All requests no longer hijacked by complexity router when `fusion.routing` is configured
- Panel/Judge calls now respect `ModelEntry.chat_path` for providers with non-standard endpoints
- `SessionManager` implements `Default` trait (clippy warning)
- Module-level doc comments use `//!` instead of `///` (clippy warnings)

## [0.1.1.0] - 2026-06-18

### Changed
- Server now uses `SO_REUSEADDR` for faster restarts without "address in use" errors
- Sidecar shutdown waits up to 5 seconds for graceful exit, then sends SIGKILL
- Tauri GUI uses `tauri::async_runtime::spawn` for correct async runtime integration

### Fixed
- Settings page now merges saved config with defaults, preventing missing-field crashes
- Settings page Tauri API calls use v1/v2 compatible invoke helper
- CSS global reset wrapped in `@layer base` for proper cascade isolation
- Unused `RotateCcw` import removed from Settings page

## [0.1.0.0] - 2026-06-17

### Added
- Initial release: TinyFusion core server, Tauri GUI, React frontend
