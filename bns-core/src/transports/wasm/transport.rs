use super::helper::RtcSessionDescriptionWrapper;
use crate::channels::wasm::CbChannel;
use crate::ecc::SecretKey;
use crate::message::Encoded;
use crate::message::MessageRelay;
use crate::message::MessageRelayMethod;
use crate::transports::helper::Promise;
use crate::transports::helper::TricklePayload;
use crate::types::channel::Channel;
use crate::types::channel::Event;
use crate::types::ice_transport::IceCandidate;
use crate::types::ice_transport::IceServer;
use crate::types::ice_transport::IceTransport;
use crate::types::ice_transport::IceTransportCallback;
use crate::types::ice_transport::IceTrickleScheme;
use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::lock::Mutex as FuturesMutex;
use log::info;
use serde::Serialize;
use serde_json;
use std::sync::Arc;
use std::sync::Mutex;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use wasm_bindgen_futures::JsFuture;
use web3::types::Address;
use web_sys::MessageEvent;
use web_sys::RtcConfiguration;
use web_sys::RtcDataChannel;
use web_sys::RtcDataChannelEvent;
use web_sys::RtcIceCandidate;
use web_sys::RtcIceCandidateInit;
use web_sys::RtcIceConnectionState;
use web_sys::RtcIceGatheringState;
use web_sys::RtcPeerConnection;
use web_sys::RtcPeerConnectionIceEvent;
use web_sys::RtcSdpType;
use web_sys::RtcSessionDescription;
use web_sys::RtcSessionDescriptionInit;

type EventSender = Arc<FuturesMutex<mpsc::Sender<Event>>>;

#[derive(Clone)]
pub struct WasmTransport {
    connection: Option<Arc<RtcPeerConnection>>,
    pending_candidates: Arc<Mutex<Vec<RtcIceCandidate>>>,
    channel: Option<Arc<RtcDataChannel>>,
    event_sender: EventSender,
}

#[async_trait(?Send)]
impl IceTransport<Event, CbChannel<Event>> for WasmTransport {
    type Connection = RtcPeerConnection;
    type Candidate = RtcIceCandidate;
    type Sdp = RtcSessionDescription;
    type DataChannel = RtcDataChannel;
    type IceConnectionState = RtcIceConnectionState;
    type Msg = JsValue;

    fn new(event_sender: EventSender) -> Self {
        Self {
            connection: None,
            pending_candidates: Arc::new(Mutex::new(vec![])),
            channel: None,
            event_sender,
        }
    }

    async fn start(&mut self, ice_server: &IceServer) -> Result<&Self> {
        let mut config = RtcConfiguration::new();
        let ice_servers: js_sys::Array = js_sys::Array::of1(&ice_server.clone().into());
        config.ice_servers(&ice_servers.into());
        self.connection = RtcPeerConnection::new_with_configuration(&config)
            .ok()
            .as_ref()
            .map(|c| Arc::new(c.to_owned()));
        self.setup_channel("bns").await;
        return Ok(self);
    }

    async fn close(&self) -> Result<()> {
        if let Some(pc) = self.get_peer_connection().await {
            pc.close()
        }
        Ok(())
    }

    async fn ice_connection_state(&self) -> Option<Self::IceConnectionState> {
        self.get_peer_connection()
            .await
            .map(|pc| pc.ice_connection_state())
    }

    async fn is_connected(&self) -> bool {
        self.ice_connection_state()
            .await
            .map(|s| s == RtcIceConnectionState::Connected)
            .unwrap_or(false)
    }

    async fn get_peer_connection(&self) -> Option<Arc<Self::Connection>> {
        self.connection.as_ref().map(|c| Arc::clone(c))
    }

    async fn get_pending_candidates(&self) -> Vec<Self::Candidate> {
        self.pending_candidates.lock().unwrap().to_vec()
    }

    async fn get_answer(&self) -> Result<Self::Sdp> {
        match self.get_peer_connection().await {
            Some(c) => {
                let promise = c.create_answer();
                match JsFuture::from(promise).await {
                    Ok(answer) => {
                        self.set_local_description(RtcSessionDescriptionWrapper::from(
                            answer.to_owned(),
                        ))
                        .await?;
                        let promise = self.gather_complete_promise().await?;
                        promise.await?;
                        Ok(answer.into())
                    }
                    Err(_) => Err(anyhow!("Failed to get answer")),
                }
            }
            None => Err(anyhow!("cannot get connection")),
        }
    }

    async fn get_offer(&self) -> Result<Self::Sdp> {
        match self.get_peer_connection().await {
            Some(c) => {
                let promise = c.create_offer();
                match JsFuture::from(promise).await {
                    Ok(offer) => {
                        self.set_local_description(RtcSessionDescriptionWrapper::from(
                            offer.to_owned(),
                        ))
                        .await?;
                        let promise = self.gather_complete_promise().await?;
                        promise.await?;
                        Ok(offer.into())
                    }
                    Err(_) => Err(anyhow!("cannot get offer")),
                }
            }
            None => Err(anyhow!("cannot get connection")),
        }
    }

    async fn get_offer_str(&self) -> Result<String> {
        Ok(self.get_offer().await?.sdp())
    }

    async fn get_answer_str(&self) -> Result<String> {
        Ok(self.get_answer().await?.sdp())
    }

    async fn get_data_channel(&self) -> Option<Arc<Self::DataChannel>> {
        self.channel.as_ref().map(|c| Arc::clone(&c))
    }

    async fn send_message<T>(&self, msg: T) -> Result<()>
    where
        T: Serialize + Send,
    {
        let data = serde_json::to_string(&msg)?;
        match self.get_data_channel().await {
            Some(cnn) => cnn.send_with_str(&data).map_err(|e| anyhow!("{:?}", e)),
            None => Err(anyhow!("data channel may not ready")),
        }
    }

    async fn set_local_description<T>(&self, desc: T) -> Result<()>
    where
        T: Into<Self::Sdp>,
    {
        match &self.get_peer_connection().await {
            Some(c) => {
                let sdp: Self::Sdp = desc.into();
                let mut offer_obj = RtcSessionDescriptionInit::new(sdp.type_());
                offer_obj.sdp(&sdp.sdp());
                let promise = c.set_local_description(&offer_obj);
                match JsFuture::from(promise).await {
                    Ok(_) => Ok(()),
                    Err(_) => Err(anyhow!("Failed to set remote description")),
                }
            }
            None => Err(anyhow!("Failed on getting connection")),
        }
    }

    async fn set_remote_description<T>(&self, desc: T) -> Result<()>
    where
        T: Into<Self::Sdp>,
    {
        match &self.get_peer_connection().await {
            Some(c) => {
                let sdp: Self::Sdp = desc.into();
                let mut offer_obj = RtcSessionDescriptionInit::new(sdp.type_());
                let sdp = &sdp.sdp();
                offer_obj.sdp(&sdp);
                let promise = c.set_remote_description(&offer_obj);

                match JsFuture::from(promise).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        info!("failed to set remote desc");
                        info!("{:?}", e);
                        Err(anyhow!("Failed to set remote description"))
                    }
                }
            }
            None => Err(anyhow!("Failed on getting connection")),
        }
    }

    async fn add_ice_candidate(&self, candidate: IceCandidate) -> Result<()> {
        match &self.get_peer_connection().await {
            Some(c) => {
                let cand: RtcIceCandidateInit = candidate.clone().into();
                let promise = c.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&cand));

                match JsFuture::from(promise).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        log::error!("failed to add ice candate");
                        Err(anyhow!(
                            "Failed to add ice candidate:: {:?}, Error:: {:?}",
                            &candidate,
                            &e
                        ))
                    }
                }
            }
            None => Err(anyhow!("Failed on getting connection")),
        }
    }
}

impl WasmTransport {
    pub async fn setup_channel(&mut self, name: &str) -> &Self {
        if let Some(conn) = &self.connection {
            let channel = conn.create_data_channel(&name);
            self.channel = Some(Arc::new(channel));
        }
        return self;
    }
}

#[async_trait(?Send)]
impl IceTransportCallback<Event, CbChannel<Event>> for WasmTransport {
    type OnLocalCandidateHdlrFn = Box<dyn FnMut(RtcPeerConnectionIceEvent) -> ()>;
    type OnDataChannelHdlrFn = Box<dyn FnMut(RtcDataChannelEvent) -> ()>;

    async fn apply_callback(&self) -> Result<&Self> {
        match &self.get_peer_connection().await {
            Some(c) => {
                let on_ice_candidate_callback = Closure::wrap(self.on_ice_candidate().await);
                let on_data_channel_callback = Closure::wrap(self.on_data_channel().await);

                c.set_onicecandidate(Some(on_ice_candidate_callback.as_ref().unchecked_ref()));
                c.set_ondatachannel(Some(on_data_channel_callback.as_ref().unchecked_ref()));
                on_ice_candidate_callback.forget();
                //on_peer_connection_state_change_callback.forget();
                on_data_channel_callback.forget();
                Ok(self)
            }
            None => {
                log::error!("cannot get connection");
                Err(anyhow!("Failed on getting connection"))
            }
        }
    }
    async fn on_ice_candidate(&self) -> Self::OnLocalCandidateHdlrFn {
        let peer_connection = self.get_peer_connection().await;
        let pending_candidates = Arc::clone(&self.pending_candidates);
        log::info!("binding ice candidate callback");
        box move |ev: RtcPeerConnectionIceEvent| {
            log::info!("ice_Candidate {:?}", ev.candidate());
            let mut candidates = pending_candidates.lock().unwrap();
            let peer_connection = peer_connection.clone();
            if let Some(candidate) = ev.candidate() {
                if let Some(_) = peer_connection {
                    candidates.push(candidate.clone());
                    println!("Candidates Number: {:?}", candidates.len());
                }
            }
        }
    }

    async fn on_data_channel(&self) -> Self::OnDataChannelHdlrFn {
        let event_sender = self.event_sender.clone();
        box move |ev: RtcDataChannelEvent| {
            let event_sender = Arc::clone(&event_sender);
            let ch = ev.channel();
            let on_message_cb = Closure::wrap(
                (box move |ev: MessageEvent| {
                    let event_sender = Arc::clone(&event_sender);
                    match ev.data().as_string() {
                        Some(msg) => spawn_local(async move {
                            let event_sender = Arc::clone(&event_sender);
                            if CbChannel::send(&event_sender, Event::ReceiveMsg(msg.into_bytes()))
                                .await
                                .is_err()
                            {
                                log::error!("Failed on handle msg");
                            }
                        }),
                        None => {
                            log::error!("Failed on handle msg");
                        }
                    }
                }) as Box<dyn FnMut(MessageEvent)>,
            );
            ch.set_onmessage(Some(on_message_cb.as_ref().unchecked_ref()));
            on_message_cb.forget();
        }
    }
}

#[async_trait(?Send)]
impl IceTrickleScheme<Event, CbChannel<Event>> for WasmTransport {
    // https://datatracker.ietf.org/doc/html/rfc5245
    // 1. Send (SdpOffer, IceCandidates) to remote
    // 2. Recv (SdpAnswer, IceCandidate) From Remote

    type SdpType = RtcSdpType;

    async fn get_handshake_info(&self, key: SecretKey, kind: Self::SdpType) -> Result<Encoded> {
        log::trace!("prepareing handshake info {:?}", kind);
        let sdp = match kind {
            RtcSdpType::Answer => self.get_answer().await?,
            RtcSdpType::Offer => self.get_offer().await?,
            _ => {
                return Err(anyhow!("unsupport sdp type"));
            }
        };
        let local_candidates_json: Vec<IceCandidate> = self
            .get_pending_candidates()
            .await
            .iter()
            .map(|c| c.clone().to_json().into_serde::<IceCandidate>().unwrap())
            .collect();
        let data = TricklePayload {
            sdp: serde_json::to_string(&RtcSessionDescriptionWrapper::from(sdp))?,
            candidates: local_candidates_json,
        };
        log::trace!("prepared hanshake info :{:?}", data);
        let resp = MessageRelay::new(data, &key, None, None, None, MessageRelayMethod::SEND)?;
        Ok(resp.try_into()?)
    }

    async fn register_remote_info(&self, data: Encoded) -> anyhow::Result<Address> {
        let data: MessageRelay<TricklePayload> = data.try_into()?;
        log::debug!("register remote info: {:?}", &data);

        match data.verify() {
            true => {
                let sdp: RtcSessionDescriptionWrapper = data.data.sdp.try_into()?;
                self.set_remote_description(sdp.to_owned()).await?;
                log::trace!("setting remote candidate");
                for c in data.data.candidates {
                    log::debug!("add remote candiates: {:?}", c);
                    self.add_ice_candidate(c.clone()).await?;
                }
                Ok(data.addr)
            }
            _ => {
                log::error!("cannot verify message sig");
                return Err(anyhow!("failed on verify message sigature"));
            }
        }
    }

    async fn wait_for_connected(&self) -> anyhow::Result<()> {
        let promise = self.connect_success_promise().await?;
        promise.await
    }
}

impl WasmTransport {
    pub async fn gather_complete_promise(&self) -> Result<Promise> {
        match self.get_peer_connection().await {
            Some(conn) => {
                let promise = Promise::default();
                let state = Arc::clone(&promise.state());
                let conn_clone = Arc::clone(&conn);
                let callback =
                    Closure::wrap(Box::new(move || match conn_clone.ice_gathering_state() {
                        RtcIceGatheringState::Complete => {
                            let state = Arc::clone(&state);
                            let mut s = state.lock().unwrap();
                            if let Some(w) = s.waker.take() {
                                w.wake();
                                s.completed = true;
                                s.successed = Some(true);
                            }
                        }
                        x => {
                            log::trace!("gather status: {:?}", x)
                        }
                    }) as Box<dyn FnMut()>);
                conn.set_onicegatheringstatechange(Some(callback.as_ref().unchecked_ref()));
                callback.forget();
                Ok(promise)
            }
            None => Err(anyhow!("cannot get connection")),
        }
    }

    pub async fn connect_success_promise(&self) -> Result<Promise> {
        match self.get_peer_connection().await {
            Some(conn) => {
                let promise = Promise::default();
                let state = Arc::clone(&promise.state());
                let callback = Closure::wrap(Box::new(move |st: RtcIceConnectionState| match st {
                    RtcIceConnectionState::Connected => {
                        let mut s = state.lock().unwrap();
                        if let Some(w) = s.waker.take() {
                            w.wake();
                            s.completed = true;
                            s.successed = Some(true);
                        }
                    }
                    RtcIceConnectionState::Failed => {
                        let mut s = state.lock().unwrap();
                        if let Some(w) = s.waker.take() {
                            w.wake();
                            s.completed = true;
                            s.successed = Some(false);
                        }
                    }
                    _ => {
                        log::trace!("Connect State changed to {:?}", st);
                    }
                })
                    as Box<dyn FnMut(RtcIceConnectionState)>);
                conn.set_oniceconnectionstatechange(Some(callback.as_ref().unchecked_ref()));
                callback.forget();
                Ok(promise)
            }
            None => Err(anyhow!("cannot get connection")),
        }
    }
}
