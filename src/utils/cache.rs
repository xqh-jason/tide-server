//! 缓存抽象（对照 GVA 的 `GVA_CACHE` 理念，文档 §1.7）：W2 先用于 token 黑名单。
//!
//! `Cache` trait 只暴露最小操作（get/set/remove），内存实现用 dashmap 惰性过期；
//! 后续若换 Redis，只需新增实现并替换注入点，业务代码不变。

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// 缓存 trait：所有操作对并发安全，值统一为 String（黑名单存 token 即可）。
pub trait Cache: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: String, ttl: Duration);
    fn remove(&self, key: &str);
    fn exists(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

/// 内存实现：`DashMap<String, (value, expires_at)>`，读取时惰性检查过期并清理。
#[derive(Clone, Default)]
pub struct MemoryCache {
    inner: Arc<DashMap<String, (String, Instant)>>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Cache for MemoryCache {
    fn get(&self, key: &str) -> Option<String> {
        let entry = self.inner.get(key)?;
        let (value, expires_at) = entry.value();
        if *expires_at <= Instant::now() {
            drop(entry);
            self.inner.remove(key);
            None
        } else {
            Some(value.clone())
        }
    }

    fn set(&self, key: &str, value: String, ttl: Duration) {
        if ttl <= Duration::ZERO {
            return;
        }

        self.inner
            .insert(key.to_string(), (value, Instant::now() + ttl));
    }

    fn remove(&self, key: &str) {
        self.inner.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_remove() {
        let cache = MemoryCache::new();
        cache.set("token:abc", "blacklisted".into(), Duration::from_secs(60));
        assert_eq!(cache.get("token:abc").as_deref(), Some("blacklisted"));
        assert!(cache.exists("token:abc"));
        cache.remove("token:abc");
        assert!(!cache.exists("token:abc"));
    }

    #[test]
    fn expired_entry_is_cleared() {
        let cache = MemoryCache::new();
        cache.set("token:exp", "blacklisted".into(), Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        assert!(!cache.exists("token:exp"));
    }

    #[test]
    fn missing_key_returns_none() {
        let cache = MemoryCache::new();
        assert_eq!(cache.get("nope"), None);
    }
}
