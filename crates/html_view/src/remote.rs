//! Remote resources (`http:`/`https:`), fetched only for documents that
//! allow them ([`crate::Resources::allow_remote`]).
//!
//! A small pool of background threads, started on the first remote request
//! (never at startup), fetches with conservative limits: no cookies, no
//! referrer, a timeout and a size cap. Bodies are kept in a bounded memory
//! cache so re-rendering a message (theme change, zoom) doesn't refetch.

use blitz_traits::net::{Bytes, NetHandler};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// Fetch threads (requests beyond these wait in the queue).
const WORKERS: usize = 4;
/// Per-request timeout.
const TIMEOUT: Duration = Duration::from_secs(15);
/// Largest body accepted.
const MAX_BODY: u64 = 15 << 20;
/// Memory kept by the body cache.
const CACHE_BYTES: usize = 64 << 20;

/// What a document tracks about its remote loads: how many are in flight and
/// whether one finished since the host last looked.
#[derive(Default, Debug)]
pub struct RemoteLoads {
    in_flight: AtomicUsize,
    arrived: AtomicBool,
}

impl RemoteLoads {
    /// Whether loads are still in flight.
    pub fn pending(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) > 0
    }

    /// Whether a load finished since the last call (cleared).
    pub fn take_arrived(&self) -> bool {
        self.arrived.swap(false, Ordering::AcqRel)
    }

    fn start(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
    }

    /// Whether a load finished since [`Self::take_arrived`] last cleared it.
    pub fn has_arrived(&self) -> bool {
        self.arrived.load(Ordering::Acquire)
    }

    fn finish(&self) {
        // In this order: seeing nothing pending then means the arrival is already marked.
        self.arrived.store(true, Ordering::Release);
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Job {
    url: url::Url,
    handler: Box<dyn NetHandler>,
    loads: Arc<RemoteLoads>,
}

/// Queues `url` for fetching; `handler` gets the body (or the error) on a
/// fetch thread and `loads` follows it.
pub(crate) fn fetch(url: url::Url, handler: Box<dyn NetHandler>, loads: Arc<RemoteLoads>) {
    if let Some(bytes) = cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(url.as_str()))
    {
        handler.bytes(url.to_string(), Bytes::from(bytes.to_vec()));
        loads.arrived.store(true, Ordering::Release);
        return;
    }
    loads.start();
    let job = Job {
        url,
        handler,
        loads,
    };
    let sent = queue().lock().map(|tx| tx.send(job));
    if let Ok(Err(std::sync::mpsc::SendError(job))) = sent {
        // The pool is gone (shouldn't happen): fail the load.
        job.handler
            .error(job.url.to_string(), "fetch pool unavailable".into());
        job.loads.finish();
    }
}

fn queue() -> &'static Mutex<Sender<Job>> {
    static QUEUE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for index in 0..WORKERS {
            let rx = rx.clone();
            let _ = std::thread::Builder::new()
                .name(format!("html_view-fetch-{index}"))
                .spawn(move || worker(rx));
        }
        Mutex::new(tx)
    })
}

/// Whether a message may make the app connect to `ip`: not to this machine or the
/// link-local range (cloud metadata). Private ranges stay reachable, for intranet
/// images in corporate mail (user decision).
fn is_reachable_from_mail(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => !(ip.is_loopback() || ip.is_link_local() || ip.octets()[0] == 0),
        IpAddr::V6(ip) => {
            !(ip.is_loopback() || ip.is_unspecified() || ip.segments()[0] & 0xffc0 == 0xfe80)
        }
    }
}

/// Resolves as usual, dropping what [`is_reachable_from_mail`] refuses: by address, as
/// a name or a redirect can lead to any of them. A proxy the user set up is exempt.
#[derive(Debug, Default)]
struct OffThisMachine(DefaultResolver);

impl Resolver for OffThisMachine {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let resolved = self.0.resolve(uri, config, timeout)?;
        let is_proxy = config
            .proxy()
            .is_some_and(|proxy| Some(proxy.host()) == uri.host());
        if is_proxy {
            return Ok(resolved);
        }
        let mut allowed = self.empty();
        for address in resolved.iter() {
            if is_reachable_from_mail(address.ip()) {
                allowed.push(*address);
            }
        }
        if allowed.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(allowed)
    }
}

fn worker(rx: Arc<Mutex<Receiver<Job>>>) {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent("Mozilla/5.0 (compatible; BGMail)")
        // Status codes are checked below; error bodies are not wanted.
        .http_status_as_error(true)
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        DefaultConnector::default(),
        OffThisMachine::default(),
    );
    loop {
        let job = match rx.lock().map(|rx| rx.recv()) {
            Ok(Ok(job)) => job,
            _ => return,
        };
        let url = job.url.to_string();
        match get(&agent, &url) {
            Ok(bytes) => {
                if let Ok(mut cache) = cache().lock() {
                    cache.insert(url.clone(), bytes.clone());
                }
                job.handler.bytes(url, Bytes::from(bytes.to_vec()));
            }
            Err(error) => job.handler.error(url, error),
        }
        job.loads.finish();
    }
}

fn get(agent: &ureq::Agent, url: &str) -> Result<Arc<[u8]>, String> {
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok(body.into())
}

/// Bodies by URL, oldest evicted first past [`CACHE_BYTES`].
#[derive(Default)]
struct BodyCache {
    bodies: HashMap<String, Arc<[u8]>>,
    order: VecDeque<String>,
    bytes: usize,
}

impl BodyCache {
    fn get(&self, url: &str) -> Option<Arc<[u8]>> {
        self.bodies.get(url).cloned()
    }

    fn insert(&mut self, url: String, body: Arc<[u8]>) {
        self.insert_capped(url, body, CACHE_BYTES);
    }

    fn insert_capped(&mut self, url: String, body: Arc<[u8]>, cap: usize) {
        if body.len() > cap || self.bodies.contains_key(&url) {
            return;
        }
        self.bytes += body.len();
        self.bodies.insert(url.clone(), body);
        self.order.push_back(url);
        while self.bytes > cap {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(body) = self.bodies.remove(&oldest) {
                self.bytes -= body.len();
            }
        }
    }
}

fn cache() -> &'static Mutex<BodyCache> {
    static CACHE: OnceLock<Mutex<BodyCache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

#[cfg(test)]
mod tests {
    use super::RemoteLoads;

    #[test]
    fn a_finished_load_shows_as_arrived_until_taken() {
        let loads = RemoteLoads::default();
        loads.start();
        assert!(loads.pending() && !loads.has_arrived());
        loads.finish();
        assert!(!loads.pending() && loads.has_arrived());
        assert!(loads.take_arrived());
        assert!(!loads.has_arrived());
    }

    use super::*;

    #[test]
    fn mail_cannot_reach_this_machine_or_the_link_local_range() {
        for blocked in [
            "127.0.0.1",
            "127.8.8.8",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "::",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
        ] {
            assert!(
                !is_reachable_from_mail(blocked.parse().unwrap()),
                "{blocked}"
            );
        }
        for allowed in [
            "8.8.8.8",
            "10.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            "2606:4700::1",
        ] {
            assert!(
                is_reachable_from_mail(allowed.parse().unwrap()),
                "{allowed}"
            );
        }
    }

    #[test]
    fn body_cache_evicts_oldest_past_its_cap() {
        let mut cache = BodyCache::default();
        let body = |n: usize| -> Arc<[u8]> { vec![0u8; n].into() };
        cache.insert_capped("a".into(), body(4), 10);
        cache.insert_capped("b".into(), body(4), 10);
        cache.insert_capped("c".into(), body(4), 10);
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_some() && cache.get("c").is_some());
        assert_eq!(cache.bytes, 8);
        // Bodies larger than the cache aren't kept.
        cache.insert_capped("big".into(), body(11), 10);
        assert!(cache.get("big").is_none());
    }

    #[test]
    fn loads_track_flight_and_arrival() {
        let loads = RemoteLoads::default();
        assert!(!loads.pending() && !loads.take_arrived());
        loads.start();
        assert!(loads.pending());
        loads.finish();
        assert!(!loads.pending());
        assert!(loads.take_arrived());
        assert!(!loads.take_arrived());
    }
}
