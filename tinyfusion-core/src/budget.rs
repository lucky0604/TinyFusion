use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::BudgetConfig;

/// Persistent budget state stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetState {
    /// Day key (YYYY-MM-DD) for daily budget tracking.
    pub day: String,
    /// Month key (YYYY-MM) for monthly budget tracking.
    pub month: String,
    /// Tokens consumed today.
    pub daily_tokens: u64,
    /// Tokens consumed this month.
    pub monthly_tokens: u64,
    /// Epoch seconds of last update.
    pub last_updated: u64,
}

impl Default for BudgetState {
    fn default() -> Self {
        let (day, month) = current_day_month();
        Self {
            day,
            month,
            daily_tokens: 0,
            monthly_tokens: 0,
            last_updated: now_secs(),
        }
    }
}

/// Read-only snapshot for passing into PipelineContext / logging.
#[derive(Debug, Clone, Serialize)]
pub struct BudgetSnapshot {
    pub daily_tokens: u64,
    pub daily_limit: u64,
    pub monthly_tokens: u64,
    pub monthly_limit: u64,
}

/// Thread-safe token budget manager with debounced disk persistence.
pub struct BudgetManager {
    config: BudgetConfig,
    state: Arc<Mutex<BudgetState>>,
    persist_path: PathBuf,
    last_persisted: Arc<Mutex<u64>>,
}

impl BudgetManager {
    pub fn new(config: BudgetConfig) -> Self {
        let persist_path = Self::default_path();
        let state = Self::load_or_default(&persist_path);

        Self {
            config,
            state: Arc::new(Mutex::new(state)),
            last_persisted: Arc::new(Mutex::new(now_secs())),
            persist_path,
        }
    }

    fn default_path() -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".tinyfusion")
            .join("budget.json")
    }

    fn load_or_default(path: &PathBuf) -> BudgetState {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                match serde_json::from_str::<BudgetState>(&content) {
                    Ok(mut state) => {
                        // Roll over if the day/month changed
                        let (today, this_month) = current_day_month();
                        if state.day != today {
                            state.daily_tokens = 0;
                            state.day = today;
                        }
                        if state.month != this_month {
                            state.monthly_tokens = 0;
                            state.month = this_month;
                        }
                        state
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse budget state, resetting: {}", e);
                        BudgetState::default()
                    }
                }
            }
            Err(_) => BudgetState::default(),
        }
    }

    /// Check if we can afford `estimated_tokens` within both daily and monthly limits.
    /// Returns true if either limit is 0 (unlimited) or usage is within bounds.
    pub fn can_afford(&self, estimated_tokens: u64) -> bool {
        let state = self.state.lock().unwrap();
        let daily_ok = self.config.daily_limit == 0
            || state.daily_tokens + estimated_tokens <= self.config.daily_limit;
        let monthly_ok = self.config.monthly_limit == 0
            || state.monthly_tokens + estimated_tokens <= self.config.monthly_limit;
        daily_ok && monthly_ok
    }

    /// Record token usage. `is_local` models are tracked but do not count against budget.
    pub fn record(&self, tokens: u64, is_local: bool) {
        if tokens == 0 {
            return;
        }

        let mut state = self.state.lock().unwrap();

        // Roll over if day/month changed
        let (today, this_month) = current_day_month();
        if state.day != today {
            state.daily_tokens = 0;
            state.day = today;
        }
        if state.month != this_month {
            state.monthly_tokens = 0;
            state.month = this_month;
        }

        if !is_local {
            state.daily_tokens += tokens;
            state.monthly_tokens += tokens;
        }
        state.last_updated = now_secs();
        drop(state);

        self.maybe_persist();
    }

    /// Get a snapshot of current budget state for logging / passing to pipeline.
    pub fn snapshot(&self) -> BudgetSnapshot {
        let state = self.state.lock().unwrap();
        BudgetSnapshot {
            daily_tokens: state.daily_tokens,
            daily_limit: self.config.daily_limit,
            monthly_tokens: state.monthly_tokens,
            monthly_limit: self.config.monthly_limit,
        }
    }

    /// Debounced persist: only writes to disk if enough time has elapsed.
    fn maybe_persist(&self) {
        let now = now_secs();
        let mut last = self.last_persisted.lock().unwrap();
        if now - *last < self.config.persist_interval_secs {
            return;
        }
        *last = now;
        drop(last);

        self.force_persist();
    }

    /// Force write budget state to disk. Logs warning on failure.
    pub fn force_persist(&self) {
        let state = self.state.lock().unwrap();
        let json = match serde_json::to_string_pretty(&*state) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("Failed to serialize budget state: {}", e);
                return;
            }
        };
        drop(state);

        if let Some(parent) = self.persist_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("Failed to create budget dir {}: {}", parent.display(), e);
                return;
            }
        }

        if let Err(e) = std::fs::write(&self.persist_path, &json) {
            tracing::warn!(
                "Failed to persist budget to {}: {}",
                self.persist_path.display(),
                e
            );
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn current_day_month() -> (String, String) {
    let secs = now_secs();
    // Simple date calculation without chrono dependency
    let days_since_epoch = secs / 86400;
    let (year, month, day) = days_to_ymd(days_since_epoch);
    (
        format!("{:04}-{:02}-{:02}", year, month, day),
        format!("{:04}-{:02}", year, month),
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BudgetConfig {
        BudgetConfig {
            daily_limit: 100_000,
            monthly_limit: 1_000_000,
            persist_interval_secs: 0,
        }
    }

    fn unlimited_config() -> BudgetConfig {
        BudgetConfig {
            daily_limit: 0,
            monthly_limit: 0,
            persist_interval_secs: 60,
        }
    }

    #[test]
    fn test_can_afford_within_limits() {
        let mgr = BudgetManager::new(test_config());
        assert!(mgr.can_afford(50_000));
    }

    #[test]
    fn test_can_afford_exceeds_daily() {
        let mgr = BudgetManager::new(test_config());
        mgr.record(90_000, false);
        assert!(!mgr.can_afford(20_000));
    }

    #[test]
    fn test_can_afford_exceeds_monthly() {
        let mgr = BudgetManager::new(BudgetConfig {
            daily_limit: 0,
            monthly_limit: 100_000,
            persist_interval_secs: 60,
        });
        mgr.record(90_000, false);
        assert!(!mgr.can_afford(20_000));
    }

    #[test]
    fn test_local_tokens_dont_count_against_budget() {
        let mgr = BudgetManager::new(test_config());
        mgr.record(200_000, true);
        assert!(mgr.can_afford(100_000));
    }

    #[test]
    fn test_unlimited_always_affords() {
        let mgr = BudgetManager::new(unlimited_config());
        mgr.record(999_999_999, false);
        assert!(mgr.can_afford(999_999_999));
    }

    #[test]
    fn test_snapshot() {
        let mgr = BudgetManager::new(test_config());
        mgr.record(5000, false);
        let snap = mgr.snapshot();
        assert_eq!(snap.daily_tokens, 5000);
        assert_eq!(snap.monthly_tokens, 5000);
        assert_eq!(snap.daily_limit, 100_000);
        assert_eq!(snap.monthly_limit, 1_000_000);
    }

    #[test]
    fn test_zero_tokens_ignored() {
        let mgr = BudgetManager::new(test_config());
        mgr.record(0, false);
        let snap = mgr.snapshot();
        assert_eq!(snap.daily_tokens, 0);
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2026-06-26 = day 20630 since epoch
        let days = 20630;
        let (y, m, d) = days_to_ymd(days);
        assert_eq!((y, m, d), (2026, 6, 26));
    }

    #[test]
    fn test_current_day_month_format() {
        let (day, month) = current_day_month();
        assert_eq!(day.len(), 10); // YYYY-MM-DD
        assert_eq!(month.len(), 7); // YYYY-MM
        assert!(day.starts_with(&month));
    }
}
