use serde::Deserialize;
use std::fs;

use crate::structs::ServerResult;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub proxies: Vec<ProxyConfig>,
    pub allow_hosts: Vec<String>,
    pub cf_root_path: String,
    pub log_folder: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfig {
    pub name: String,
    // pub domain: String,
    pub upstreams: Vec<String>,
    pub tls_cert: String,
    pub tls_key: String,
    pub is_http_healthcheck: bool,
    pub health_check_host: String,
    pub health_check_path: String,
    // pub tls_client_cert: String,
}

impl Config {
    pub fn load(filename: &str) -> ServerResult<Self> {
        let contents = fs::read_to_string(filename)?;
        Ok(toml::from_str(&contents)?)
    }
}
