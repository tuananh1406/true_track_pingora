use std::time::Duration;

use http::Uri;
use pingora::{
    lb::{
        LoadBalancer,
        health_check::HttpHealthCheck,
        selection::{BackendIter, BackendSelection},
    },
    prelude::{TcpHealthCheck, background_service},
    services::background::GenBackgroundService,
};

use crate::structs::ServerResult;

#[allow(dead_code)]
pub fn build_cluster_service<S>(
    upstreams: &[&str],
    is_http_healthcheck: bool,
    health_check_host: &str,
    health_check_path: &str,
) -> GenBackgroundService<LoadBalancer<S>>
where
    S: BackendSelection + 'static,
    S::Iter: BackendIter,
{
    let mut cluster = LoadBalancer::try_from_iter(upstreams).unwrap();

    if is_http_healthcheck {
        let mut hc = HttpHealthCheck::new(health_check_host, false);
        let uri = Uri::try_from(health_check_path).unwrap();
        hc.req.set_uri(uri);
        cluster.set_health_check(Box::new(hc));
    } else {
        let hc = TcpHealthCheck::new();
        cluster.set_health_check(hc);
    }
    cluster.health_check_frequency = Some(Duration::from_secs(5));
    background_service(
        format!("{}_health_check", health_check_host).as_str(),
        cluster,
    )
}

pub fn precheck_missing_folders() -> ServerResult<()> {
    let must_have_folders = vec!["./tmp", "./logs"];
    must_have_folders.iter().try_for_each(|folder| {
        if !std::path::Path::new(folder).exists() {
            std::fs::create_dir_all(folder)?;
        }
        Ok(())
    })
}
