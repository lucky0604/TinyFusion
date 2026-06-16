# PRD: TinyFusion MVP — Fine-Grained User Stories

## Introduction

TinyFusion is a lightweight, local-running AI API gateway built in Rust (Axum + Tokio) with a Tauri desktop GUI. It implements **Sequential State Sniffing**, **SSE Stream Keepalives**, and a **Verify-and-Retry Local Oracle** to deliver an automated self-correcting debugging loop for coding agents while reducing API costs by over 90%.

This PRD breaks the MVP into 44 atomic user stories across 10 implementation phases, designed for sequential execution via Ralph Loop.

---

## Goals

- Build a working local AI API gateway that proxies OpenAI-compatible requests
- Implement MoA (Mixture of Agents) for diagnostic phase with 2-3 workers + 1 judge
- Provide SSE keep-alive during long MoA computations (5-15 seconds)
- Implement verify-and-retry oracle for self-correcting code modifications
- Create a Tauri desktop GUI with dashboard, model config, and log viewer
- Achieve <15MB memory footprint for the core daemon
- Support dark/light themes following the refined developer console design system

---

## Implementation Phases

### Phase 1: Foundation (Core + GUI Shell)

#### US-001: Rust Project Scaffolding
**Description:** As a developer, I want a well-structured Rust workspace with `tinyfusion-core` and `tinyfusion-gui` crates so that the codebase is modular and maintainable.

**Acceptance Criteria:**
- [ ] Cargo workspace with two member crates: `tinyfusion-core` and `tinyfusion-gui`
- [ ] `tinyfusion-core` has `src/main.rs`, `src/lib.rs`, `src/sniffer.rs`, `src/session.rs`, `src/moa/`, `src/keepalive.rs`, `src/oracle.rs`, `src/proxy.rs`
- [ ] `tinyfusion-gui` has standard Tauri v2 structure
- [ ] `cargo check` passes with no errors
- [ ] `cargo test` passes (no tests yet, just compilation)

#### US-002: Axum HTTP Server Setup
**Description:** As a developer, I want a basic Axum HTTP server listening on port 9999 so that it can accept incoming API requests.

**Acceptance Criteria:**
- [ ] Axum server binds to `127.0.0.1:9999`
- [ ] `GET /health` returns `{"status": "ok"}`
- [ ] Graceful shutdown on SIGINT/SIGTERM
- [ ] Server startup logs to stdout with port number
- [ ] `cargo test` passes

#### US-003: Config File Loading
**Description:** As a developer, I want the core to load configuration from `~/.tinyfusion/config.json` so that model endpoints and settings are persistent.

**Acceptance Criteria:**
- [ ] Config schema: `port`, `workers[]`, `judge`, `executor`, `workspaces{}`
- [ ] Create default config if file doesn't exist
- [ ] Parse config with serde_json, return typed struct
- [ ] Log config load success with model count
- [ ] `cargo test` passes with config parsing tests

#### US-004: Tauri App Shell
**Description:** As a user, I want a Tauri desktop application that opens a window with the TinyFusion GUI so that I can configure and monitor the gateway.

**Acceptance Criteria:**
- [ ] Tauri v2 app launches with a native window (1200x800 default)
- [ ] Window title: "TinyFusion"
- [ ] WebView loads `index.html` from frontend assets
- [ ] App icon displayed in taskbar/dock
- [ ] `cargo tauri dev` launches the app successfully

#### US-005: Sidebar Navigation
**Description:** As a user, I want a sidebar navigation with Dashboard, Models, Logs, and Settings so that I can navigate between pages.

**Acceptance Criteria:**
- [ ] Sidebar width: 240px (expanded), 48px (collapsed icon-only at <768px)
- [ ] Nav items: Dashboard, Models, Logs, Settings (with Lucide icons)
- [ ] Active item has accent bg + left border indicator
- [ ] Hover state shifts background color
- [ ] Click navigates to route (React Router or equivalent)
- [ ] Verify in browser using dev-browser skill

---

### Phase 2: Core Gateway (Passthrough)

#### US-006: /v1/chat/completions Endpoint
**Description:** As a developer, I want the gateway to expose a standard OpenAI `/v1/chat/completions` endpoint so that AI coding tools can connect to it.

**Acceptance Criteria:**
- [ ] POST `/v1/chat/completions` accepts JSON body with `model`, `messages`, `stream`
- [ ] Request validated against OpenAI schema (reject invalid with 400)
- [ ] Returns `200 OK` with standard chat completion response
- [ ] Supports both streaming and non-streaming modes
- [ ] `cargo test` passes with request/response tests

#### US-007: Single Model Passthrough
**Description:** As a developer, I want the gateway to forward requests to a configured upstream model so that I can verify the proxy chain works.

**Acceptance Criteria:**
- [ ] Request forwarded to configured upstream endpoint (e.g., Ollama)
- [ ] Response body passed through unchanged
- [ ] Upstream errors forwarded with correct HTTP status
- [ ] Logging shows request → upstream URL mapping
- [ ] `cargo test` passes with mock upstream

#### US-008: SSE Streaming Support
**Description:** As a developer, I want the gateway to stream SSE responses from upstream so that clients receive tokens in real-time.

**Acceptance Criteria:**
- [ ] SSE headers emitted correctly (`Content-Type: text/event-stream`, etc.)
- [ ] Each upstream chunk forwarded as-is to client
- [ ] Stream closed when upstream closes connection
- [ ] Client disconnect handled gracefully (abort upstream)
- [ ] `cargo test` passes with streaming tests

---

### Phase 3: Model Configuration (GUI)

#### US-009: Model Config Page Layout
**Description:** As a user, I want a Model Configuration page with sections for Workers, Judge, and Executor so that I can organize my AI models by role.

**Acceptance Criteria:**
- [ ] Page route: `/models`
- [ ] Three sections: Workers, Judge, Executor
- [ ] Each section has header with role name, description, and "Add" button
- [ ] Sections separated by 32px gap
- [ ] Follows design system card/section patterns
- [ ] Verify in browser using dev-browser skill

#### US-010a: Model Card Display
**Description:** As a user, I want to see each configured model as a card showing name, status, and endpoint so that I can identify models at a glance.

**Acceptance Criteria:**
- [ ] Card shows: model name (md, 600 weight), status dot, endpoint URL (mono), API key (masked)
- [ ] Status dot colors: green (connected), red (error), amber (testing), gray (untested)
- [ ] Copy URL button with "Copied" feedback
- [ ] Hover: background shifts to `--bg-hover`
- [ ] Verify in browser using dev-browser skill

#### US-010b: Model Card Status States
**Description:** As a user, I want model cards to visually indicate connection status so that I can quickly identify issues.

**Acceptance Criteria:**
- [ ] Connected: status dot `--status-active` (green), static
- [ ] Error: status dot `--status-error` (red), static, error message below endpoint
- [ ] Testing: status dot `--status-warning` (amber), pulse animation
- [ ] Untested: status dot `--status-inactive` (gray), "Untested" label
- [ ] Verify in browser using dev-browser skill

#### US-010c: Model Card Actions Menu
**Description:** As a user, I want actions on model cards (Test, Edit, Delete) so that I can manage models.

**Acceptance Criteria:**
- [ ] "Test" secondary button: sends test request, shows spinner
- [ ] "⋯" ghost icon button: dropdown with Edit, Duplicate, Delete
- [ ] Delete action: confirmation dialog before removal
- [ ] Verify in browser using dev-browser skill

#### US-011a: Add/Edit Model Dialog Structure
**Description:** As a user, I want a dialog to add or edit a model with provider selection and form fields.

**Acceptance Criteria:**
- [ ] Dialog max width: 520px
- [ ] Provider tabs: Ollama, OpenAI, Anthropic, DeepSeek, Custom
- [ ] Form fields: Display Name, Endpoint URL, Model ID, API Key (optional)
- [ ] "Test connection after saving" checkbox (default: checked)
- [ ] Save button disabled until all required fields filled
- [ ] Verify in browser using dev-browser skill

#### US-011b: Add/Edit Model Form Validation
**Description:** As a user, I want form validation to prevent saving invalid model configurations.

**Acceptance Criteria:**
- [ ] Required fields: Provider, Display Name, Endpoint URL, Model ID
- [ ] URL format validation (http/https)
- [ ] API key warning for non-localhost endpoints
- [ ] Inline errors below fields in `--text-xs`, `--status-error`
- [ ] Save disabled until validation passes
- [ ] Verify in browser using dev-browser skill

#### US-012: Connection Testing
**Description:** As a user, I want to test a model connection so that I can verify it's working before saving.

**Acceptance Criteria:**
- [ ] "Test" button sends minimal request to model endpoint
- [ ] Shows spinner during test
- [ ] Success: status dot turns green, "Connected" label
- [ ] Failure: status dot turns red, error message displayed
- [ ] Test timeout: 10 seconds
- [ ] Verify in browser using dev-browser skill

#### US-013: Global Settings Section
**Description:** As a user, I want global settings for port, retries, timeout, and workspace verification commands.

**Acceptance Criteria:**
- [ ] Settings fields: Port (number), Max Retries (1-10), Verify Timeout (seconds), Keep Core Running (toggle)
- [ ] Workspace commands: key-value pairs (path → verify command)
- [ ] Add/remove workspace commands
- [ ] Settings persisted to config file on save
- [ ] Verify in browser using dev-browser skill

---

### Phase 4: Dashboard (GUI)

#### US-014: Dashboard Page Layout
**Description:** As a user, I want a Dashboard page showing active session cards and a live activity log so that I can monitor gateway activity.

**Acceptance Criteria:**
- [ ] Page route: `/` (default view)
- [ ] Layout: session cards grid (top) + live activity log (bottom)
- [ ] Header: "Dashboard" title + "New Session" button
- [ ] Responsive: 1/2/3 columns based on breakpoint
- [ ] Follows dashboard.md design spec
- [ ] Verify in browser using dev-browser skill

#### US-015a: Session Card Display
**Description:** As a user, I want session cards showing basic info (state badge, session ID) so that I can identify sessions.

**Acceptance Criteria:**
- [ ] Card shows: status dot, state badge (Diagnostic/Execution/Verify/Retry/Done), session ID
- [ ] State colors: green (Diagnostic), blue (Execution), amber (Verify), red (Retry)
- [ ] Card border-left: 3px color matching state
- [ ] Empty state: "No Active Sessions" with gateway endpoint display
- [ ] Verify in browser using dev-browser skill

#### US-015b: Session Card Stats Block
**Description:** As a user, I want session cards to show real-time stats (workers, duration, requests, tokens).

**Acceptance Criteria:**
- [ ] Stats block: Workers active, Duration, Requests, Tokens
- [ ] Numbers update in real-time
- [ ] Mono font for numeric values
- [ ] Verify in browser using dev-browser skill

#### US-015c: Session Card Actions & Retry State
**Description:** As a user, I want session card actions and retry error context.

**Acceptance Criteria:**
- [ ] "End Session" ghost-danger button with confirmation dialog
- [ ] Retry state shows error summary (2 lines max)
- [ ] Error text: mono font, truncated with ellipsis
- [ ] Suggested action in plain language
- [ ] Verify in browser using dev-browser skill

#### US-016a: Live Activity Log Entry Rendering
**Description:** As a user, I want log entries to display timestamp, phase tag, session ID, and message clearly.

**Acceptance Criteria:**
- [ ] Panel height: flex-1, min 200px
- [ ] Log entries: timestamp (mono), phase tag, session ID, message
- [ ] Phase tag colors: green (diag), blue (exec), amber (veri), red (retry), gray (info)
- [ ] Empty state: "Waiting for activity..."
- [ ] Verify in browser using dev-browser skill

#### US-016b: Live Activity Log Auto-Scroll
**Description:** As a user, I want the log to auto-scroll to new entries but pause when I scroll up.

**Acceptance Criteria:**
- [ ] Header: "Live Activity" + auto-scroll toggle + "Clear" button
- [ ] Auto-scroll to bottom (pauses when user scrolls up)
- [ ] "↓ New entries" indicator when paused
- [ ] Click indicator or scroll to bottom resumes auto-scroll
- [ ] New entry highlight animation (500ms fade)
- [ ] Verify in browser using dev-browser skill

---

### Phase 5: State Machine (Core)

#### US-017: Sequential State Sniffer
**Description:** As a developer, I want the gateway to detect request state (Diagnostic/Execution) by inspecting message history so that it can route to the correct handler.

**Acceptance Criteria:**
- [ ] `sniff_state()` function inspects `messages` array
- [ ] Diagnostic: msg_count <= 2 OR last message contains error keywords (stack trace, compile error, test failed)
- [ ] Execution: messages contain `</final_plan>` tag
- [ ] Default: Diagnostic
- [ ] `cargo test` passes with 4+ test cases

#### US-018: Session Manager
**Description:** As a developer, I want an in-memory session manager that tracks sessions by hash/UUID so that state transitions persist across requests.

**Acceptance Criteria:**
- [ ] Session identified by: `user` field, explicit `session_id`, or hash of first system+user messages
- [ ] Session struct: id, state, retry_count, messages, created_at
- [ ] In-memory HashMap storage
- [ ] Session lookup by identifier
- [ ] `cargo test` passes with session creation/lookup tests

#### US-019: State Transition Logic
**Description:** As a developer, I want the state machine to transition between states based on sniff results and verification outcomes.

**Acceptance Criteria:**
- [ ] Diagnostic → Execution: when `</final_plan>` present in history
- [ ] Execution → Verify: when executor response contains completion marker
- [ ] Verify → Done: when verify command succeeds (exit 0)
- [ ] Verify → Diagnostic: when verify fails AND retry_count < max
- [ ] Verify → Done: when retry_count >= max
- [ ] `cargo test` passes with transition tests

---

### Phase 6: MoA Integration (Core)

#### US-020: Worker Concurrent Execution
**Description:** As a developer, I want the gateway to spawn 2-3 worker model calls in parallel during Diagnostic phase so that multiple models analyze the problem.

**Acceptance Criteria:**
- [ ] Worker endpoints configured in config file
- [ ] `tokio::spawn` for each worker call
- [ ] All workers called concurrently (not sequentially)
- [ ] Worker timeout: 30 seconds
- [ ] Collect worker responses (success or timeout error)
- [ ] `cargo test` passes with mock workers

#### US-021a: Judge System Prompt Construction
**Description:** As a developer, I want the gateway to construct a structured Judge prompt that requests XML-formatted diagnostic output.

**Acceptance Criteria:**
- [ ] Judge system prompt requests: `<consensus>`, `<contradictions>`, `<coverage_gaps>`, `<unique_insights>`, `<final_plan>`
- [ ] Judge receives: original prompt + all worker outputs
- [ ] Prompt enforces XML tag structure
- [ ] `cargo test` passes with prompt construction tests

#### US-021b: Judge XML Response Parsing
**Description:** As a developer, I want the gateway to parse structured XML tags from Judge response.

**Acceptance Criteria:**
- [ ] Parse XML tags: consensus, contradictions, coverage_gaps, unique_insights, final_plan
- [ ] Fallback: if XML parsing fails, use raw Judge output
- [ ] Extract final_plan for state transition detection
- [ ] `cargo test` passes with XML parsing tests

#### US-022: MoA Response Aggregation
**Description:** As a developer, I want the gateway to aggregate worker responses and send them to the Judge so that we get a comprehensive diagnostic.

**Acceptance Criteria:**
- [ ] Aggregate worker responses into single payload
- [ ] Include original user prompt in Judge request
- [ ] Judge response streamed back to client as SSE
- [ ] Keep-alive active during Judge computation
- [ ] `cargo test` passes

---

### Phase 7: Keep-Alive (Core)

#### US-023: SSE Header Emission
**Description:** As a developer, I want the gateway to emit SSE headers immediately so that clients don't timeout during long computations.

**Acceptance Criteria:**
- [ ] Headers: `Content-Type: text/event-stream`, `Cache-Control: no-cache`, `Connection: keep-alive`, `X-Accel-Buffering: no`
- [ ] Headers sent within 500ms of request
- [ ] `cargo test` passes

#### US-024: Keepalive Comment Injection
**Description:** As a developer, I want the gateway to send SSE keepalive comments every 15 seconds so that clients stay connected during MoA computation.

**Acceptance Criteria:**
- [ ] Every 15 seconds of inactivity, send `: keepalive\n\n`
- [ ] Keepalive comments are valid SSE (lines starting with `:` are ignored by parsers)
- [ ] Keepalive stops when real tokens arrive
- [ ] `cargo test` passes with timing tests

#### US-025: Transition to Real Tokens
**Description:** As a developer, I want the gateway to seamlessly switch from keepalive comments to real tokens so that clients receive data without interruption.

**Acceptance Criteria:**
- [ ] When Judge produces output, stop keepalive
- [ ] Stream real tokens as SSE `data:` events
- [ ] No gap or delay in transition
- [ ] Client receives continuous stream
- [ ] `cargo test` passes

---

### Phase 8: Oracle (Core)

#### US-026: Command Execution with Output Capture
**Description:** As a developer, I want the gateway to run verification commands and capture exit code and output.

**Acceptance Criteria:**
- [ ] Command runs via `tokio::process::Command`
- [ ] Working directory set to workspace path
- [ ] Timeout: configurable (default 45 seconds)
- [ ] Capture stdout and stderr
- [ ] Exit code 0 = success, non-zero = failure
- [ ] Handle timeout (kill process, treat as failure)
- [ ] `cargo test` passes with mock command

#### US-028: Error Injection into Messages
**Description:** As a developer, I want the gateway to inject verification errors back into the messages array so that the next diagnostic cycle can fix them.

**Acceptance Criteria:**
- [ ] On verify failure, append user message with error output
- [ ] Message format: "Local verification failed with exit code X. Stderr:\n```\n...\n```\nPlease re-analyze."
- [ ] Message injected before next Diagnostic phase
- [ ] `cargo test` passes

#### US-029: Retry Loop Logic
**Description:** As a developer, I want the gateway to retry failed verifications up to max_retries so that the system can self-correct.

**Acceptance Criteria:**
- [ ] Retry counter per session
- [ ] On verify failure: if retry_count < max, transition to Diagnostic
- [ ] On verify failure: if retry_count >= max, transition to Done
- [ ] Log retry attempt with count
- [ ] `cargo test` passes

---

### Phase 9: Logs (GUI)

#### US-030: Logs Page Layout
**Description:** As a user, I want a Logs page with a filter bar and full log table so that I can investigate gateway activity.

**Acceptance Criteria:**
- [ ] Page route: `/logs`
- [ ] Layout: filter bar (top) + log table (bottom) + footer
- [ ] Header: "Logs" title + "Export" button
- [ ] Follows logs.md design spec
- [ ] Verify in browser using dev-browser skill

#### US-031a: Log Search Input
**Description:** As a user, I want to search log messages by text so that I can find relevant entries.

**Acceptance Criteria:**
- [ ] Full-width search input with magnifying glass icon
- [ ] Debounced 150ms search
- [ ] Searches message content only
- [ ] Clear button (✕) when text entered
- [ ] Verify in browser using dev-browser skill

#### US-031b: Log Phase & Session Filters
**Description:** As a user, I want to filter logs by phase and session so that I can narrow down results.

**Acceptance Criteria:**
- [ ] Phase filter: multi-select dropdown (diag, exec, veri, retry, info)
- [ ] Session filter: multi-select dropdown with active/recent sessions
- [ ] "Select All" / "Deselect All" quick actions
- [ ] Apply on close (not live)
- [ ] Verify in browser using dev-browser skill

#### US-031c: Active Filter Indicators
**Description:** As a user, I want to see active filters as removable pills so that I can manage my filter state.

**Acceptance Criteria:**
- [ ] Active filters shown as pills below search bar
- [ ] Each pill has ✕ to remove
- [ ] "Clear all filters" action
- [ ] Filter count displayed
- [ ] Verify in browser using dev-browser skill

#### US-032a: Log Entry Table Structure
**Description:** As a user, I want a log table with properly formatted columns so that I can scan entries efficiently.

**Acceptance Criteria:**
- [ ] Columns: Timestamp (120px), Phase (64px), Session (120px), Message (flex-1), Actions (40px)
- [ ] Row height: 32px (compact)
- [ ] Alternating row backgrounds
- [ ] Sticky table header
- [ ] Verify in browser using dev-browser skill

#### US-032b: Log Table Row Interactions
**Description:** As a user, I want to interact with log rows (hover, click, expand) to see details.

**Acceptance Criteria:**
- [ ] Hover: background shift + copy icon appears
- [ ] Click: expands inline detail view with full context
- [ ] Expanded view shows: Session ID, Request Size, Workers, Judge, Full JSON
- [ ] "Copy JSON" and "Copy Session ID" actions
- [ ] Virtual scrolling if >10,000 entries
- [ ] Verify in browser using dev-browser skill

#### US-033: Export Functionality
**Description:** As a user, I want to export filtered logs as JSON, CSV, or plain text so that I can analyze them externally.

**Acceptance Criteria:**
- [ ] Export dialog with format selection (JSON, CSV, Plain Text)
- [ ] Date range picker
- [ ] Session and phase filters applied
- [ ] Shows estimated entry count
- [ ] Downloads file with timestamp in filename
- [ ] Verify in browser using dev-browser skill

---

### Phase 10: Polish & Testing

#### US-034: Light Mode Toggle
**Description:** As a user, I want to switch between dark and light themes so that I can use my preferred color scheme.

**Acceptance Criteria:**
- [ ] Theme toggle in sidebar footer (or Settings page)
- [ ] Persists to localStorage
- [ ] Light mode colors follow MASTER.md §8
- [ ] All components work in both themes
- [ ] System preference detection as default
- [ ] Verify in browser using dev-browser skill

#### US-035: Mobile Responsiveness
**Description:** As a user, I want the GUI to work on mobile screens with a hamburger menu so that I can access it from any device.

**Acceptance Criteria:**
- [ ] Sidebar collapses to hamburger menu at <768px
- [ ] Hamburger opens sidebar as slide-out overlay
- [ ] Session cards stack vertically on mobile
- [ ] Log panel full-width, min-height 150px
- [ ] Touch targets minimum 36px
- [ ] Verify in browser using dev-browser skill

#### US-036: Unit Tests
**Description:** As a developer, I want comprehensive unit tests for core modules so that I can refactor with confidence.

**Acceptance Criteria:**
- [ ] `tests/sniffer_test.rs`: 4+ test cases for state detection
- [ ] `tests/session_test.rs`: session creation, lookup, state transition
- [ ] `tests/moa_test.rs`: worker execution, judge parsing
- [ ] `tests/keepalive_test.rs`: header emission, comment injection, transition
- [ ] `tests/oracle_test.rs`: command execution, exit code, output capture
- [ ] `tests/proxy_test.rs`: request forwarding, SSE passthrough
- [ ] `cargo test` passes with >80% coverage

#### US-037: Integration Tests
**Description:** As a developer, I want integration tests for full flows so that I can verify end-to-end behavior.

**Acceptance Criteria:**
- [ ] `tests/integration_test.rs`: Full Diagnostic → Execution → Verify → Done cycle
- [ ] Retry loop: Verify fails → Diagnostic → retry until max
- [ ] SSE stream continuity (no client disconnect during MoA)
- [ ] Upstream model timeout handling
- [ ] Invalid config handling
- [ ] `cargo test` passes

---

## Functional Requirements

- FR-1: Gateway exposes `/v1/chat/completions` endpoint on port 9999
- FR-2: Gateway proxies requests to configured upstream models
- FR-3: Gateway streams SSE responses from upstream
- FR-4: Gateway detects request state via sequential sniffing
- FR-5: Gateway spawns 2-3 workers in parallel during Diagnostic phase
- FR-6: Gateway sends worker outputs to Judge and parses XML response
- FR-7: Gateway sends keepalive comments every 15 seconds during MoA
- FR-8: Gateway runs verification commands with timeout and captures output
- FR-9: Gateway injects verification errors back into messages
- FR-10: Gateway retries failed verifications up to max_retries
- FR-11: GUI provides Dashboard with session cards and live log
- FR-12: GUI provides Model Configuration with role-based sections
- FR-13: GUI provides Logs page with filtering and export
- FR-14: GUI supports dark and light themes
- FR-15: GUI is responsive with mobile hamburger menu

---

## Non-Goals (Out of Scope)

- No multi-tenant support (single-user local tool)
- No authentication/authorization (local only)
- No persistent session storage (in-memory is correct for 15MB budget)
- No configuration hot-reload (requires restart)
- No logging/observability (add after core works)
- No rate limiting (single-user)
- No graceful degradation (fail fast if upstream fails)
- No load balancing across workers (sequential state sniffing)
- No Session detail view (post-MVP)
- No Settings page full design (post-MVP)

---

## Design Considerations

- **Design System**: Follow MASTER.md with Inter font, Lucide icons, 8px grid
- **Color Strategy**: Dark-first, indigo accent (#6366F1), semantic status colors
- **Component Patterns**: Cards with border-based depth, no shadows in dark mode
- **Accessibility**: WCAG AA, keyboard navigation, focus indicators, reduced motion
- **Performance**: <15MB memory footprint for core daemon

---

## Technical Considerations

- **Core Stack**: Rust, Axum, Tokio, Reqwest, Serde
- **GUI Stack**: Tauri v2, React 18+, Tailwind CSS 3.x, Lucide React
- **State Management**: React Context + useReducer (or Zustand)
- **Testing**: `#[cfg(test)]` with `cargo test`
- **Fonts**: Bundle Inter + JetBrains Mono as local WOFF2

---

## Success Metrics

- Core daemon starts in <500ms
- Memory footprint <15MB under load
- SSE keepalive prevents client timeout (100% success rate)
- MoA computation completes in 5-15 seconds
- Verify-and-retry reduces manual debugging by 80%
- GUI launches in <2 seconds
- All unit tests pass with >80% coverage

---

## Open Questions

1. Should we bundle font files in the Tauri app or download on first launch?
2. What's the exact Tauri v2 sidecar configuration for spawning tinyfusion-core?
3. Should the GUI communicate with core via HTTP or IPC?
4. How to handle concurrent requests to the same session?

---

## Implementation Order

| Phase | Stories | Estimated Effort |
|-------|---------|------------------|
| 1. Foundation | US-001 to US-005 | 2-3 hours |
| 2. Core Gateway | US-006 to US-008 | 2-3 hours |
| 3. Model Config | US-009, US-010a-c, US-011a-b, US-012, US-013 | 4-5 hours |
| 4. Dashboard | US-014, US-015a-c, US-016a-b | 4-5 hours |
| 5. State Machine | US-017 to US-019 | 2-3 hours |
| 6. MoA Integration | US-020, US-021a-b, US-022 | 4-5 hours |
| 7. Keep-Alive | US-023 to US-025 | 2-3 hours |
| 8. Oracle | US-026, US-028, US-029 | 3-4 hours |
| 9. Logs | US-030, US-031a-c, US-032a-b, US-033 | 4-5 hours |
| 10. Polish & Testing | US-034 to US-037 | 4-5 hours |
| **Total** | **44 stories** | **31-41 hours** |

---

## Checklist

- [x] Asked clarifying questions with lettered options (N/A - design docs provided)
- [x] Incorporated design system from MASTER.md
- [x] User stories are small and specific (44 atomic stories)
- [x] Functional requirements are numbered and unambiguous
- [x] Non-goals section defines clear boundaries
- [x] Saved to `tasks/prd-tinyfusion-mvp.md`
