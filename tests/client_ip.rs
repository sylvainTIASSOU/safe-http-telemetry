#![cfg(feature = "tower")]

mod common;

use std::{convert::Infallible, net::SocketAddr};

use bytes::Bytes;
use common::expect_http;
use http::{Request, Response};
use http_body_util::Full;
use ipnet::IpNet;
use safe_http_telemetry::{
    config::ConfigError,
    core::{event::HttpEvent, telemetry::InternalTelemetry},
    policy::{CaptureProfile, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{ClientIpConfig, HttpTelemetryLayer, MiddlewareConfig},
};
use tower::{service_fn, Layer, ServiceExt};

fn queue() -> BoundedEventQueue {
    BoundedEventQueue::new(QueueConfig::new(4), InternalTelemetry::default()).unwrap()
}

fn middleware_config() -> MiddlewareConfig {
    MiddlewareConfig {
        capture_client_ip: true,
        ..MiddlewareConfig::default()
    }
}

async fn capture(
    config: MiddlewareConfig,
    profile: CaptureProfile,
    request: Request<Full<Bytes>>,
) -> HttpEvent {
    let queue = queue();
    let receiver = queue.receiver();
    let layer = HttpTelemetryLayer::new(queue, RoutePolicySet::new(profile), config).unwrap();
    let service = service_fn(|_request: Request<Full<Bytes>>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
    });

    layer.layer(service).oneshot(request).await.unwrap();
    expect_http(receiver.try_recv().expect("one HTTP event"))
}

fn direct_request() -> Request<Full<Bytes>> {
    Request::builder()
        .extension("192.0.2.10:1234".parse::<SocketAddr>().unwrap())
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[tokio::test]
async fn direct_peer_is_used_and_untrusted_forwarding_is_ignored() {
    let request = Request::builder()
        .extension("192.0.2.10:1234".parse::<SocketAddr>().unwrap())
        .header("forwarded", "for=198.51.100.20")
        .header("x-forwarded-for", "198.51.100.30")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let event = capture(middleware_config(), CaptureProfile::Standard, request).await;

    assert_eq!(event.request.client_ip.as_deref(), Some("192.0.2.10"));
    assert!(event.request.geolocation.is_none());
}

#[tokio::test]
async fn trusted_proxy_chain_selects_the_first_untrusted_address_from_the_right() {
    let mut config = middleware_config();
    config.client_ip.trusted_proxies = vec!["10.0.0.0/8".parse::<IpNet>().unwrap()];
    let request = Request::builder()
        .extension("10.0.0.4:1234".parse::<SocketAddr>().unwrap())
        .header("x-forwarded-for", "198.51.100.20, 203.0.113.40, 10.0.0.3")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let event = capture(config, CaptureProfile::Standard, request).await;

    assert_eq!(event.request.client_ip.as_deref(), Some("203.0.113.40"));
}

#[tokio::test]
async fn minimal_profile_and_disabled_capture_emit_no_client_ip() {
    let minimal = capture(
        middleware_config(),
        CaptureProfile::Minimal,
        direct_request(),
    )
    .await;
    assert!(minimal.request.client_ip.is_none());
    assert!(minimal.request.geolocation.is_none());

    let disabled = capture(
        MiddlewareConfig::default(),
        CaptureProfile::Standard,
        direct_request(),
    )
    .await;
    assert!(disabled.request.client_ip.is_none());
    assert!(disabled.request.geolocation.is_none());
}

#[tokio::test]
async fn absent_peer_produces_no_client_ip() {
    let request = Request::builder()
        .header("x-forwarded-for", "198.51.100.20")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let event = capture(middleware_config(), CaptureProfile::Standard, request).await;
    assert!(event.request.client_ip.is_none());
}

#[test]
fn forwarding_limits_must_be_non_zero() {
    for (field, config) in [
        (
            "max_forwarded_header_bytes",
            ClientIpConfig {
                max_forwarded_header_bytes: 0,
                ..ClientIpConfig::default()
            },
        ),
        (
            "max_forwarded_hops",
            ClientIpConfig {
                max_forwarded_hops: 0,
                ..ClientIpConfig::default()
            },
        ),
    ] {
        let config = MiddlewareConfig {
            client_ip: config,
            ..MiddlewareConfig::default()
        };
        let result = HttpTelemetryLayer::new(
            queue(),
            RoutePolicySet::new(CaptureProfile::Standard),
            config,
        );

        match result {
            Err(ConfigError::ZeroLimit(actual)) => assert_eq!(actual, field),
            Err(_) => panic!("unexpected client-IP configuration error"),
            Ok(_) => panic!("zero forwarding limit was accepted"),
        }
    }
}

#[cfg(feature = "axum")]
#[tokio::test]
async fn axum_connect_info_is_accepted_as_the_direct_peer() {
    let request = Request::builder()
        .extension(axum::extract::ConnectInfo(
            "192.0.2.60:4321".parse::<SocketAddr>().unwrap(),
        ))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let event = capture(middleware_config(), CaptureProfile::Standard, request).await;
    assert_eq!(event.request.client_ip.as_deref(), Some("192.0.2.60"));
}

#[cfg(feature = "loki")]
#[test]
fn client_ip_and_geolocation_stay_out_of_loki_labels_and_metadata() {
    use safe_http_telemetry::{
        config::ServiceConfig,
        core::event::{CaptureInfo, GeolocationInfo, RequestInfo, ResponseInfo, TimingInfo},
        loki::payload::LokiStream,
    };

    let mut request = RequestInfo::new("GET", "/users/{id}", "/users/42");
    request.client_ip = Some("203.0.113.40".to_owned());
    request.geolocation = Some(GeolocationInfo {
        country_code: Some("SE".to_owned()),
        country: Some("Sweden".to_owned()),
        city: Some("Stockholm".to_owned()),
        ..GeolocationInfo::default()
    });
    let event = HttpEvent::new(
        "request-geolocation-test",
        request,
        ResponseInfo::new(200),
        TimingInfo::new(1.0),
        CaptureInfo::default(),
    );
    let stream = LokiStream::from_event(&event, &ServiceConfig::default(), 1).unwrap();

    for forbidden in [
        "client_ip",
        "geolocation",
        "country",
        "country_code",
        "city",
        "latitude",
        "longitude",
    ] {
        assert!(!stream.stream.contains_key(forbidden));
        assert!(!stream.metadata.contains_key(forbidden));
    }

    let line = &stream.values[0][1];
    assert!(line.contains("203.0.113.40"));
    assert!(line.contains("Sweden"));
}
