//! Local MMDB geolocation with bounded loading and caching.
//!
//! Lookups never perform network I/O. Database loading happens once during
//! engine construction, and lookup failures degrade to an absent result.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    fs::File,
    io::Read,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use maxminddb::{geoip2, Reader};

use crate::core::{event::GeolocationInfo, telemetry::InternalTelemetry};

const DEFAULT_CACHE_CAPACITY: usize = 10_000;
const DEFAULT_MAX_DATABASE_BYTES: u64 = 128 * 1024 * 1024;
const DEFAULT_MAX_STRING_BYTES: usize = 256;

/// Configuration for a local GeoIP2/GeoLite2 City or Country database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeolocationConfig {
    pub database_path: PathBuf,
    pub cache_capacity: usize,
    pub cache_ttl: Duration,
    pub max_database_bytes: u64,
    pub max_string_bytes: usize,
    pub fields: GeolocationFields,
}

impl GeolocationConfig {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
            cache_capacity: DEFAULT_CACHE_CAPACITY,
            cache_ttl: Duration::from_secs(300),
            max_database_bytes: DEFAULT_MAX_DATABASE_BYTES,
            max_string_bytes: DEFAULT_MAX_STRING_BYTES,
            fields: GeolocationFields::default(),
        }
    }

    pub fn validate(&self) -> Result<(), GeolocationConfigError> {
        if self.database_path.as_os_str().is_empty() {
            return Err(GeolocationConfigError::EmptyDatabasePath);
        }
        if self.cache_capacity == 0 {
            return Err(GeolocationConfigError::ZeroCacheCapacity);
        }
        if self.cache_ttl.is_zero() {
            return Err(GeolocationConfigError::ZeroCacheTtl);
        }
        if self.max_database_bytes == 0 {
            return Err(GeolocationConfigError::ZeroDatabaseLimit);
        }
        if self.max_string_bytes == 0 {
            return Err(GeolocationConfigError::ZeroStringLimit);
        }
        if self.fields.is_empty() {
            return Err(GeolocationConfigError::NoFieldsSelected);
        }

        Ok(())
    }
}

/// Selects the approximate geographic fields retained in HTTP events.
///
/// The default retains only the country code and English country name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeolocationFields {
    pub country_code: bool,
    pub country: bool,
    pub region: bool,
    pub city: bool,
    pub coordinates: bool,
}

impl GeolocationFields {
    pub const fn all() -> Self {
        Self {
            country_code: true,
            country: true,
            region: true,
            city: true,
            coordinates: true,
        }
    }

    const fn is_empty(self) -> bool {
        !self.country_code && !self.country && !self.region && !self.city && !self.coordinates
    }
}

impl Default for GeolocationFields {
    fn default() -> Self {
        Self {
            country_code: true,
            country: true,
            region: false,
            city: false,
            coordinates: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeolocationConfigError {
    EmptyDatabasePath,
    ZeroCacheCapacity,
    ZeroCacheTtl,
    ZeroDatabaseLimit,
    ZeroStringLimit,
    NoFieldsSelected,
}

impl fmt::Display for GeolocationConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyDatabasePath => "geolocation database path must not be empty",
            Self::ZeroCacheCapacity => "geolocation cache capacity must be greater than zero",
            Self::ZeroCacheTtl => "geolocation cache TTL must be greater than zero",
            Self::ZeroDatabaseLimit => "geolocation database limit must be greater than zero",
            Self::ZeroStringLimit => "geolocation string limit must be greater than zero",
            Self::NoFieldsSelected => "at least one geolocation field must be selected",
        })
    }
}

impl Error for GeolocationConfigError {}

/// Startup errors contain no database contents or filesystem path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeolocationError {
    InvalidConfig(GeolocationConfigError),
    DatabaseOpen,
    DatabaseTooLarge { limit: u64 },
    DatabaseRead,
    InvalidDatabase,
}

impl fmt::Display for GeolocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(error) => error.fmt(formatter),
            Self::DatabaseOpen => formatter.write_str("geolocation database could not be opened"),
            Self::DatabaseTooLarge { limit } => {
                write!(
                    formatter,
                    "geolocation database exceeds the {limit}-byte limit"
                )
            }
            Self::DatabaseRead => formatter.write_str("geolocation database could not be read"),
            Self::InvalidDatabase => formatter.write_str("geolocation database is invalid"),
        }
    }
}

impl Error for GeolocationError {}

impl From<GeolocationConfigError> for GeolocationError {
    fn from(error: GeolocationConfigError) -> Self {
        Self::InvalidConfig(error)
    }
}

/// A cloneable local geolocation engine sharing its database and cache.
///
/// This data-bearing type intentionally does not implement `Debug`.
#[derive(Clone)]
pub struct GeolocationEngine {
    reader: Arc<Reader<Vec<u8>>>,
    cache: Arc<Mutex<BoundedCache>>,
    telemetry: InternalTelemetry,
    cache_ttl: Duration,
    max_string_bytes: usize,
    fields: GeolocationFields,
}

impl GeolocationEngine {
    /// Loads and validates a local database without using memory mapping.
    pub fn open(
        config: &GeolocationConfig,
        telemetry: InternalTelemetry,
    ) -> Result<Self, GeolocationError> {
        config.validate()?;
        let bytes = match read_database(&config.database_path, config.max_database_bytes) {
            Ok(bytes) => bytes,
            Err(error) => {
                telemetry.record_geolocation_error();
                return Err(error);
            }
        };
        let reader = match Reader::from_source(bytes) {
            Ok(reader) => reader,
            Err(_) => {
                telemetry.record_geolocation_error();
                return Err(GeolocationError::InvalidDatabase);
            }
        };

        Ok(Self {
            reader: Arc::new(reader),
            cache: Arc::new(Mutex::new(BoundedCache::new(config.cache_capacity))),
            telemetry,
            cache_ttl: config.cache_ttl,
            max_string_bytes: config.max_string_bytes,
            fields: config.fields,
        })
    }

    /// Resolves an address locally, returning `None` for unknown or bad data.
    pub fn lookup(&self, address: IpAddr) -> Option<GeolocationInfo> {
        let now = Instant::now();

        {
            let mut cache = self.lock_cache();
            if let Some(result) = cache.get(address, now) {
                self.telemetry.record_geolocation_cache_hit();
                return result;
            }
        }
        self.telemetry.record_geolocation_cache_miss();

        let result = match self.lookup_database(address) {
            Ok(result) => result,
            Err(()) => {
                self.telemetry.record_geolocation_error();
                return None;
            }
        };

        if let Some(expires_at) = now.checked_add(self.cache_ttl) {
            let mut cache = self.lock_cache();
            cache.insert(address, result.clone(), expires_at);
        }

        result
    }

    fn lookup_database(&self, address: IpAddr) -> Result<Option<GeolocationInfo>, ()> {
        let lookup = self.reader.lookup(address).map_err(|_| ())?;
        let record = lookup.decode::<geoip2::City>().map_err(|_| ())?;
        Ok(record.and_then(|record| map_record(record, self.fields, self.max_string_bytes)))
    }

    fn lock_cache(&self) -> MutexGuard<'_, BoundedCache> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn map_record(
    record: geoip2::City<'_>,
    fields: GeolocationFields,
    max_string_bytes: usize,
) -> Option<GeolocationInfo> {
    let mut info = GeolocationInfo::default();

    if fields.country_code {
        info.country_code = record.country.iso_code.and_then(normalize_country_code);
    }
    if fields.country {
        info.country = record
            .country
            .names
            .english
            .and_then(|value| bounded_string(value, max_string_bytes));
    }
    if fields.region {
        info.region = record.subdivisions.first().and_then(|subdivision| {
            subdivision
                .names
                .english
                .or(subdivision.iso_code)
                .and_then(|value| bounded_string(value, max_string_bytes))
        });
    }
    if fields.city {
        info.city = record
            .city
            .names
            .english
            .and_then(|value| bounded_string(value, max_string_bytes));
    }
    if fields.coordinates {
        info.latitude = record.location.latitude.and_then(valid_latitude);
        info.longitude = record.location.longitude.and_then(valid_longitude);
    }

    (info != GeolocationInfo::default()).then_some(info)
}

fn read_database(path: &Path, limit: u64) -> Result<Vec<u8>, GeolocationError> {
    let file = File::open(path).map_err(|_| GeolocationError::DatabaseOpen)?;

    if file
        .metadata()
        .map_err(|_| GeolocationError::DatabaseRead)?
        .len()
        > limit
    {
        return Err(GeolocationError::DatabaseTooLarge { limit });
    }

    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| GeolocationError::DatabaseRead)?;

    if bytes.len() as u64 > limit {
        return Err(GeolocationError::DatabaseTooLarge { limit });
    }

    Ok(bytes)
}

fn normalize_country_code(value: &str) -> Option<String> {
    if value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        Some(value.to_ascii_uppercase())
    } else {
        None
    }
}

fn bounded_string(value: &str, limit: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    if end == 0 {
        None
    } else {
        Some(value[..end].to_owned())
    }
}

fn valid_latitude(value: f64) -> Option<f64> {
    (value.is_finite() && (-90.0..=90.0).contains(&value)).then_some(value)
}

fn valid_longitude(value: f64) -> Option<f64> {
    (value.is_finite() && (-180.0..=180.0).contains(&value)).then_some(value)
}

struct BoundedCache {
    entries: BTreeMap<IpAddr, CacheEntry>,
    capacity: usize,
}

struct CacheEntry {
    result: Option<GeolocationInfo>,
    expires_at: Instant,
}

impl BoundedCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            capacity,
        }
    }

    fn get(&mut self, address: IpAddr, now: Instant) -> Option<Option<GeolocationInfo>> {
        match self.entries.get(&address) {
            Some(entry) if entry.expires_at > now => return Some(entry.result.clone()),
            Some(_) => {
                self.entries.remove(&address);
            }
            None => {}
        }

        None
    }

    fn insert(&mut self, address: IpAddr, result: Option<GeolocationInfo>, expires_at: Instant) {
        if !self.entries.contains_key(&address) && self.entries.len() >= self.capacity {
            if let Some(first_key) = self.entries.keys().next().copied() {
                self.entries.remove(&first_key);
            }
        }

        self.entries
            .insert(address, CacheEntry { result, expires_at });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(last: u8) -> IpAddr {
        IpAddr::from([192, 0, 2, last])
    }

    fn country(code: &str) -> Option<GeolocationInfo> {
        Some(GeolocationInfo {
            country_code: Some(code.to_owned()),
            ..GeolocationInfo::default()
        })
    }

    #[test]
    fn cache_hits_negative_results_and_expires_entries() {
        let now = Instant::now();
        let mut cache = BoundedCache::new(2);
        cache.insert(address(1), None, now + Duration::from_secs(5));

        assert!(cache.get(address(1), now) == Some(None));
        assert!(cache
            .get(address(1), now + Duration::from_secs(6))
            .is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_capacity_is_never_exceeded() {
        let now = Instant::now();
        let mut cache = BoundedCache::new(1);
        cache.insert(address(1), country("FR"), now + Duration::from_secs(60));
        cache.insert(address(2), country("SE"), now + Duration::from_secs(60));

        assert_eq!(cache.len(), 1);
        assert!(cache.get(address(1), now).is_none());
        assert!(cache.get(address(2), now) == Some(country("SE")));
    }

    #[test]
    fn field_selection_and_input_normalization_are_enforced() {
        let mut record = geoip2::City::default();
        record.country.iso_code = Some("fr");
        record.country.names.english = Some("France");
        record.city.names.english = Some("Paris");
        record.location.latitude = Some(f64::NAN);
        record.location.longitude = Some(200.0);

        let country_only = map_record(record.clone(), GeolocationFields::default(), 256).unwrap();
        assert_eq!(country_only.country_code.as_deref(), Some("FR"));
        assert_eq!(country_only.country.as_deref(), Some("France"));
        assert!(country_only.city.is_none());
        assert!(country_only.latitude.is_none());
        assert!(country_only.longitude.is_none());

        let all_fields = map_record(record, GeolocationFields::all(), 256).unwrap();
        assert_eq!(all_fields.city.as_deref(), Some("Paris"));
        assert!(all_fields.latitude.is_none());
        assert!(all_fields.longitude.is_none());
    }

    #[test]
    fn strings_are_bounded_at_utf8_boundaries() {
        assert_eq!(bounded_string("  Paris  ", 256).as_deref(), Some("Paris"));
        assert_eq!(bounded_string("É Paris", 1), None);
        assert_eq!(bounded_string("É Paris", 2).as_deref(), Some("É"));
        assert_eq!(bounded_string("   ", 256), None);
        assert_eq!(normalize_country_code("se").as_deref(), Some("SE"));
        assert_eq!(normalize_country_code("fictional"), None);
    }
}
