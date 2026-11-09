//! Rate limiting and quotas.
//!
//! * **Rate limit** — a token bucket per principal (API key or client IP):
//!   `burst` tokens, refilled at `rate` tokens/second. Answers "how fast".
//! * **Daily quota** — a counter per API key per UTC day. Answers "how much".
//!   Counters live in memory on the hot path and are flushed to SQLite every
//!   few seconds, so a restart loses at most one flush interval of usage.
//!
//! The bucket map is sharded to keep lock contention low under concurrency,
//! and idle buckets are swept periodically so memory stays bounded.

use std::{
    collections::HashMap,
    hash::{BuildHasher, RandomState},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use sqlx::SqlitePool;

const SHARDS: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Tokens added per second.
    pub rate: f64,
    /// Bucket size: the largest burst allowed.
    pub burst: f64,
}

impl Policy {
    pub const fn new(rate: f64, burst: f64) -> Self {
        Self { rate, burst }
    }
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

/// Outcome of a rate-limit check, used to fill the `RateLimit-*` headers.
#[derive(Debug, Clone, Copy)]
pub struct Decision {
    pub allowed: bool,
    pub limit: u64,
    pub remaining: u64,
    /// Seconds until at least one token is available again.
    pub reset_secs: u64,
}

pub struct RateLimiter {
    shards: Vec<Mutex<HashMap<String, Bucket>>>,
    hasher: RandomState,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self {
            shards: (0..SHARDS).map(|_| Mutex::new(HashMap::new())).collect(),
            hasher: RandomState::new(),
        }
    }
}

impl RateLimiter {
    pub fn check(&self, key: &str, policy: Policy) -> Decision {
        self.check_at(key, policy, Instant::now())
    }

    fn check_at(&self, key: &str, policy: Policy, now: Instant) -> Decision {
        let shard = &self.shards[(self.hasher.hash_one(key) as usize) % SHARDS];
        let mut map = shard.lock().expect("rate limiter lock poisoned");
        let bucket = map.entry(key.to_owned()).or_insert(Bucket {
            tokens: policy.burst,
            last: now,
        });
        let elapsed = now.saturating_duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * policy.rate).min(policy.burst);
        bucket.last = now;

        let allowed = bucket.tokens >= 1.0;
        if allowed {
            bucket.tokens -= 1.0;
        }
        let missing = (1.0 - bucket.tokens).max(0.0);
        Decision {
            allowed,
            limit: policy.burst as u64,
            remaining: bucket.tokens.floor() as u64,
            reset_secs: if policy.rate > 0.0 {
                (missing / policy.rate).ceil() as u64
            } else {
                u64::MAX
            },
        }
    }

    /// Drop buckets that have been idle long enough to be full again.
    pub fn sweep(&self, idle: Duration) -> usize {
        let now = Instant::now();
        let mut removed = 0;
        for shard in &self.shards {
            let mut map = shard.lock().expect("rate limiter lock poisoned");
            let before = map.len();
            map.retain(|_, b| now.saturating_duration_since(b.last) < idle);
            removed += before - map.len();
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().expect("rate limiter lock poisoned").len())
            .sum()
    }
}

/// UTC day number (days since the Unix epoch).
pub fn utc_day() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp().div_euclid(86_400)
}

pub fn secs_until_utc_midnight() -> u64 {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    (86_400 - now.rem_euclid(86_400)) as u64
}

#[derive(Debug)]
struct DayCounter {
    day: i64,
    /// Total used today, including what was loaded from the database.
    used: AtomicU64,
    /// Not yet written to the database.
    unflushed: AtomicU64,
}

/// Per-key daily usage counters.
#[derive(Default)]
pub struct DailyQuotas {
    counters: Mutex<HashMap<i64, DayCounter>>,
}

impl DailyQuotas {
    /// Count `n` units against `key_id`'s quota. Returns the new total, or
    /// `Err(total)` without counting when that would exceed `limit`.
    pub fn try_consume(&self, key_id: i64, n: u64, limit: Option<u64>) -> Result<u64, u64> {
        let today = utc_day();
        let mut map = self.counters.lock().expect("quota lock poisoned");
        let c = map.entry(key_id).or_insert_with(|| DayCounter {
            day: today,
            used: AtomicU64::new(0),
            unflushed: AtomicU64::new(0),
        });
        if c.day != today {
            *c = DayCounter {
                day: today,
                used: AtomicU64::new(0),
                unflushed: AtomicU64::new(c.unflushed.load(Ordering::Relaxed)),
            };
        }
        let used = c.used.load(Ordering::Relaxed);
        if let Some(limit) = limit
            && used + n > limit
        {
            return Err(used);
        }
        c.used.fetch_add(n, Ordering::Relaxed);
        c.unflushed.fetch_add(n, Ordering::Relaxed);
        Ok(used + n)
    }

    pub fn used_today(&self, key_id: i64) -> u64 {
        let map = self.counters.lock().expect("quota lock poisoned");
        map.get(&key_id)
            .filter(|c| c.day == utc_day())
            .map(|c| c.used.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Seed today's counters from the database at startup.
    pub async fn load(&self, db: &SqlitePool) -> sqlx::Result<()> {
        let today = utc_day();
        let rows: Vec<(i64, i64)> =
            sqlx::query_as("SELECT key_id, units FROM api_usage WHERE day = ?")
                .bind(today)
                .fetch_all(db)
                .await?;
        let mut map = self.counters.lock().expect("quota lock poisoned");
        for (key_id, units) in rows {
            map.insert(
                key_id,
                DayCounter {
                    day: today,
                    used: AtomicU64::new(units as u64),
                    unflushed: AtomicU64::new(0),
                },
            );
        }
        Ok(())
    }

    /// Persist the deltas accumulated since the last flush.
    pub async fn flush(&self, db: &SqlitePool) -> sqlx::Result<()> {
        let deltas: Vec<(i64, i64, u64)> = {
            let map = self.counters.lock().expect("quota lock poisoned");
            map.iter()
                .filter_map(|(k, c)| {
                    let n = c.unflushed.swap(0, Ordering::Relaxed);
                    (n > 0).then_some((*k, c.day, n))
                })
                .collect()
        };
        if deltas.is_empty() {
            return Ok(());
        }
        let mut tx = db.begin().await?;
        for (key_id, day, n) in deltas {
            sqlx::query(
                "INSERT INTO api_usage (key_id, day, units) VALUES (?, ?, ?)
                 ON CONFLICT (key_id, day) DO UPDATE SET units = units + excluded.units",
            )
            .bind(key_id)
            .bind(day)
            .bind(n as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_burst_then_refills() {
        let rl = RateLimiter::default();
        let p = Policy::new(2.0, 3.0);
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(rl.check_at("k", p, t0).allowed);
        }
        let denied = rl.check_at("k", p, t0);
        assert!(!denied.allowed);
        assert_eq!(denied.reset_secs, 1);
        // 0.5 s at 2 tokens/s refills one token.
        assert!(rl.check_at("k", p, t0 + Duration::from_millis(500)).allowed);
        assert!(!rl.check_at("k", p, t0 + Duration::from_millis(500)).allowed);
    }

    #[test]
    fn buckets_are_independent_per_key() {
        let rl = RateLimiter::default();
        let p = Policy::new(1.0, 1.0);
        assert!(rl.check("a", p).allowed);
        assert!(rl.check("b", p).allowed);
        assert!(!rl.check("a", p).allowed);
    }

    #[test]
    fn daily_quota_is_all_or_nothing() {
        let q = DailyQuotas::default();
        assert_eq!(q.try_consume(1, 8, Some(10)), Ok(8));
        assert_eq!(q.try_consume(1, 3, Some(10)), Err(8));
        assert_eq!(q.try_consume(1, 2, Some(10)), Ok(10));
        assert_eq!(q.try_consume(2, 1_000, None), Ok(1_000));
    }
}
