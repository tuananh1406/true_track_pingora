use crate::errors::ServerError;
use async_trait::async_trait;
use bytes::Bytes;
use http::StatusCode;
use serde_json::json;
use std::sync::Arc;
// use tracing::info;

pub type ServerResult<T> = Result<T, ServerError>;

use pingora::{
    http::{RequestHeader, ResponseHeader},
    lb::LoadBalancer,
    prelude::*,
    proxy::ProxyHttp,
};

pub(crate) struct RouterService {
    pub host_name: String,
    pub service: Arc<LoadBalancer<RoundRobin>>,
}

pub(crate) struct Router {
    pub allow_hosts: Vec<String>,
    pub routes: Vec<RouterService>,
}

pub(crate) struct RouterContext {
    body_size: usize,
}

#[async_trait]
impl ProxyHttp for Router {
    type CTX = RouterContext;
    fn new_ctx(&self) -> Self::CTX {
        RouterContext { body_size: 0 }
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut RouterContext,
    ) -> Result<Box<HttpPeer>> {
        let host = session.req_header().uri.host().unwrap_or("");
        let cluster = self
            .routes
            .iter()
            .find(|service| host.ends_with(&service.host_name))
            .map(|service| service.service.clone())
            .unwrap();
        let upstream = cluster.select(b"", 256).unwrap();

        let peer = Box::new(HttpPeer::new(upstream, false, host.to_string()));
        Ok(peer)
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

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let request = session.req_header();
        // info!(
        //     "request from {} to {}",
        //     session.client_addr().unwrap(),
        //     request.uri
        // );
        // info!("request headers: {request:#?}");
        let host = request.uri.host();
        match host {
            None => {
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
                    let mut resp = ResponseHeader::build(StatusCode::FORBIDDEN, None).unwrap();
                    let msg = json!({ "message": "Http is not allow" });
                    let bytes = Bytes::from(msg.to_string());
                    resp.insert_header("Content-Length", bytes.len()).unwrap();
                    session.write_response_header(Box::new(resp), true).await?;
                    session.write_response_body(Some(bytes), true).await?;
                    return Ok(true);
                }
                // info!("Host: {}", host);
                if !&self
                    .allow_hosts
                    .iter()
                    .any(|allow_host| host.ends_with(allow_host))
                {
                    let mut resp = ResponseHeader::build(StatusCode::FORBIDDEN, None).unwrap();
                    let msg = json!({ "message": "Host is not allow" });
                    let bytes = Bytes::from(msg.to_string());
                    resp.insert_header("Content-Length", bytes.len()).unwrap();
                    session.write_response_header(Box::new(resp), true).await?;
                    session.write_response_body(Some(bytes), true).await?;
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
