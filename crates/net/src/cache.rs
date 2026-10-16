//! A small in-memory HTTP cache for subresources (stylesheets, images).
//!
//! Deliberately conservative (RFC 9111 subset): only `200` responses with an
//! explicit `Cache-Control: max-age` > 0 and none of `no-store`, `no-cache`
//! or `private` are stored; there is no heuristic freshness, no
//! revalidation, and documents are never cached. Entries expire at their
//! max-age, and the total body size stays under a byte budget by evicting
//! the least recently used entries.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::{Destination, Response};

/// Default budget for cached bodies.
pub const DEFAULT_CACHE_BYTES: usize = 32 * 1024 * 1024;

/// The freshness lifetime a response may be cached for, from its
/// `Cache-Control` header value; `None` if it must not be stored.
pub fn freshness(cache_control: Option<&str>) -> Option<Duration> {
    let mut max_age = None;
    for directive in cache_control?.split(',') {
        let directive = directive.trim();
        let (name, value) = match directive.split_once('=') {
            Some((n, v)) => (n.trim(), Some(v.trim().trim_matches('"'))),
            None => (directive, None),
        };
        match name.to_ascii_lowercase().as_str() {
            "no-store" | "no-cache" | "private" => return None,
            "max-age" => max_age = value.and_then(|v| v.parse::<u64>().ok()),
            _ => {}
        }
    }
    max_age.filter(|&s| s > 0).map(Duration::from_secs)
}

#[derive(Debug)]
struct Entry {
    response: Response,
    expires: Instant,
    last_used: u64,
}

/// Counters for diagnostics and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

#[derive(Debug)]
pub struct HttpCache {
    entries: HashMap<(String, Destination), Entry>,
    budget: usize,
    clock: u64,
    stats: CacheStats,
}

impl HttpCache {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            budget,
            clock: 0,
            stats: CacheStats::default(),
        }
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// A fresh cached response for `url`, if any.
    pub fn get(&mut self, url: &str, destination: Destination, now: Instant) -> Option<Response> {
        let key = (url.to_owned(), destination);
        let fresh = self.entries.get(&key).is_some_and(|e| e.expires > now);
        if !fresh {
            if let Some(stale) = self.entries.remove(&key) {
                self.stats.bytes -= stale.response.body.len();
                self.stats.entries -= 1;
            }
            self.stats.misses += 1;
            return None;
        }
        self.clock += 1;
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = self.clock;
        self.stats.hits += 1;
        Some(entry.response.clone())
    }

    /// Stores `response` (fetched for `url`) if it is cacheable and small
    /// enough, evicting least recently used entries to stay in budget.
    pub fn put(&mut self, url: &str, destination: Destination, response: &Response, now: Instant) {
        if destination == Destination::Document || response.status != 200 {
            return;
        }
        let Some(lifetime) = response.cache_lifetime else {
            return;
        };
        let size = response.body.len();
        // One huge image must not flush everything else.
        if size > self.budget / 4 {
            return;
        }
        let key = (url.to_owned(), destination);
        if let Some(old) = self.entries.remove(&key) {
            self.stats.bytes -= old.response.body.len();
            self.stats.entries -= 1;
        }
        while self.stats.bytes + size > self.budget {
            let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(e) = self.entries.remove(&victim) {
                self.stats.bytes -= e.response.body.len();
                self.stats.entries -= 1;
                self.stats.evictions += 1;
            }
        }
        self.clock += 1;
        self.entries.insert(
            key,
            Entry {
                response: response.clone(),
                expires: now + lifetime,
                last_used: self.clock,
            },
        );
        self.stats.bytes += size;
        self.stats.entries += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(bytes: usize, lifetime: Option<u64>) -> Response {
        Response {
            url: crate::Url::parse("http://a.test/x").unwrap(),
            status: 200,
            mime: None,
            charset: None,
            body: vec![0; bytes],
            redirects: Vec::new(),
            cache_lifetime: lifetime.map(Duration::from_secs),
        }
    }

    #[test]
    fn cache_control_parsing() {
        assert_eq!(freshness(Some("max-age=60")), Some(Duration::from_secs(60)));
        assert_eq!(
            freshness(Some("public, MAX-AGE=\"3600\", immutable")),
            Some(Duration::from_secs(3600))
        );
        assert_eq!(freshness(Some("max-age=60, no-store")), None);
        assert_eq!(freshness(Some("no-cache, max-age=60")), None);
        assert_eq!(freshness(Some("private, max-age=60")), None);
        assert_eq!(freshness(Some("max-age=0")), None);
        assert_eq!(freshness(Some("public")), None, "no heuristic freshness");
        assert_eq!(freshness(None), None);
    }

    #[test]
    fn hits_expire_and_documents_are_never_stored() {
        let now = Instant::now();
        let mut c = HttpCache::new(1000);
        c.put("u", Destination::Image, &response(10, Some(60)), now);
        assert!(c.get("u", Destination::Image, now).is_some());
        assert!(
            c.get("u", Destination::Style, now).is_none(),
            "keyed by destination"
        );
        assert!(
            c.get("u", Destination::Image, now + Duration::from_secs(61))
                .is_none()
        );
        assert_eq!(c.stats().entries, 0, "expired entries are dropped");
        c.put("d", Destination::Document, &response(10, Some(60)), now);
        c.put("n", Destination::Image, &response(10, None), now);
        assert_eq!(c.stats().entries, 0);
    }

    #[test]
    fn budget_evicts_least_recently_used() {
        let now = Instant::now();
        let mut c = HttpCache::new(900);
        c.put("a", Destination::Image, &response(200, Some(60)), now);
        c.put("b", Destination::Image, &response(200, Some(60)), now);
        c.put("c", Destination::Image, &response(200, Some(60)), now);
        c.get("a", Destination::Image, now);
        c.put("d", Destination::Image, &response(200, Some(60)), now);
        c.put("e", Destination::Image, &response(200, Some(60)), now);
        assert!(c.stats().bytes <= 900);
        assert_eq!(c.stats().evictions, 1);
        assert!(
            c.get("b", Destination::Image, now).is_none(),
            "b was least recent"
        );
        assert!(c.get("a", Destination::Image, now).is_some());
        c.put("big", Destination::Image, &response(300, Some(60)), now);
        assert!(
            c.get("big", Destination::Image, now).is_none(),
            "over a quarter of the budget"
        );
    }
}
