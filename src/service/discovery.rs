use super::{http_error::HttpError, result::HttpResult};
use anyhow::anyhow;
use axum::extract::{Extension, Query, RawBody};
use bns_core::session::SessionManager;
use bns_core::swarm::{Swarm, TransportManager};
use bns_core::types::ice_transport::IceTrickleScheme;

use std::collections::HashMap;
use std::sync::Arc;
use webrtc::peer_connection::sdp::sdp_type::RTCSdpType;

pub async fn handshake_handler(
    RawBody(body): RawBody,
    Extension(swarm): Extension<Arc<Swarm>>,
    Extension(session): Extension<SessionManager>,
) -> HttpResult<String> {
    log::info!("Incoming body: {:?}", body);

    let payload = hyper::body::to_bytes(body).await?;
    match handshake(swarm, session.clone(), payload.to_vec()).await {
        Ok(data) => Ok(data),
        Err(e) => {
            log::error!("failed to handle trickle scheme: {:?}", e);
            Err(HttpError::Internal)
        }
    }
}

pub async fn connect_handler(
    Query(params): Query<HashMap<String, String>>,
    Extension(swarm): Extension<Arc<Swarm>>,
    Extension(session): Extension<SessionManager>,
) -> HttpResult<String> {
    let node = params.get("node").ok_or(HttpError::BadRequest)?;

    match trickle_forward(swarm, session.clone(), node.into()).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            log::error!("failed to trickle forward {:?}", e);
            Err(HttpError::Internal)
        }
    }
}

async fn handshake(
    swarm: Arc<Swarm>,
    session: SessionManager,
    data: Vec<u8>,
) -> anyhow::Result<String> {
    // get offer from remote and send answer back

    let transport = swarm.new_transport().await?;
    match transport
        .register_remote_info(String::from_utf8(data)?.into())
        .await
    {
        Ok(addr) => {
            let resp = transport
                .get_handshake_info(session.clone(), RTCSdpType::Answer)
                .await;
            match resp {
                Ok(info) => {
                    swarm.register(&addr, Arc::clone(&transport)).await?;
                    anyhow::Result::Ok(info.to_string())
                }
                Err(e) => {
                    log::error!("failed to get handshake info: {:?}", e);
                    Err(anyhow!(e))
                }
            }
        }
        Err(e) => {
            log::error!("failed to register {:?}", e);
            Err(anyhow!(e))
        }
    }
}

pub async fn trickle_forward(
    swarm: Arc<Swarm>,
    session: SessionManager,
    node: String,
) -> anyhow::Result<String> {
    // request remote offer and sand answer to remote
    let client = reqwest::Client::new();
    let transport = swarm.new_transport().await?;
    let req = transport
        .get_handshake_info(session.clone(), RTCSdpType::Offer)
        .await?;
    log::debug!(
        "sending offer and candidate {:?} to {:?}",
        req.to_owned(),
        &node
    );

    match client
        .post(node)
        .body(req.to_string())
        .send()
        .await?
        .text()
        .await
    {
        Ok(resp) => {
            log::debug!("get answer and candidate from remote");
            let addr = transport.register_remote_info(resp.into()).await?;
            swarm.register(&addr, Arc::clone(&transport)).await?;
            Ok("ok".to_string())
        }
        Err(e) => {
            log::error!("someting wrong {:?}", e);
            anyhow::Result::Err(anyhow!(e))
        }
    }
}
