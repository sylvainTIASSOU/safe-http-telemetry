use proptest::prelude::*;
use safe_http_telemetry::policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet};

fn arb_path() -> impl Strategy<Value = String> {
    // Use simple paths without forbidden characters to stay within the valid domain.
    "[a-z]{1,8}".prop_map(|s| format!("/{}", s))
}

fn arb_profile() -> impl Strategy<Value = CaptureProfile> {
    prop_oneof![
        Just(CaptureProfile::Minimal),
        Just(CaptureProfile::Standard),
        Just(CaptureProfile::Detailed),
    ]
}

proptest! {
    /// Invariant: `RoutePolicySet::new(default_profile)` contains no rules
    /// until a policy is added.
    #[test]
    fn empty_policy_set_has_no_rules(profile in arb_profile()) {
        let set = RoutePolicySet::new(profile);
        // `resolve` receives the HTTP method and path.
        let selected = set.resolve("GET", "/does-not-exist");
        prop_assert_eq!(selected.profile, profile);
    }

    /// Invariant: an exact policy for a path takes precedence over a prefix policy.
      #[test]
    fn exact_policy_beats_prefix_policy(
        prefix_path in arb_path(),
        profile_prefix in arb_profile(),
        profile_exact in arb_profile(),
    ) {
        let mut set = RoutePolicySet::new(CaptureProfile::Minimal);

        let prefix_policy = RoutePolicy::new(
            RoutePattern::prefix(prefix_path.clone()),
            profile_prefix,
        );
        let exact_path = format!("{}/child", prefix_path);
        let exact_policy = RoutePolicy::new(
            RoutePattern::exact(exact_path.clone()),
            profile_exact,
        );

        set.add_policy(prefix_policy).unwrap();
        set.add_policy(exact_policy).unwrap();

        // Add the HTTP method "GET".
        let selected = set.resolve("GET", &exact_path);
        prop_assert_eq!(selected.profile, profile_exact, "exact policy must win over prefix policy");
    }
}
