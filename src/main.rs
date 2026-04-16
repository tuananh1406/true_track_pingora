mod config;
mod errors;
use std::path::Path;
use tracing_appender::non_blocking;
use tracing_appender::rolling;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::{filter::EnvFilter, prelude::*};
mod structs;
mod tls;
mod utils;
use bytes::Bytes;
use openssl::ssl::SslAlert;
use openssl::ssl::SslRef;
use pingora::proxy::http_proxy_service_with_name;
use pingora::services::background::GenBackgroundService;
use serde_json::json;
use utils::build_cluster_service;

use async_trait::async_trait;
use http::StatusCode;
use pingora::{http::ResponseHeader, prelude::*};
use std::sync::Arc;
use tracing::info;

use pingora::listeners::tls::TlsSettings;

use crate::structs::Router;
use crate::structs::RouterService;
use crate::structs::ServerResult;
use crate::utils::precheck_missing_folders;

use self::config::Config;

fn main() -> ServerResult<()> {
    precheck_missing_folders()?;

    let cfg = Config::load("./proxy/true_track_api.toml")?;
    // tracing_subscriber::fmt()
    // .with_max_level(tracing::Level::INFO)
    // .with_line_number(true)
    // .with_file(true)
    // .with_target(false)
    // .init();
    // 1. Cấu hình xoay vòng log hàng ngày
    let file = rolling::daily(cfg.log_folder, "proxy.log");
    let (non_blocking, _guard) = non_blocking(file);
    let log_to_file_layer = tracing_subscriber::fmt::layer()
        .with_timer(ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f %z".to_string()))
        .with_target(true)
        .with_line_number(true)
        .with_ansi(false)
        .with_writer(non_blocking);

    let subcriber = tracing_subscriber::registry()
        .with(log_to_file_layer)
        // .with(EnvFilter::from_default_env())
        .with(EnvFilter::new("info"));

    tracing::subscriber::set_global_default(subcriber)?;

    let mut routers: Vec<RouterService> = Vec::new();
    let mut services: Vec<GenBackgroundService<LoadBalancer<RoundRobin>>> = Vec::new();

    let mut my_server = Server::new(Some(Opt::parse_args()))?;
    my_server.bootstrap();
    let mut cert_configs: Vec<tls::CertificateConfig> = Vec::new();
    info!("Starting True Track Proxy Server...");

    for proxy in &cfg.proxies {
        let cluster = build_cluster_service::<RoundRobin>(
            &proxy
                .upstreams
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>(),
            proxy.is_http_healthcheck,
            &proxy.health_check_host,
            &proxy.health_check_path,
        );
        routers.push(RouterService {
            host_name: proxy.name.clone(),
            service: cluster.task(),
        });
        services.push(cluster);
        cert_configs.push(tls::CertificateConfig {
            cert_path: proxy.tls_cert.clone(),
            key_path: proxy.tls_key.clone(),
        });
    }

    let certificates = tls::Certificates::new(&cert_configs);
    let mut tls_settings = TlsSettings::intermediate(
        &certificates.default_cert_path,
        &certificates.default_key_path,
    )?;
    tls_settings.enable_h2();
    tls_settings.set_servername_callback(move |ssl_ref: &mut SslRef, ssl_alert: &mut SslAlert| {
        certificates.server_name_callback(ssl_ref, ssl_alert)
    });
    tls_settings.set_alpn_select_callback(tls::prefer_h2);
    tls_settings.set_ca_file(Path::new(&cfg.cf_root_path))?;

    let router = Router {
        routes: routers,
        allow_hosts: cfg.allow_hosts.clone(),
    };
    let mut router_services =
        http_proxy_service_with_name(&my_server.configuration, router, "Personal API Proxy");
    router_services.add_tcp(format!("0.0.0.0:{}", 80).as_str());

    router_services.add_tls_with_settings(format!("0.0.0.0:{}", 443).as_str(), None, tls_settings);

    my_server.add_service(router_services);
    for service in services {
        my_server.add_service(service);
    }
    my_server.run_forever();
}

pub struct LB(Arc<LoadBalancer<RoundRobin>>);
pub struct LBContext {
    body_size: usize,
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = LBContext;
    fn new_ctx(&self) -> Self::CTX {
        LBContext { body_size: 0 }
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        let client_addr = session.client_addr().unwrap();
        upstream_request
            .insert_header("X-Forwarded-For", client_addr.to_string())
            .unwrap();
        upstream_request
            .insert_header(
                "X-Forwarded-Proto",
                session.req_header().uri.scheme_str().unwrap_or("http"),
            )
            .unwrap();
        upstream_request
            .insert_header(
                "X-Forwarded-Host",
                session
                    .req_header()
                    .uri
                    .host()
                    .unwrap_or("unknown-host")
                    .to_string(),
            )
            .unwrap();
        upstream_request
            .insert_header(
                "Host",
                session
                    .req_header()
                    .uri
                    .host()
                    .unwrap_or("unknown-host")
                    .to_string(),
            )
            .unwrap();
        Ok(())
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut LBContext,
    ) -> Result<Box<HttpPeer>> {
        let upstream = self.0.select(b"", 256).unwrap();

        let peer = Box::new(HttpPeer::new(upstream, false, "".to_string()));
        info!("Upstream peer selected: {}", peer._address);
        Ok(peer)
    }

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let request = session.req_header();
        info!(
            "request from {} to {}",
            session.client_addr().unwrap(),
            request.uri
        );
        // info!("request headers: {request:#?}");
        let host = request.uri.host();
        match host {
            None => {
                // info!(
                //     "request from {} to {}",
                //     session.client_addr().unwrap(),
                //     request.uri
                // );
                let mut resp = ResponseHeader::build(StatusCode::BAD_GATEWAY, None).unwrap();
                let msg = json!({ "message": "Host invalid" });
                let bytes = Bytes::from(msg.to_string());
                resp.insert_header("Content-Length", bytes.len()).unwrap();
                session.write_response_header(Box::new(resp), true).await?;
                session.write_response_body(None, true).await?;
                return Ok(true);
            }
            Some(host) => {
                let scheme = request.uri.scheme_str().unwrap_or("http");
                if scheme != "https" {
                    let redirect_to = format!(
                        "https://{}{}",
                        host,
                        request
                            .uri
                            .path_and_query()
                            .map(|x| x.as_str())
                            .unwrap_or("/")
                    );
                    let mut resp = ResponseHeader::build(StatusCode::FOUND, None).unwrap();
                    resp.insert_header("Location", &redirect_to).unwrap();
                    resp.insert_header("Content-Length", "0").unwrap();
                    resp.insert_header("Redirect-From", scheme).unwrap();
                    session.write_response_header(Box::new(resp), true).await?;
                    session.write_response_body(None, true).await?;
                    return Ok(true);
                }
                let server_port = session.server_addr().unwrap().as_inet().unwrap().port();

                if scheme != "https" && server_port == 443 {
                    let redirect_to = "https://huutuananh.com/error/500";
                    let mut resp = ResponseHeader::build(StatusCode::FOUND, None).unwrap();
                    resp.insert_header("Location", redirect_to).unwrap();
                    resp.insert_header("Content-Length", "0").unwrap();
                    resp.insert_header("Redirect-From", scheme).unwrap();
                    session.write_response_header(Box::new(resp), true).await?;
                    session.write_response_body(None, true).await?;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    async fn request_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(chunk) = body.as_ref() {
            // Tăng tổng số byte đã nhận
            ctx.body_size += chunk.len();
        }

        if ctx.body_size > 120 * 1024 * 1024 {
            info!(
                "Request payload too large from {}",
                session.client_addr().unwrap()
            );
            // Max body size 120mb
            let mut resp = ResponseHeader::build(StatusCode::PAYLOAD_TOO_LARGE, None).unwrap();
            let msg = json!({ "message": "payload too large (max: 120mb)" });
            let bytes = Bytes::from(msg.to_string());
            resp.insert_header("Content-Length", bytes.len()).unwrap();
            session.write_response_header(Box::new(resp), true).await?;
            session.write_response_body(Some(bytes), true).await?;
        }
        Ok(())
    }
}
