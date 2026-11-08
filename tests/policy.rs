use safe_http_telemetry::policy::{
    CaptureProfile, PolicyError, RoutePattern, RoutePolicy, RoutePolicySet,
};

#[test]
fn profiles_are_ordered_from_restrictive_to_detailed() {
    assert!(CaptureProfile::Minimal < CaptureProfile::Standard);
    assert!(CaptureProfile::Standard < CaptureProfile::Detailed);

    assert!(!CaptureProfile::Minimal.allows_query_capture());
    assert!(CaptureProfile::Standard.allows_query_capture());
    assert!(CaptureProfile::Detailed.allows_body_capture());
}

#[test]
fn default_standard_profile_does_not_capture_bodies() {
    let policies = RoutePolicySet::default();

    let decision = policies.resolve("GET", "/api/users");

    assert_eq!(decision.profile, CaptureProfile::Standard);
    assert!(!decision.excluded);
    assert!(!decision.capture_request_body);
    assert!(!decision.capture_response_body);
}

#[test]
fn exact_route_beats_prefix_route() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::prefix("/api"),
            CaptureProfile::Detailed,
        ))
        .unwrap();

    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::exact("/api/users"),
            CaptureProfile::Minimal,
        ))
        .unwrap();

    let decision = policies.resolve("GET", "/api/users");

    assert_eq!(decision.profile, CaptureProfile::Minimal);
    assert!(!decision.capture_request_body);
}

#[test]
fn longest_prefix_wins() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::prefix("/api"),
            CaptureProfile::Standard,
        ))
        .unwrap();

    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::prefix("/api/admin"),
            CaptureProfile::Detailed,
        ))
        .unwrap();

    let decision = policies.resolve("GET", "/api/admin/users");

    assert_eq!(decision.profile, CaptureProfile::Detailed);
    assert!(decision.capture_request_body);
    assert!(decision.capture_response_body);
}

#[test]
fn method_specific_rule_wins_for_the_same_route() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::exact("/api/users"),
            CaptureProfile::Standard,
        ))
        .unwrap();

    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/api/users"), CaptureProfile::Detailed)
                .for_method("POST"),
        )
        .unwrap();

    let post = policies.resolve("POST", "/api/users");
    let get = policies.resolve("GET", "/api/users");

    assert_eq!(post.profile, CaptureProfile::Detailed);
    assert_eq!(get.profile, CaptureProfile::Standard);
}

#[test]
fn method_matching_is_case_insensitive() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/api/users"), CaptureProfile::Detailed)
                .for_method("post"),
        )
        .unwrap();

    let decision = policies.resolve("POST", "/api/users");

    assert_eq!(decision.profile, CaptureProfile::Detailed);
}

#[test]
fn detailed_route_captures_bodies_by_default() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::exact("/api/orders"),
            CaptureProfile::Detailed,
        ))
        .unwrap();

    let decision = policies.resolve("POST", "/api/orders");

    assert!(decision.capture_request_body);
    assert!(decision.capture_response_body);
}

#[test]
fn route_can_independently_disable_request_body() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/api/orders"), CaptureProfile::Detailed)
                .with_body_capture(false, true),
        )
        .unwrap();

    let decision = policies.resolve("POST", "/api/orders");

    assert_eq!(decision.profile, CaptureProfile::Detailed);
    assert!(!decision.capture_request_body);
    assert!(decision.capture_response_body);
}

#[test]
fn body_permission_cannot_elevate_standard_profile() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/api/users"), CaptureProfile::Standard)
                .with_body_capture(true, true),
        )
        .unwrap();

    let decision = policies.resolve("POST", "/api/users");

    assert!(!decision.capture_request_body);
    assert!(!decision.capture_response_body);
}

#[test]
fn excluded_route_never_captures_bodies() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Detailed);

    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/auth/login"), CaptureProfile::Detailed)
                .exclude(),
        )
        .unwrap();

    let decision = policies.resolve("POST", "/auth/login");

    assert!(decision.excluded);
    assert!(!decision.capture_request_body);
    assert!(!decision.capture_response_body);
}

#[test]
fn prefix_matching_respects_path_segment_boundaries() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::prefix("/api"),
            CaptureProfile::Detailed,
        ))
        .unwrap();

    let matching = policies.resolve("GET", "/api/users");
    let non_matching = policies.resolve("GET", "/apiculture");

    assert_eq!(matching.profile, CaptureProfile::Detailed);
    assert_eq!(non_matching.profile, CaptureProfile::Minimal);
}

#[test]
fn duplicate_selectors_are_rejected_case_insensitively() {
    let mut policies = RoutePolicySet::default();

    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/api/users"), CaptureProfile::Minimal)
                .for_method("GET"),
        )
        .unwrap();

    let duplicate = RoutePolicy::new(RoutePattern::exact("/api/users"), CaptureProfile::Detailed)
        .for_method("get");

    assert_eq!(
        policies.add_policy(duplicate),
        Err(PolicyError::DuplicateRouteSelector)
    );
}

#[test]
fn invalid_paths_are_rejected() {
    let mut policies = RoutePolicySet::default();

    for path in [
        "api/users",
        "/api/users?debug=true",
        "/api/*",
        "/api/",
        "/api users",
    ] {
        let result = policies.add_policy(RoutePolicy::new(
            RoutePattern::prefix(path),
            CaptureProfile::Minimal,
        ));

        assert!(
            matches!(result, Err(PolicyError::InvalidPath(_))),
            "path should be rejected: {path}"
        );
    }
}

#[test]
fn invalid_methods_are_rejected() {
    let mut policies = RoutePolicySet::default();

    let result = policies.add_policy(
        RoutePolicy::new(RoutePattern::exact("/api/users"), CaptureProfile::Minimal)
            .for_method("GET POST"),
    );

    assert_eq!(
        result,
        Err(PolicyError::InvalidMethod("GET POST".to_owned()))
    );
}
