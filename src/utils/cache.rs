//! 缓存抽象（内存实现）：先用于 token 黑名单。
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

/// 默认容量上限：防公开端点（如验证码 generate）以随机 key 无限写入吃内存。
const DEFAULT_CAPACITY: usize = 100_000;

/// 内存实现：`DashMap<String, (value, expires_at)>`，读取时惰性检查过期并清理；
/// 写入满容量时驱逐（先清过期，仍满则驱逐最早过期条目），保证条目数有界。
#[derive(Clone)]
pub struct MemoryCache {
    inner: Arc<DashMap<String, (String, Instant)>>,
    capacity: usize,
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self {
            inner: Arc::default(),
            capacity: DEFAULT_CAPACITY,
        }
    }
}

impl MemoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 指定容量上限（测试与小内存场景用）；`capacity == 0` 时所有写入被丢弃。
    // 目前仅测试消费，非测试构建允许未使用
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::default(),
            capacity,
        }
    }

    /// 当前条目总数（测试断言与观测用）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// 是否为空。
    ///
    /// 与 [`Self::len`] 成对存在：拆出 lib target 后 `utils` 成为公开 API 面，
    /// 只有 `len` 没有 `is_empty` 会被 `clippy::len_without_is_empty` 判为
    /// 不完整的公开接口（CI 是 `-D warnings`，必须消掉）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 满容量时驱逐：先清已过期条目；清完仍满则驱逐 `expires_at` 最早的一条
    /// （≈ 最早写入）。只在满时触发，单次 O(n) 但 n 受容量上限约束。
    fn evict_if_full(&self) {
        if self.inner.len() < self.capacity {
            return;
        }

        let now = Instant::now();
        let expired: Vec<String> = self
            .inner
            .iter()
            .filter(|e| e.value().1 <= now)
            .map(|e| e.key().clone())
            .collect();
        for key in expired {
            self.inner.remove(&key);
        }
        if self.inner.len() < self.capacity {
            return;
        }

        if let Some(oldest) = self
            .inner
            .iter()
            .min_by_key(|e| e.value().1)
            .map(|e| e.key().clone())
        {
            self.inner.remove(&oldest);
        }
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
        if ttl <= Duration::ZERO || self.capacity == 0 {
            return;
        }

        self.evict_if_full();
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

    /// 容量上限：写满后再 set，条目总数不超上限（防公开端点无限写入吃内存）
    #[test]
    fn set_evicts_oldest_when_capacity_exceeded() {
        let cache = MemoryCache::new_with_capacity(3);
        for i in 0..3 {
            cache.set(&format!("k{i}"), "v".into(), Duration::from_secs(60));
        }
        cache.set("k3", "v".into(), Duration::from_secs(60));

        assert_eq!(cache.len(), 3, "条目数不得超容量上限");
        assert_eq!(cache.get("k0"), None, "最早写入的条目应被驱逐");
        assert_eq!(cache.get("k3").as_deref(), Some("v"), "最新写入必须保留");
    }

    /// 满容量时优先驱逐已过期条目，而不是误伤未过期的旧条目
    #[test]
    fn set_prefers_evicting_expired_entries_first() {
        let cache = MemoryCache::new_with_capacity(2);
        cache.set("expired", "v0".into(), Duration::from_millis(1));
        cache.set("alive", "v1".into(), Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(10));

        cache.set("new", "v2".into(), Duration::from_secs(60));

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get("expired"), None, "过期条目应先被清掉");
        assert_eq!(
            cache.get("alive").as_deref(),
            Some("v1"),
            "未过期条目不应被驱逐"
        );
        assert_eq!(cache.get("new").as_deref(), Some("v2"));
    }
}
