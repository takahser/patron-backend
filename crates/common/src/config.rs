use std::{net::SocketAddr, path::PathBuf};

use byte_unit::{n_gib_bytes, n_mib_bytes};
use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

#[cfg(feature = "logging")]
use tracing_subscriber::filter::LevelFilter;

/// Database configuration.
#[derive(Deserialize)]
pub struct Database {
    /// Database URL string.
    pub url: String,
}

/// HTTP server configuration.
#[derive(Deserialize)]
pub struct Server {
    /// Address, that HTTP server will listen on.
    pub address: SocketAddr,
}

#[cfg(feature = "logging")]
fn deserialize_from_str<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: std::str::FromStr,
    T::Err: std::error::Error,
    D: serde::de::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    std::str::FromStr::from_str(&s).map_err(serde::de::Error::custom)
}

#[cfg(feature = "logging")]
#[derive(Deserialize)]
pub struct Logging {
    /// Log level.
    #[serde(deserialize_with = "deserialize_from_str")]
    pub level: LevelFilter,
}

#[cfg(feature = "logging")]
impl Default for Logging {
    fn default() -> Self {
        Self {
            level: LevelFilter::WARN,
        }
    }
}

#[derive(Deserialize)]
pub struct Builder {
    /// Path in which contract builder will store all user artifacts.
    pub images_path: PathBuf,

    /// Total count of workers started for build processing.
    #[serde(default = "default_worker_count")]
    pub worker_count: usize,

    /// Max build duration value, in seconds.
    #[serde(default = "default_build_duration")]
    pub max_build_duration: u64,

    /// Max WASM blob size, in bytes.
    #[serde(default = "default_wasm_size_limit")]
    pub wasm_size_limit: usize,

    /// Max JSON metadata size, in bytes.
    #[serde(default = "default_metadata_size_limit")]
    pub metadata_size_limit: usize,

    /// Memory limit per build.
    #[serde(default = "default_memory_limit")]
    pub memory_limit: i64,

    /// Memory swap limit per build.
    /// This value should at least be equal to memory limit.
    #[serde(default = "default_memory_swap_limit")]
    pub memory_swap_limit: i64,

    /// Volume size available to each build.
    /// Accepts the same format as passed to fallocate command.
    #[serde(default = "default_volume_size")]
    pub volume_size: String,
}

fn default_worker_count() -> usize {
    1
}

fn default_build_duration() -> u64 {
    3600
}

fn default_wasm_size_limit() -> usize {
    n_mib_bytes!(5) as usize
}

fn default_metadata_size_limit() -> usize {
    n_mib_bytes!(1) as usize
}

fn default_memory_limit() -> i64 {
    n_gib_bytes!(8) as i64
}

fn default_memory_swap_limit() -> i64 {
    n_gib_bytes!(8) as i64
}

fn default_volume_size() -> String {
    String::from("8G")
}

#[derive(Deserialize)]
pub struct Storage {
    /// Access key identifier.
    pub access_key_id: String,

    /// Secret access key.
    pub secret_access_key: String,

    /// S3 region name.
    pub region: String,

    /// S3 endpoint URL.
    pub endpoint_url: String,

    /// S3 bucket name for source code archive storage.
    pub source_code_bucket: String,
}

#[derive(Deserialize)]
pub struct Config {
    /// General database configuration.
    pub database: Database,

    /// HTTP server configuration.
    #[serde(default)]
    pub server: Option<Server>,

    /// Logging configuration.
    #[cfg(feature = "logging")]
    #[serde(default)]
    pub logging: Logging,

    /// Contract builder configuration.
    #[serde(default)]
    pub builder: Option<Builder>,

    /// Storage configuration.
    pub storage: Storage,

    #[serde(default = "default_payments")]
    pub payments: bool,
}

fn default_payments() -> bool {
    false
}

impl Config {
    /// Create new config using default configuration file.
    pub fn new() -> Result<Self, figment::Error> {
        Figment::new()
            .merge(Toml::file("Config.toml"))
            .merge(Env::prefixed("CONFIG_").split("_"))
            .extract()
    }
}
