# Changelog

All notable changes to this project will be documented in this file.

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
