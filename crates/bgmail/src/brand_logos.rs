//! Brand logos in the reader's avatar (BIMI), found off the UI thread.
//!
//! The avatar shows the sender's initial at once. Opening a message asks a
//! worker thread whether it has a verified logo (`protocols::bimi`): DKIM and
//! DMARC on the message, then the domain's mark certificate, cached per domain
//! in `brand_logos`. Only when all of that passes does the logo replace the
//! initial. The worker starts on the first request, so it costs nothing at
//! startup, and one domain known to have no logo costs no network at all.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use protocols::bimi::BrandChecker;
use storage::{BrandLogo, Database};
use ui::SvgLogo;

/// How long a logo found stays good before it is checked again.
const LOGO_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// How long "this domain has no logo" stays good.
const NONE_KEPT: Duration = Duration::from_secs(24 * 60 * 60);

/// What the worker found for a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Finding {
    /// Its body isn't downloaded yet: nothing to check against.
    NotReady,
    /// No logo to show (not authenticated, or the domain has none).
    None,
    /// The verified logo of `domain`.
    Logo { domain: String, svg: Vec<u8> },
}

struct Request {
    message_id: i64,
    domain: String,
}

/// The reader's brand logos: what was found per message, and the worker.
pub(crate) struct BrandLogos {
    db_path: PathBuf,
    requests: Option<mpsc::Sender<Request>>,
    /// Per message: the domain whose logo it shows, or `None` for none.
    shown: HashMap<i64, Option<String>>,
    pending: HashSet<i64>,
    /// Asked about before their body was downloaded: asked again on `body_ready`.
    awaiting_body: HashSet<i64>,
    images: HashMap<String, SvgLogo>,
}

impl BrandLogos {
    pub(crate) fn new(db_path: PathBuf) -> Self {
        Self {
            db_path,
            requests: None,
            shown: HashMap::new(),
            pending: HashSet::new(),
            awaiting_body: HashSet::new(),
            images: HashMap::new(),
        }
    }

    /// The logo to show for `message_id`, once found.
    pub(crate) fn logo(&self, message_id: i64) -> Option<SvgLogo> {
        let domain = self.shown.get(&message_id)?.as_ref()?;
        self.images.get(domain).cloned()
    }

    /// Whether `message_id` still has to be asked about. Cheap: the reader asks
    /// on every render.
    pub(crate) fn needs_request(&self, message_id: i64) -> bool {
        !self.shown.contains_key(&message_id)
            && !self.pending.contains(&message_id)
            && !self.awaiting_body.contains(&message_id)
    }

    /// The body of `message_id` arrived: it can be checked now.
    pub(crate) fn body_ready(&mut self, message_id: i64) {
        self.awaiting_body.remove(&message_id);
    }

    /// Asks the worker about a message from `sender_email`, starting the worker
    /// (and its result channel, through `start`) the first time. Returns whether
    /// a request went out.
    pub(crate) fn request(
        &mut self,
        message_id: i64,
        sender_email: &str,
        start: impl FnOnce(async_channel::Receiver<(i64, Finding)>),
    ) -> bool {
        if !self.needs_request(message_id) {
            return false;
        }
        let Some(domain) = sender_domain(sender_email) else {
            self.shown.insert(message_id, None);
            return false;
        };
        let requests = self.requests.get_or_insert_with(|| {
            let (results, incoming) = async_channel::unbounded();
            start(incoming);
            spawn_worker(self.db_path.clone(), results)
        });
        if requests.send(Request { message_id, domain }).is_err() {
            return false;
        }
        self.pending.insert(message_id);
        true
    }

    /// Takes in what the worker found. Returns whether what shows may change.
    pub(crate) fn receive(&mut self, message_id: i64, finding: Finding) -> bool {
        self.pending.remove(&message_id);
        match finding {
            Finding::NotReady => {
                self.awaiting_body.insert(message_id);
                false
            }
            Finding::None => {
                self.shown.insert(message_id, None);
                false
            }
            Finding::Logo { domain, svg } => {
                self.images
                    .entry(domain.clone())
                    .or_insert_with(|| SvgLogo::new(svg));
                self.shown.insert(message_id, Some(domain));
                true
            }
        }
    }
}

/// The domain of a sender's address, lower-cased.
fn sender_domain(email: &str) -> Option<String> {
    let (_, domain) = email.trim().rsplit_once('@')?;
    let domain = domain.trim_end_matches('>').trim().to_ascii_lowercase();
    (!domain.is_empty() && domain.contains('.')).then_some(domain)
}

/// Whether a cached finding is still good at `now` (Unix seconds).
fn is_fresh(cached: &BrandLogo, now: i64) -> bool {
    let kept = if cached.svg.is_some() {
        LOGO_KEPT
    } else {
        NONE_KEPT
    };
    now.saturating_sub(cached.checked_at) < kept.as_secs() as i64
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// One thread, its own database connection and resolver, answering requests in
/// order. It ends when the view drops its sender.
fn spawn_worker(
    db_path: PathBuf,
    results: async_channel::Sender<(i64, Finding)>,
) -> mpsc::Sender<Request> {
    let (requests, incoming) = mpsc::channel::<Request>();
    let spawned = std::thread::Builder::new()
        .name("bgmail-brand-logos".into())
        .spawn(move || {
            let Ok(db) = Database::open(&db_path) else {
                return;
            };
            // Built on the first message that needs the network.
            let mut checker: Option<Option<BrandChecker>> = None;
            for request in incoming {
                let finding = check(&db, &mut checker, &request);
                if results
                    .send_blocking((request.message_id, finding))
                    .is_err()
                {
                    return;
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("BGMail: brand logos unavailable: {error}");
    }
    requests
}

fn check(db: &Database, checker: &mut Option<Option<BrandChecker>>, request: &Request) -> Finding {
    let now = unix_now();
    let cached = db.brand_logo(&request.domain).ok().flatten();
    // A domain known to have no logo needs no authentication of the message.
    if cached
        .as_ref()
        .is_some_and(|cached| cached.svg.is_none() && is_fresh(cached, now))
    {
        return Finding::None;
    }
    let Some(raw) = db.message_raw(request.message_id).ok().flatten() else {
        return Finding::NotReady;
    };
    let Some(checker) = checker
        .get_or_insert_with(|| BrandChecker::new().ok())
        .as_ref()
    else {
        return Finding::None;
    };
    // The logo is only ever shown for the domain the message proves it is from.
    let Some(domain) = checker
        .authenticated_domain(&raw)
        .filter(|domain| *domain == request.domain)
    else {
        return Finding::None;
    };
    let svg = match cached {
        Some(cached) if is_fresh(&cached, now) => cached.svg,
        stale => match checker.logo(&domain, SystemTime::now()) {
            Ok(svg) => {
                db.set_brand_logo(&domain, svg.as_deref(), now).ok();
                svg
            }
            // Offline or the authority is down: what was known before still holds.
            Err(_) => stale.and_then(|cached| cached.svg),
        },
    };
    match svg {
        Some(svg) => Finding::Logo { domain, svg },
        None => Finding::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_sender_domain() {
        assert_eq!(
            sender_domain("Todo@Nubank.com.BR").as_deref(),
            Some("nubank.com.br")
        );
        assert_eq!(sender_domain("x@localhost"), None);
        assert_eq!(sender_domain("no-address"), None);
    }

    #[test]
    fn a_logo_is_kept_longer_than_its_absence() {
        let found = BrandLogo {
            svg: Some(b"<svg/>".to_vec()),
            checked_at: 0,
        };
        let none = BrandLogo {
            svg: None,
            checked_at: 0,
        };
        let day = 24 * 60 * 60;
        assert!(is_fresh(&found, 6 * day));
        assert!(!is_fresh(&found, 7 * day));
        assert!(is_fresh(&none, day - 1));
        assert!(!is_fresh(&none, day));
    }

    fn logos() -> BrandLogos {
        BrandLogos::new(PathBuf::from("/nonexistent/mail.db"))
    }

    #[test]
    fn a_message_is_asked_about_once_and_shows_what_was_found() {
        let mut logos = logos();
        let mut started = 0;
        assert!(logos.request(1, "a@nubank.com.br", |_| started += 1));
        // Pending: not asked again, and the worker started only once.
        assert!(!logos.request(1, "a@nubank.com.br", |_| started += 1));
        assert!(logos.request(2, "b@nubank.com.br", |_| started += 1));
        assert_eq!(started, 1);
        assert!(logos.logo(1).is_none());

        let svg = b"<svg/>".to_vec();
        let found = Finding::Logo {
            domain: "nubank.com.br".into(),
            svg,
        };
        assert!(logos.receive(1, found));
        assert!(logos.logo(1).is_some());
        assert!(!logos.needs_request(1));

        // None found: settled, nothing shows.
        assert!(!logos.receive(2, Finding::None));
        assert!(logos.logo(2).is_none());
        assert!(!logos.needs_request(2));
    }

    #[test]
    fn a_message_without_its_body_is_asked_about_again_once_it_arrives() {
        let mut logos = logos();
        assert!(logos.request(3, "a@x.com", |_| {}));
        assert!(!logos.receive(3, Finding::NotReady));
        // Not on every render meanwhile.
        assert!(!logos.needs_request(3));
        logos.body_ready(3);
        assert!(logos.needs_request(3));
    }

    #[test]
    fn a_sender_without_a_domain_is_settled_at_once() {
        let mut logos = logos();
        assert!(!logos.request(4, "nobody", |_| panic!("no worker for it")));
        assert!(!logos.needs_request(4));
        assert!(logos.logo(4).is_none());
    }

    #[test]
    fn a_cached_absence_skips_the_message_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("mail.db")).unwrap();
        db.set_brand_logo("airbnb.com", None, unix_now()).unwrap();
        let mut checker = None;
        let request = Request {
            message_id: 99,
            domain: "airbnb.com".into(),
        };
        assert_eq!(check(&db, &mut checker, &request), Finding::None);
        // No resolver was needed for it.
        assert!(checker.is_none());
        // Unknown domain, message not downloaded: nothing to check yet.
        let request = Request {
            message_id: 99,
            domain: "nubank.com.br".into(),
        };
        assert_eq!(check(&db, &mut checker, &request), Finding::NotReady);
        assert!(checker.is_none());
    }
}
