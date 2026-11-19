//! Small fixed-window admission controls for anonymous expensive requests.

use std::collections::HashMap;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use axum::extract::ConnectInfo;
use axum::extract::Request;
use axum::http::Method;

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
enum Bucket {
    Challenge,
    Publish,
    DeviceStart,
    DevicePoll,
    Oauth,
    Resolve,
    Playground,
}

impl Bucket {
    #[allow(
        clippy::single_call_fn,
        reason = "request paths map to independent rate-limit buckets"
    )]
    fn request(request: &Request) -> Option<Self> {
        match (request.method(), request.uri().path()) {
            | (&Method::POST, "/api/v1/publish/challenge" | "/api/v1/yank/challenge") => {
                Some(Self::Challenge)
            },
            | (&Method::POST, "/api/v1/publish" | "/api/v1/yank") => Some(Self::Publish),
            | (&Method::POST, "/api/v1/auth/device") => Some(Self::DeviceStart),
            | (&Method::GET, path) if path.starts_with("/api/v1/auth/device/") => {
                Some(Self::DevicePoll)
            },
            | (&Method::GET, path) if path.starts_with("/auth/") => Some(Self::Oauth),
            | (&Method::POST, "/api/v1/resolve") => Some(Self::Resolve),
            | (&Method::POST, "/api/v1/playground/run") => Some(Self::Playground),
            | _ => None,
        }
    }

    const fn allowances(self) -> (u32, u32) {
        match self {
            | Self::Challenge => (60, 1200),
            | Self::Publish => (20, 240),
            | Self::DeviceStart | Self::Playground => (12, 240),
            | Self::DevicePoll => (180, 3600),
            | Self::Oauth => (30, 600),
            | Self::Resolve => (60, 600),
        }
    }
}

struct Window {
    start: Instant,
    totals: HashMap<Bucket, u32>,
    clients: HashMap<(Bucket, IpAddr), u32>,
}

impl Window {
    #[allow(
        clippy::single_call_fn,
        reason = "the window accepts an explicit instant for deterministic tests"
    )]
    fn starting_at(start: Instant) -> Self {
        Self {
            start,
            totals: HashMap::new(),
            clients: HashMap::new(),
        }
    }

    fn allow(
        &mut self,
        bucket: Bucket,
        client: IpAddr,
        now: Instant,
    ) -> bool {
        if now.duration_since(self.start) >= WINDOW {
            self.start = now;
            self.totals.clear();
            self.clients.clear();
        }
        let (per_client, total) = bucket.allowances();
        let requests = self.totals.entry(bucket).or_default();
        if *requests >= total {
            return false;
        }
        // A client entry is only inserted before the global limit is reached.
        let client_requests = self.clients.entry((bucket, client)).or_default();
        if *client_requests >= per_client {
            return false;
        }
        *client_requests = client_requests.saturating_add(1);
        *requests = requests.saturating_add(1);
        true
    }
}

pub struct RateLimits(Mutex<Window>);

impl RateLimits {
    #[allow(
        clippy::single_call_fn,
        reason = "configuration constructors centralize their invariants"
    )]
    pub(crate) fn new() -> Self {
        Self(Mutex::new(Window::starting_at(Instant::now())))
    }

    pub(crate) fn allow(
        &self,
        request: &Request,
        trust_proxy: bool,
    ) -> bool {
        let Some(bucket) = Bucket::request(request) else {
            return true;
        };
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map_or_else(|| Ipv4Addr::LOCALHOST.into(), |peer| peer.0.ip());
        let client = if trust_proxy {
            request
                .headers()
                .get("x-forwarded-for")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<IpAddr>().ok())
                .unwrap_or(peer)
        } else {
            peer
        };
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allow(bucket, network(client), Instant::now())
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "client network normalization has its own IPv4 and IPv6 policy"
)]
fn network(ip: IpAddr) -> IpAddr {
    match ip {
        | IpAddr::V4(ip) => ip.into(),
        | IpAddr::V6(ip) => {
            ip.to_ipv4_mapped().map_or_else(
                || Ipv6Addr::from_bits(ip.to_bits() & (u128::MAX << 64)).into(),
                IpAddr::V4,
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_hits_its_limit_without_blocking_another_client() {
        let now = Instant::now();
        let mut window = Window::starting_at(now);
        let client = Ipv4Addr::LOCALHOST.into();
        let another = Ipv4Addr::new(192, 0, 2, 2).into();
        let bucket = Bucket::DeviceStart;
        for _ in 0..bucket.allowances().0 {
            assert!(window.allow(bucket, client, now));
        }
        assert!(!window.allow(bucket, client, now));
        assert!(window.allow(bucket, another, now));
        assert!(window.allow(bucket, client, now + WINDOW));
    }

    #[test]
    fn adjacent_ipv6_client_addresses_share_their_network_limit() {
        let client: IpAddr = "2001:db8::1".parse().unwrap();
        let another: IpAddr = "2001:db8::16".parse().unwrap();
        assert_eq!(network(client), network(another));
    }
}
