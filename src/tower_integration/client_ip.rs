//! Client-IP resolution with an explicit trusted-proxy boundary.

use std::net::{IpAddr, SocketAddr};

use http::{header::FORWARDED, HeaderMap, HeaderName, Request};

use super::config::ClientIpConfig;

const X_FORWARDED_FOR: &str = "x-forwarded-for";

pub(crate) fn resolve<Body>(request: &Request<Body>, config: &ClientIpConfig) -> Option<IpAddr> {
    let peer = direct_peer(request)?.ip();

    if !is_trusted(peer, config) {
        return Some(peer);
    }

    match forwarding_chain(request.headers(), config) {
        Chain::Present(chain) => Some(select_client(peer, &chain, config)),
        Chain::Missing | Chain::Invalid => Some(peer),
    }
}

fn direct_peer<Body>(request: &Request<Body>) -> Option<SocketAddr> {
    if let Some(peer) = request.extensions().get::<SocketAddr>() {
        return Some(*peer);
    }

    #[cfg(feature = "axum")]
    if let Some(peer) = request
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
    {
        return Some(peer.0);
    }

    None
}

fn is_trusted(address: IpAddr, config: &ClientIpConfig) -> bool {
    config
        .trusted_proxies
        .iter()
        .any(|network| network.contains(&address))
}

enum Chain {
    Missing,
    Invalid,
    Present(Vec<IpAddr>),
}

fn forwarding_chain(headers: &HeaderMap, config: &ClientIpConfig) -> Chain {
    match read_chain(headers, &FORWARDED, config, parse_forwarded) {
        Chain::Missing => read_chain(
            headers,
            &HeaderName::from_static(X_FORWARDED_FOR),
            config,
            parse_x_forwarded_for,
        ),
        result => result,
    }
}

fn read_chain(
    headers: &HeaderMap,
    name: &HeaderName,
    config: &ClientIpConfig,
    parse: fn(&str, &mut Vec<IpAddr>, usize) -> bool,
) -> Chain {
    let mut found = false;
    let mut total_bytes = 0usize;
    let mut chain = Vec::new();

    for value in headers.get_all(name) {
        found = true;
        total_bytes = total_bytes.saturating_add(value.len());
        if total_bytes > config.max_forwarded_header_bytes {
            return Chain::Invalid;
        }

        let Ok(value) = value.to_str() else {
            return Chain::Invalid;
        };
        if !parse(value, &mut chain, config.max_forwarded_hops) {
            return Chain::Invalid;
        }
    }

    if !found {
        Chain::Missing
    } else if chain.is_empty() {
        Chain::Invalid
    } else {
        Chain::Present(chain)
    }
}

fn parse_forwarded(value: &str, chain: &mut Vec<IpAddr>, max_hops: usize) -> bool {
    for_each_segment(value, ',', |element| {
        let mut address = None;
        let valid = for_each_segment(element, ';', |parameter| {
            let parameter = parameter.trim();
            let Some((name, value)) = parameter.split_once('=') else {
                return false;
            };

            if name.trim().eq_ignore_ascii_case("for") {
                if address.is_some() {
                    return false;
                }
                address = parse_node(value);
                return address.is_some();
            }

            true
        });

        valid && push_address(address, chain, max_hops)
    })
}

fn parse_x_forwarded_for(value: &str, chain: &mut Vec<IpAddr>, max_hops: usize) -> bool {
    for_each_segment(value, ',', |entry| {
        push_address(parse_node(entry), chain, max_hops)
    })
}

fn push_address(address: Option<IpAddr>, chain: &mut Vec<IpAddr>, max_hops: usize) -> bool {
    let Some(address) = address else {
        return false;
    };
    if chain.len() >= max_hops {
        return false;
    }

    chain.push(address);
    true
}

/// Splits on delimiters outside quoted strings without allocating substrings.
fn for_each_segment(value: &str, delimiter: char, mut visit: impl FnMut(&str) -> bool) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;

    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            character if character == delimiter && !quoted => {
                if !visit(&value[start..index]) {
                    return false;
                }
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }

    !quoted && !escaped && visit(&value[start..])
}

fn parse_node(value: &str) -> Option<IpAddr> {
    let value = value.trim();
    let value = if value.starts_with('"') {
        let inner = value.strip_prefix('"')?.strip_suffix('"')?;
        if inner.contains(['"', '\\']) {
            return None;
        }
        inner.trim()
    } else {
        if value.contains('"') {
            return None;
        }
        value
    };

    if let Ok(address) = value.parse::<IpAddr>() {
        return Some(address);
    }
    if let Ok(address) = value.parse::<SocketAddr>() {
        return Some(address.ip());
    }
    if let Some(address) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return address.parse().ok();
    }

    None
}

fn select_client(peer: IpAddr, chain: &[IpAddr], config: &ClientIpConfig) -> IpAddr {
    for address in chain.iter().rev() {
        if !is_trusted(*address, config) {
            return *address;
        }
    }

    // If every declared hop is trusted, the leftmost hop is the closest
    // available approximation of the original client.
    chain.first().copied().unwrap_or(peer)
}

#[cfg(test)]
mod tests {
    use ipnet::IpNet;

    use super::*;

    fn proxy_config() -> ClientIpConfig {
        ClientIpConfig {
            trusted_proxies: vec!["10.0.0.0/8".parse::<IpNet>().unwrap()],
            ..ClientIpConfig::default()
        }
    }

    fn peer_request(peer: &str) -> Request<()> {
        Request::builder()
            .extension(peer.parse::<SocketAddr>().unwrap())
            .body(())
            .unwrap()
    }

    #[test]
    fn untrusted_peer_cannot_inject_forwarding_headers() {
        let request = Request::builder()
            .extension("192.0.2.10:1234".parse::<SocketAddr>().unwrap())
            .header(FORWARDED, "for=198.51.100.20")
            .header(X_FORWARDED_FOR, "198.51.100.30")
            .body(())
            .unwrap();

        assert_eq!(
            resolve(&request, &ClientIpConfig::default()),
            Some(IpAddr::from([192, 0, 2, 10]))
        );
    }

    #[test]
    fn trusted_chain_is_walked_from_right_to_left() {
        let mut request = peer_request("10.0.0.3:1234");
        request.headers_mut().insert(
            HeaderName::from_static(X_FORWARDED_FOR),
            "198.51.100.20, 203.0.113.40, 10.0.0.2".parse().unwrap(),
        );

        assert_eq!(
            resolve(&request, &proxy_config()),
            Some(IpAddr::from([203, 0, 113, 40]))
        );
    }

    #[test]
    fn forwarded_supports_quoted_ipv6_and_takes_precedence() {
        let mut request = peer_request("10.0.0.3:1234");
        request.headers_mut().insert(
            FORWARDED,
            "for=198.51.100.20;proto=https, for=\"[2001:db8::40]:443\""
                .parse()
                .unwrap(),
        );
        request.headers_mut().insert(
            HeaderName::from_static(X_FORWARDED_FOR),
            "203.0.113.99".parse().unwrap(),
        );

        assert_eq!(
            resolve(&request, &proxy_config()),
            Some("2001:db8::40".parse::<IpAddr>().unwrap())
        );
    }

    #[test]
    fn invalid_forwarded_value_falls_back_to_the_direct_peer() {
        let mut request = peer_request("10.0.0.3:1234");
        request
            .headers_mut()
            .insert(FORWARDED, "for=fictional.invalid".parse().unwrap());
        request.headers_mut().insert(
            HeaderName::from_static(X_FORWARDED_FOR),
            "203.0.113.99".parse().unwrap(),
        );

        assert_eq!(
            resolve(&request, &proxy_config()),
            Some(IpAddr::from([10, 0, 0, 3]))
        );
    }

    #[test]
    fn forwarding_limits_reject_the_whole_chain() {
        let mut config = proxy_config();
        config.max_forwarded_hops = 1;
        let mut request = peer_request("10.0.0.3:1234");
        request.headers_mut().insert(
            HeaderName::from_static(X_FORWARDED_FOR),
            "198.51.100.20, 203.0.113.40".parse().unwrap(),
        );
        assert_eq!(
            resolve(&request, &config),
            Some(IpAddr::from([10, 0, 0, 3]))
        );

        let mut config = proxy_config();
        config.max_forwarded_header_bytes = 8;
        let mut request = peer_request("10.0.0.3:1234");
        request.headers_mut().insert(
            HeaderName::from_static(X_FORWARDED_FOR),
            "198.51.100.20".parse().unwrap(),
        );
        assert_eq!(
            resolve(&request, &config),
            Some(IpAddr::from([10, 0, 0, 3]))
        );
    }

    #[test]
    fn missing_peer_produces_no_client_address() {
        assert!(resolve(&Request::new(()), &proxy_config()).is_none());
    }
}
