# TinyFusion — AGENTS.md

> Compact instruction file for OpenCode agents working in this repo.
> Last updated: 2026-06-22 (version 0.1.1.0)

## Project Identity

Local AI API gateway with MoA (Multi-of-Agents) diagnostic, SSE keep-alive, and verify-and-retry oracle. Desktop app via Tauri v2 + Rust core server + React frontend.

## Architecture (two-crate Rust workspace)

```
tinyfusion-core/     — Axum HTTP server (port 9999), the runtime engine
tinyfusion-gui/      — Tauri v2 desktop shell, manages core as sidecar process
frontend/            — React 19 + Vite 8 + Tailwind CSS v4 + React Router v7
```

**Core module map** (`tinyfusion-core/src/`, 10 `pub mod` in `lib.rs`, no re-exports):
| Module | Job |
|---|---|
| `server` | Axum router: 5 routes, graceful shutdown, `test_app()` helper |
| `chat` | `/v1/chat/completions` handler; 3-phase routing (Diagnostic→Execution→Verify) |
| `config` | `~/.tinyfusion/config.json` load/save, `ModelConfig`, `WorkspaceConfig` |
| `session` | `SessionManager` (`Mutex<HashMap>`), state machine, JSON snapshot to disk |
| `sniffer` | State detection: `</final_plan>` tag → Execution, error keywords → Diagnostic |
| `moa` | Worker fan-out (concurrent `tokio::spawn`), Judge prompt, XML tag parsing |
| `proxy` | Upstream request forwarding, streaming passthrough (reqwest → Axum Body) |
| `keepalive` | SSE `: keepalive` every 15s with `CancellationToken` |
| `oracle` | Verify command runner (`tokio::process::Command`), timeout, error formatting |
| `events` | `EventBus` (broadcast channel), SSE event stream for `/v1/events` |

**State machine**: `Diagnostic → Execution → Verify → Done` (retry cycling on verify failure).

**GUI** (`tinyfusion-gui/src/main.rs`, single 127-line file):
- 3 Tauri commands: `get_config`, `save_config`, `restart_core`
- Sidecar lifecycle: auto-starts core on app launch, SIGTERM+SIGKILL on stop
- Config path hardcoded: `~/.tinyfusion/config.json` (no env override)

**Frontend pages** (React Router v7):
| Route | Component | Backend calls |
|---|---|---|
| `/` | Dashboard | None (mock data) |
| `/sessions` | Sessions | None (mock data) |
| `/models` | Models | None (mock data) |
| `/logs` | Logs | None (mock data) |
| `/settings` | Settings | `get_config`, `save_config`, `restart_core` |

## Developer Commands

### Quick Reference
```bash
npm run dev              # Full stack: Vite (:5173) + Tauri GUI (reuse Vite)
cargo test               # Workspace-wide tests (both crates)
cargo check              # Typecheck entire workspace
cargo clippy             # Lint entire workspace
cd frontend && npm run lint  # ESLint (typescript-eslint + react-hooks)
```

### Faster iteration
```bash
cargo test --package tinyfusion-core          # Core tests only
cargo check --package tinyfusion-core         # Faster: skip GUI
cd frontend && npx tsc -b --noEmit            # TS typecheck only (no build)
```

### Production build
```bash
cd frontend && npm run build   # tsc + vite build → frontend/dist/
cargo tauri build              # Full Tauri bundle (.app + .dmg on macOS)
```

### Port conflict
```bash
lsof -ti:5173 | xargs kill -9 2>/dev/null; true   # Kill stale Vite
```
The root `npm run predev` does this automatically.

## Testing

- **Rust**: inline `#[cfg(test)]` modules in every source file + `tinyfusion-core/tests/integration_test.rs`
- **No frontend tests** — no Vitest/Jest configured
- Build requires `cargo build` of `tinyfusion-core` before `cargo tauri dev` (sidecar binary)

## Config

- **Path**: `~/.tinyfusion/config.json` — auto-created with defaults on first run
- **Schema**: `port`, `workers[]`, `judge`, `executor`, `workspaces{}`, `error_keywords[]`
- Only `Settings.tsx` reads/writes it; all other frontend pages use hardcoded mock data

## Conventions

### Rust
- No `pub use` re-exports — use `tinyfusion_core::module::Type`
- `pub(crate)` visibility on internal handlers (e.g., `chat_completions`, `ChatResponse`)
- Shared state via `Arc<Config>` + `Arc<SessionManager>` + `Arc<EventBus>` in `AppState`
- `CancellationToken` for cooperative async shutdown (keepalive → data arrival)

### Frontend
- **Styling**: Tailwind v4 (`@import "tailwindcss"` in `index.css`, no `tailwind.config.js`). Two patterns coexist (Tailwind classes in Sidebar/Sessions/Settings vs inline CSS vars in Dashboard/Models/Logs) — prefer Tailwind classes for new work.
- **Tauri IPC**: `window.__TAURI__.core.invoke(cmd, args)` — NO `@tauri-apps/api` npm package. See `Settings.tsx` for the adapter pattern (handles both v1/v2 invoke signatures).
- **Theme**: `data-theme="dark|light"` on `<html>`, stored in `localStorage('theme')`, toggled in Sidebar.
- **Fonts**: Inter (UI) + JetBrains Mono (code/numbers)
- **Icons**: `lucide-react`
- **TypeScript 6.0**: `noUnusedLocals`, `noUnusedParameters`, `erasableSyntaxOnly` enabled

## Gotchas

1. **No CI, no pre-commit, no `rust-toolchain.toml`** — everything is local-only. Run `cargo test` and `npm run lint` manually before pushing.
2. **OS-specific sidecar binary** in `tinyfusion-gui/src-tauri/binaries/`. Must be rebuilt for each platform: `cargo build --release -p tinyfusion-core`, then copy to the correct target-triple path.
3. **Port 9999 binding**: Core uses `SO_REUSEADDR` but not `SO_REUSEPORT`. Second instance will fail on port conflict.
4. **Config path is hardcoded** — `~/.tinyfusion/config.json` cannot be overridden by env var.
5. **GUI kills sidecar with SIGKILL fallback** after 5s graceful window — no `wait()` for child process.
6. **`libc` is unix-only** — the GUI crate won't compile on Windows without changes.
7. **`withGlobalTauri: true`** in `tauri.conf.json` — enables `window.__TAURI__` without npm imports. Necessary for the current IPC pattern.
8. **Version bump**: edit `VERSION` file + `CHANGELOG.md` manually; no automation.
