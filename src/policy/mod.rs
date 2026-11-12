//! Capture profiles and deterministic route-policy resolution.
//!
//! Route policies restrict what may be captured. They never grant more
//! capture capability than the selected profile provides.
//!

use std::{error::Error, fmt, vec::Vec};

/// Controls the maximum amount of HTTP data that may be captured.
///
/// The derived ordering is intentional: a lower value is more restrictive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CaptureProfile {
    /// Correlation, method, route, status and timing information only.
    Minimal,
    /// Filtered parameters and headers, without bodies by default.
    Standard,
    /// Bounded request and response bodies after mandatory redaction.
    Detailed,
}

impl CaptureProfile {
    /// Returns whether this profile permits body capture at all.
    pub const fn allows_body_capture(self) -> bool {
        matches!(self, Self::Detailed)
    }

    /// Returns whether filtered query parameters may be captured.
    pub const fn allows_query_capture(self) -> bool {
        matches!(self, Self::Standard | Self::Detailed)
    }

    /// Returns whether filtered request and response header may be captured
    pub const fn allows_header_capture(self) -> bool {
        matches!(self, Self::Standard | Self::Detailed)
    }

    /// Returns whether client geolocation may be resolved.
    pub const fn allows_geolocation(self) -> bool {
        matches!(self, Self::Standard | Self::Detailed)
    }
}

/// Matches either one exact route path or a path-segment prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutePattern {
    /// Matches a single route path exactly.
    Exact(String),
    /// Matches any route path that starts with the given prefix.
    Prefix(String),
}

impl RoutePattern {
    pub fn exact(path: impl Into<String>) -> Self {
        Self::Exact(path.into())
    }

    /// Creates a prefix matcher.
    ///
    /// Except for `/`, prefixes must not end with `/`. A prefix such as
    /// `/api` matches `/api` and `/api/users`, but not `/apiculture`.
    pub fn prefix(path: impl Into<String>) -> Self {
        Self::Prefix(path.into())
    }

    fn path(&self) -> &str {
        match self {
            Self::Exact(path) | Self::Prefix(path) => path,
        }
    }

    fn matches_path(&self, path: &str) -> bool {
        match self {
            Self::Exact(expected) => path == expected,
            Self::Prefix(prefix) if prefix == "/" => path.starts_with('/'),
            Self::Prefix(prefix) => {
                path == prefix
                    || path
                        .strip_prefix(prefix)
                        .is_some_and(|remainder| remainder.starts_with('/'))
            }
        }
    }

    fn pattern_specificity(&self) -> u8 {
        match self {
            Self::Prefix(_) => 0,
            Self::Exact(_) => 1,
        }
    }

    fn validate(&self) -> Result<(), PolicyError> {
        let path = self.path();

        if !path.starts_with('/')
            || path.contains(['?', '#', '*'])
            || path.chars().any(char::is_whitespace)
        {
            return Err(PolicyError::InvalidPath(path.to_owned()));
        }

        if matches!(self, Self::Prefix(_)) && path != "/" && path.ends_with('/') {
            return Err(PolicyError::InvalidPath(path.to_owned()));
        }

        Ok(())
    }
}

/// A capture palicy attached to a route selector
#[derive(Debug, Clone)]
pub struct RoutePolicy {
    pub pattern: RoutePattern,
    pub method: Option<String>,
    pub profile: CaptureProfile,
    pub excluded: bool,
    pub capture_request_body: Option<bool>,
    pub capture_response_body: Option<bool>,
    pub capture_query: Option<bool>,
    pub capture_request_headers: Option<bool>,
    pub capture_response_headers: Option<bool>,
}

impl RoutePolicy {
    pub fn new(pattern: RoutePattern, profile: CaptureProfile) -> Self {
        Self {
            pattern,
            method: None,
            profile,
            excluded: false,
            capture_request_body: None,
            capture_response_body: None,
            capture_query: None,
            capture_request_headers: None,
            capture_response_headers: None,
        }
    }

    /// Restricts this rule to one HTTP method.
    pub fn for_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Excludes matching requests from HTTP telemetry capture.
    pub fn exclude(mut self) -> Self {
        self.excluded = true;
        self.capture_request_body = Some(false);
        self.capture_response_body = Some(false);
        self.capture_query = Some(false);
        self.capture_request_headers = Some(false);
        self.capture_response_headers = Some(false);
        self
    }

    /// Adds body-capture restrictions or explicit permissions.
    ///
    /// Setting a value to `true` never elevates a non-detailed profile.
    pub fn with_body_capture(mut self, request: bool, response: bool) -> Self {
        self.capture_request_body = Some(request);
        self.capture_response_body = Some(response);
        self
    }

    /// Restricts query-parameter capture for matching requests.
    ///
    /// `True` never elevates the minimal profils.
    pub fn with_query_capture(mut self, capture: bool) -> Self {
        self.capture_query = Some(capture);
        self
    }

    /// Restricts request and response header capture independently
    ///
    /// `true` never elevates the minimal profile
    pub fn with_header_capture(mut self, request: bool, response: bool) -> Self {
        self.capture_request_headers = Some(request);
        self.capture_response_headers = Some(response);
        self
    }

    fn matches(&self, method: &str, path: &str) -> bool {
        self.pattern.matches_path(path)
            && self
                .method
                .as_deref()
                .is_none_or(|expected| expected.eq_ignore_ascii_case(method))
    }

    fn specificity(&self) -> (u8, usize, u8) {
        (
            self.pattern.pattern_specificity(),
            self.pattern.path().len(),
            u8::from(self.method.is_some()),
        )
    }

    fn validate(&self) -> Result<(), PolicyError> {
        self.pattern.validate()?;

        if let Some(method) = &self.method {
            if !is_valid_http_method(method) {
                return Err(PolicyError::InvalidMethod(method.clone()));
            }
        }

        Ok(())
    }

    fn has_same_selector(&self, other: &Self) -> bool {
        self.pattern == other.pattern
            && match (&self.method, &other.method) {
                (None, None) => true,
                (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
                _ => false,
            }
    }
}

/// The effective capture decision for one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyDecision {
    pub excluded: bool,
    pub profile: CaptureProfile,
    pub capture_request_body: bool,
    pub capture_response_body: bool,
    pub capture_query: bool,
    pub capture_request_headers: bool,
    pub capture_response_headers: bool,
}

impl PolicyDecision {
    fn from_profile(profile: CaptureProfile) -> Self {
        Self {
            excluded: false,
            profile,
            capture_request_body: profile.allows_body_capture(),
            capture_response_body: profile.allows_body_capture(),
            capture_query: profile.allows_query_capture(),
            capture_request_headers: profile.allows_header_capture(),
            capture_response_headers: profile.allows_header_capture(),
        }
    }
}

/// An ordered-independent collection of route policies.
#[derive(Clone, Debug)]
pub struct RoutePolicySet {
    default_profile: CaptureProfile,
    policies: Vec<RoutePolicy>,
}

impl RoutePolicySet {
    pub fn new(default_profile: CaptureProfile) -> Self {
        Self {
            default_profile,
            policies: Vec::new(),
        }
    }

    /// Adds a validated route policy.
    ///
    /// Duplicate selectors are rejected instead of depending on insertion
    /// order. Matching is case-insensitive for HTTP methods.
    pub fn add_policy(&mut self, policy: RoutePolicy) -> Result<(), PolicyError> {
        policy.validate()?;

        if self
            .policies
            .iter()
            .any(|existing| existing.has_same_selector(&policy))
        {
            return Err(PolicyError::DuplicateRouteSelector);
        }

        self.policies.push(policy);
        Ok(())
    }

    /// Resolves the effective policy without using insertion order.
    pub fn resolve(&self, method: &str, path: &str) -> PolicyDecision {
        let best_specificity = self
            .policies
            .iter()
            .filter(|policy| policy.matches(method, path))
            .map(RoutePolicy::specificity)
            .max();

        let Some(best_specificity) = best_specificity else {
            return PolicyDecision::from_profile(self.default_profile);
        };

        let mut accumulator: Option<MatchAccumulator> = None;

        for policy in self
            .policies
            .iter()
            .filter(|policy| policy.matches(method, path))
            .filter(|policy| policy.specificity() == best_specificity)
        {
            match &mut accumulator {
                Some(current) => current.merge(policy),
                None => {
                    accumulator = Some(MatchAccumulator::new(policy));
                }
            }
        }

        accumulator.map_or_else(
            || PolicyDecision::from_profile(self.default_profile),
            MatchAccumulator::into_decision,
        )
    }
}

impl Default for RoutePolicySet {
    fn default() -> Self {
        Self::new(CaptureProfile::Standard)
    }
}

#[derive(Clone, Copy)]
struct MatchAccumulator {
    profile: CaptureProfile,
    excluded: bool,
    request_body_denied: bool,
    response_body_denied: bool,
    query_denied: bool,
    request_headers_denied: bool,
    response_headers_denied: bool,
}

impl MatchAccumulator {
    fn new(policy: &RoutePolicy) -> Self {
        Self {
            profile: policy.profile,
            excluded: policy.excluded,
            request_body_denied: policy.capture_request_body == Some(false),
            response_body_denied: policy.capture_response_body == Some(false),
            query_denied: policy.capture_query == Some(false),
            request_headers_denied: policy.capture_request_headers == Some(false),
            response_headers_denied: policy.capture_response_headers == Some(false),
        }
    }

    fn merge(&mut self, policy: &RoutePolicy) {
        self.profile = self.profile.min(policy.profile);
        self.excluded |= policy.excluded;
        self.request_body_denied |= policy.capture_request_body == Some(false);
        self.response_body_denied |= policy.capture_response_body == Some(false);
        self.query_denied |= policy.capture_query == Some(false);
        self.request_headers_denied |= policy.capture_request_headers == Some(false);
        self.response_headers_denied |= policy.capture_response_headers == Some(false);
    }

    fn into_decision(self) -> PolicyDecision {
        let bodies_allowed = self.profile.allows_body_capture() && !self.excluded;
        let query_allowed = self.profile.allows_query_capture() && !self.excluded;
        let headers_allowed = self.profile.allows_header_capture() && !self.excluded;

        PolicyDecision {
            excluded: self.excluded,
            profile: self.profile,
            capture_request_body: bodies_allowed && !self.request_body_denied,
            capture_response_body: bodies_allowed && !self.response_body_denied,
            capture_query: query_allowed && !self.query_denied,
            capture_request_headers: headers_allowed && !self.request_headers_denied,
            capture_response_headers: headers_allowed && !self.response_headers_denied,
        }
    }
}

fn is_valid_http_method(method: &str) -> bool {
    !method.is_empty()
        && method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// A route-policy configuration error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    InvalidPath(String),
    InvalidMethod(String),
    DuplicateRouteSelector,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(path) => {
                write!(formatter, "invalid route pattern: `{path}`")
            }
            Self::InvalidMethod(method) => {
                write!(formatter, "invalid HTTP method: `{method}`")
            }
            Self::DuplicateRouteSelector => formatter.write_str("duplicate route policy selector"),
        }
    }
}

impl Error for PolicyError {}
