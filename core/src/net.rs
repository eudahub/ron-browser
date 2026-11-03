//! Reading from the network, on threads of its own. It sends as little as it can: no cookies, no
//! `Referer` and nothing that tells one visit from another. All of it follows the same rules:
//! only `http` and `https`, a redirect never from `https` to `http`, limits on size and time, and
//! a page on the network reaches no addresses inside the user's own network, as otherwise any page
//! could poke at printers and routers from the user's computer.

use std::collections::VecDeque;
use std::fmt;
use std::io::Read;
use std::net::IpAddr;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use ureq::Agent;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use url::{Host, Url};

use crate::images::{DecodedImage, decode};
use crate::sheets::{SheetRequest, decode_css};

/// Larger pages are not read to the end, also when they only get this large after unpacking.
pub const MAX_PAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_REDIRECTS: usize = 10;
/// How long one fetch may take, with all of its redirects counted apart.
const TIMEOUT: Duration = Duration::from_secs(60);
const USER_AGENT: &str = concat!("ron-browser/", env!("CARGO_PKG_VERSION"));
const MAX_SHEET_BYTES: u64 = 16 * 1024 * 1024;
const PAGE_ACCEPT: &str = "text/html,application/xhtml+xml;q=0.9,*/*;q=0.1";
/// A page comes in pieces of at most this size, of which only so many wait for being taken.
const PIECE_BYTES: usize = 64 * 1024;
const PIECES_WAITING: usize = 16;
/// How many images or style sheets are read at once.
const WORKER_THREADS: usize = 4;
/// How many images of a page are read.
const MAX_IMAGES: usize = 2000;

/// Who asks for an address, which decides what it may reach.
#[derive(Clone, Copy, Debug)]
pub enum Asker<'a> {
    /// The user, by typing it or by going back to it.
    User,
    /// The page at this address.
    Page(&'a str),
    /// A local page, for what it needs without the user having chosen it. The user trusts such a
    /// page only as far as opening it, and it may have come from anywhere.
    LocalPage,
}

/// What came from an address.
#[derive(Clone, Debug, PartialEq)]
pub struct Fetched {
    /// Where it really came from, after any redirects.
    pub address: String,
    pub bytes: Vec<u8>,
    /// The type the server gives it, such as `text/html`, in lowercase; empty when it gives none.
    pub mime: String,
    /// The charset the server gives it, or empty.
    pub charset: String,
}

/// Why nothing came from an address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FetchError {
    /// It breaks the rules, whatever the server would do.
    Refused(String),
    /// The server answered with this status.
    Status(u16),
    TooLarge,
    Failed(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(reason) => write!(formatter, "not allowed: {reason}"),
            Self::Status(status) => write!(formatter, "the server answered {status}"),
            Self::TooLarge => write!(formatter, "too large"),
            Self::Failed(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Addresses that are not on the public internet: this computer, the local network and the like.
#[must_use]
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [first, second, ..] = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                // Shared by providers, and not reachable from outside.
                || (first == 100 && (64..128).contains(&second))
        }
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            ip.to_ipv4_mapped()
                .is_some_and(|mapped| is_private_ip(IpAddr::V4(mapped)))
                || ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                // Unique local and link local.
                || (first & 0xFE00) == 0xFC00
                || (first & 0xFFC0) == 0xFE80
        }
    }
}

/// Only gives addresses on the public internet. This is checked on what a name really resolves to,
/// as a public name can lead anywhere.
#[derive(Debug, Default)]
struct PublicResolver(DefaultResolver);

impl Resolver for PublicResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let found = self.0.resolve(uri, config, timeout)?;
        let mut result = self.empty();
        for address in found.iter().filter(|address| !is_private_ip(address.ip())) {
            result.push(*address);
        }
        if result.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(result)
    }
}

fn config() -> Config {
    Agent::config_builder()
        // Each step of a redirect goes through the rules here.
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .user_agent(USER_AGENT)
        .build()
}

fn agent(public_only: bool) -> &'static Agent {
    static ANYWHERE: OnceLock<Agent> = OnceLock::new();
    static PUBLIC: OnceLock<Agent> = OnceLock::new();
    if public_only {
        PUBLIC.get_or_init(|| {
            Agent::with_parts(
                config(),
                DefaultConnector::default(),
                PublicResolver::default(),
            )
        })
    } else {
        ANYWHERE.get_or_init(|| config().new_agent())
    }
}

/// Whether what the page asks for must stay on the public internet. Its own server is always fine,
/// as a page from inside a network belongs there.
fn must_be_public(asker: Asker<'_>, url: &Url) -> bool {
    match asker {
        Asker::User => false,
        Asker::LocalPage => true,
        Asker::Page(page) => Url::parse(page).is_ok_and(|page| {
            matches!(page.scheme(), "http" | "https") && page.host() != url.host()
        }),
    }
}

/// Whether the address names a place inside the user's network without having to look it up.
fn names_private_place(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => is_private_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_private_ip(IpAddr::V6(ip)),
        Some(Host::Domain(name)) => {
            name.eq_ignore_ascii_case("localhost")
                || name.to_ascii_lowercase().ends_with(".localhost")
        }
        None => false,
    }
}

/// Whether a redirect may go there: only `https` from `https`, and only to the web.
#[must_use]
pub fn may_redirect(from: &Url, to: &Url) -> bool {
    match to.scheme() {
        "https" => true,
        "http" => from.scheme() == "http",
        _ => false,
    }
}

fn check_rules(asker: Asker<'_>, url: &Url) -> Result<bool, FetchError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::Refused(format!("{} addresses", url.scheme())));
    }
    let public_only = must_be_public(asker, url);
    if public_only && names_private_place(url) {
        return Err(FetchError::Refused(
            "a page on the network reaching a local address".to_owned(),
        ));
    }
    Ok(public_only)
}

/// Reads an address, following redirects. `accept` tells the server what is wanted.
fn fetch_limited(
    address: &str,
    asker: Asker<'_>,
    accept: &str,
    max_bytes: u64,
) -> Result<Fetched, FetchError> {
    let mut opened = open_address(address, asker, accept, max_bytes)?;
    // The limit of the library counts packed bytes, while a few kilobytes can unpack to anything.
    // This counts what really comes out.
    let mut bytes = Vec::new();
    opened
        .response
        .body_mut()
        .as_reader()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| FetchError::Failed(error.to_string()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(FetchError::TooLarge);
    }
    Ok(Fetched {
        address: opened.address,
        bytes,
        mime: opened.mime,
        charset: opened.charset,
    })
}

/// An address that has answered, with its content still to be read.
struct Opened {
    address: String,
    mime: String,
    charset: String,
    response: ureq::http::Response<ureq::Body>,
}

/// Asks for an address, following redirects, as far as the start of its content.
fn open_address(
    address: &str,
    asker: Asker<'_>,
    accept: &str,
    max_bytes: u64,
) -> Result<Opened, FetchError> {
    let mut url = Url::parse(address).map_err(|error| FetchError::Failed(error.to_string()))?;
    url.set_fragment(None);
    for _ in 0..=MAX_REDIRECTS {
        let public_only = check_rules(asker, &url)?;
        let response = agent(public_only)
            .get(url.as_str())
            .header("Accept", accept)
            .call()
            .map_err(|error| FetchError::Failed(error.to_string()))?;
        let status = response.status();
        if status.is_redirection()
            && let Some(location) = response.headers().get("location")
        {
            let location = location
                .to_str()
                .map_err(|error| FetchError::Failed(error.to_string()))?;
            let mut next = url
                .join(location)
                .map_err(|error| FetchError::Failed(error.to_string()))?;
            next.set_fragment(None);
            if !may_redirect(&url, &next) {
                return Err(FetchError::Refused(format!(
                    "a redirect from {} to {}",
                    url.scheme(),
                    next.scheme()
                )));
            }
            url = next;
            continue;
        }
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        // What says at once that it is too large need not be read at all.
        let body = response.body();
        if body
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(FetchError::TooLarge);
        }
        let mime = body.mime_type().unwrap_or_default().to_ascii_lowercase();
        let charset = body.charset().unwrap_or_default().to_owned();
        return Ok(Opened {
            address: url.into(),
            mime,
            charset,
            response,
        });
    }
    Err(FetchError::Failed("too many redirects".to_owned()))
}

/// Reads a page. This blocks, so it is for threads other than that of the window.
///
/// # Errors
/// Tells what went wrong or what the rules refuse.
pub fn fetch_page(address: &str, asker: Asker<'_>) -> Result<Fetched, FetchError> {
    fetch_limited(address, asker, PAGE_ACCEPT, MAX_PAGE_BYTES)
}

/// A page being read on a thread of its own. Dropping this lets go of the result.
pub struct PageFetch {
    receiver: Receiver<PageEvent>,
    ended: bool,
}

/// What has happened with a page being read.
#[derive(Debug, PartialEq)]
pub enum PageEvent {
    /// The server has answered, and this is what it says of the page.
    Started {
        address: String,
        mime: String,
        charset: String,
    },
    /// The next piece of the page.
    Data(Vec<u8>),
    /// All of it has come.
    End,
    /// It ends here, whether or not anything has come.
    Failed(FetchError),
}

impl PageFetch {
    /// Starts reading a page. Pieces wait for being taken, so a fast server cannot fill the memory
    /// with more than a few of them.
    #[must_use]
    pub fn start(address: &str, asking_page: Option<&str>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(PIECES_WAITING);
        let address = address.to_owned();
        let asking_page = asking_page.map(str::to_owned);
        thread::spawn(move || {
            let asker = asking_page.as_deref().map_or(Asker::User, Asker::Page);
            let event = read_in_pieces(&address, asker, &sender)
                .map_or_else(PageEvent::Failed, |()| PageEvent::End);
            let _ = sender.send(event);
        });
        Self {
            receiver,
            ended: false,
        }
    }

    /// The next thing that has happened, if any.
    pub fn poll(&mut self) -> Option<PageEvent> {
        if self.ended {
            return None;
        }
        let event = match self.receiver.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                PageEvent::Failed(FetchError::Failed("the fetch stopped".to_owned()))
            }
        };
        self.ended = matches!(event, PageEvent::End | PageEvent::Failed(_));
        Some(event)
    }
}

/// Sends a page along as it comes. Stops by itself once nobody wants it.
fn read_in_pieces(
    address: &str,
    asker: Asker<'_>,
    sender: &mpsc::SyncSender<PageEvent>,
) -> Result<(), FetchError> {
    let mut opened = open_address(address, asker, PAGE_ACCEPT, MAX_PAGE_BYTES)?;
    let started = PageEvent::Started {
        address: opened.address,
        mime: opened.mime,
        charset: opened.charset,
    };
    if sender.send(started).is_err() {
        return Ok(());
    }
    let mut reader = opened.response.body_mut().as_reader();
    let mut total = 0_u64;
    loop {
        let mut piece = vec![0; PIECE_BYTES];
        let count = reader
            .read(&mut piece)
            .map_err(|error| FetchError::Failed(error.to_string()))?;
        if count == 0 {
            return Ok(());
        }
        total += count as u64;
        if total > MAX_PAGE_BYTES {
            return Err(FetchError::TooLarge);
        }
        piece.truncate(count);
        if sender.send(PageEvent::Data(piece)).is_err() {
            return Ok(());
        }
    }
}

/// An image to read, by how the page names it and where it really is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageRequest {
    pub written: String,
    pub url: String,
}

/// Which images of a page to read. A page over `https` reads none over `http`, as otherwise
/// anyone on the way could see and change them.
#[must_use]
pub fn image_requests(page_address: &str, sources: &[String]) -> Vec<ImageRequest> {
    let Ok(page) = Url::parse(page_address) else {
        return Vec::new();
    };
    sources
        .iter()
        .filter_map(|written| {
            let mut url = page.join(written).ok()?;
            url.set_fragment(None);
            let allowed = may_redirect(&page, &url);
            allowed.then(|| ImageRequest {
                written: written.clone(),
                url: url.into(),
            })
        })
        .take(MAX_IMAGES)
        .collect()
}

/// A piece of work for [`Workers`].
type Job<T> = Box<dyn FnOnce() -> Option<T> + Send>;

/// Does jobs on a few threads of their own, and hands over what comes of them as it gets there.
struct Workers<T> {
    waiting: Arc<Mutex<VecDeque<Job<T>>>>,
    receiver: Receiver<T>,
    /// What came while checking whether more would come.
    arrived: Vec<T>,
}

impl<T: Send + 'static> Workers<T> {
    fn start(jobs: Vec<Job<T>>) -> Self {
        let threads = WORKER_THREADS.min(jobs.len());
        let waiting = Arc::new(Mutex::new(VecDeque::from(jobs)));
        let (sender, receiver) = mpsc::channel();
        for _ in 0..threads {
            let waiting = Arc::clone(&waiting);
            let sender = sender.clone();
            thread::spawn(move || {
                loop {
                    let next = waiting.lock().ok().and_then(|mut queue| queue.pop_front());
                    let Some(job) = next else {
                        return;
                    };
                    if let Some(result) = job()
                        && sender.send(result).is_err()
                    {
                        return;
                    }
                }
            });
        }
        Self {
            waiting,
            receiver,
            arrived: Vec::new(),
        }
    }

    fn poll(&mut self) -> Vec<T> {
        let mut results = std::mem::take(&mut self.arrived);
        results.extend(self.receiver.try_iter());
        results
    }

    /// Whether every job is over and all has been taken.
    fn is_done(&mut self) -> bool {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.arrived.push(result);
                false
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => self.arrived.is_empty(),
        }
    }
}

impl<T> Drop for Workers<T> {
    fn drop(&mut self) {
        // What is being read ends by itself; the rest is not read at all.
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.clear();
        }
    }
}

/// The images of a page being read, as they get there.
pub struct ImageFetch(Workers<(String, DecodedImage)>);

impl ImageFetch {
    /// Starts reading images for the page at this address.
    #[must_use]
    pub fn start(page_address: &str, requests: Vec<ImageRequest>) -> Self {
        let jobs = requests
            .into_iter()
            .map(|request| {
                let page = page_address.to_owned();
                let job: Job<(String, DecodedImage)> = Box::new(move || {
                    let fetched = fetch_limited(
                        &request.url,
                        Asker::Page(&page),
                        "image/*,*/*;q=0.1",
                        MAX_IMAGE_BYTES,
                    )
                    .ok()?;
                    Some((request.written, decode(&fetched.bytes)?))
                });
                job
            })
            .collect();
        Self(Workers::start(jobs))
    }

    /// The images that have arrived since the last time.
    pub fn poll(&mut self) -> Vec<(String, DecodedImage)> {
        self.0.poll()
    }

    /// Whether every image has been read and taken.
    pub fn is_done(&mut self) -> bool {
        self.0.is_done()
    }
}

/// The style sheets of a page being read, as they get there, with the text of each.
pub struct SheetFetch(Workers<(u32, String)>);

impl SheetFetch {
    /// Starts reading sheets for the page at this address. As with images, a page over `https`
    /// reads none over `http`, and what the server does not call CSS does not count as such, so
    /// that nothing else on a server can be made to act as a sheet.
    #[must_use]
    pub fn start(page_address: &str, requests: Vec<SheetRequest>) -> Self {
        let page_url = Url::parse(page_address).ok();
        let jobs = requests
            .into_iter()
            .filter(|request| {
                let target = Url::parse(&request.location).ok();
                match (&page_url, &target) {
                    (Some(page), Some(target)) => may_redirect(page, target),
                    _ => false,
                }
            })
            .map(|request| {
                let page = page_address.to_owned();
                let job: Job<(u32, String)> = Box::new(move || {
                    let asker = if page.starts_with("file:") {
                        Asker::LocalPage
                    } else {
                        Asker::Page(&page)
                    };
                    let fetched = fetch_limited(
                        &request.location,
                        asker,
                        "text/css,*/*;q=0.1",
                        MAX_SHEET_BYTES,
                    )
                    .ok()?;
                    (fetched.mime.is_empty() || fetched.mime == "text/css")
                        .then(|| (request.slot, decode_css(&fetched.bytes, &fetched.charset)))
                });
                job
            })
            .collect();
        Self(Workers::start(jobs))
    }

    /// The sheets that have arrived since the last time.
    pub fn poll(&mut self) -> Vec<(u32, String)> {
        self.0.poll()
    }

    /// Whether every sheet has been read and taken.
    pub fn is_done(&mut self) -> bool {
        self.0.is_done()
    }
}
