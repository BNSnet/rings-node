use crate::dht::{Did, PeerRing};
use crate::err::{Error, Result};
use crate::message::payload::{MessageRelay, MessageRelayMethod};
use crate::message::types::ActorContext;
use crate::message::types::Message;
use crate::message::types::MessageActor;
use crate::swarm::Swarm;
use async_recursion::async_recursion;
use async_trait::async_trait;
use futures::lock::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use web3::types::Address;

pub mod connection;
pub mod storage;

use connection::TChordConnection;
use storage::TChordStorage;

#[derive(Clone)]
pub struct MessageHandler {
    dht: Arc<Mutex<PeerRing>>,
    swarm: Arc<Swarm>,
}

impl MessageHandler {
    pub fn new(dht: Arc<Mutex<PeerRing>>, swarm: Arc<Swarm>) -> Self {
        Self { dht, swarm }
    }

    pub async fn send_message(
        &self,
        address: &Address,
        to_path: Option<VecDeque<Did>>,
        from_path: Option<VecDeque<Did>>,
        method: MessageRelayMethod,
        message: impl MessageActor,
    ) -> Result<()> {
        // TODO: diff ttl for each message?
        let payload = MessageRelay::new(
            message,
            &self.swarm.session(),
            None,
            to_path,
            from_path,
            method,
        )?;
        self.swarm.send_message(address, payload).await
    }

    pub async fn handle_message_relay(
        &self,
        relay: MessageRelay<impl MessageActor>,
        prev: Did,
    ) -> Result<()> {
        let ctx = ActorContext {
            relay: relay.clone(),
            prev,
        };
        relay.data.handler(self, ctx).await
    }

    /// This method is required because web-sys components is not `Send`
    /// which means a listening loop cannot running concurrency.
    pub async fn listen_once(&self) -> Option<MessageRelay<Message>> {
        if let Some(relay_message) = self.swarm.poll_message().await {
            if !relay_message.verify() {
                log::error!("Cannot verify msg or it's expired: {:?}", relay_message);
            }
            let addr = relay_message.addr.into();
            if let Err(e) = self.handle_message_relay(relay_message.clone(), addr).await {
                log::error!("Error in handle_message: {}", e);
            }
            Some(relay_message)
        } else {
            None
        }
    }
}

#[cfg_attr(feature = "wasm", async_trait(?Send))]
#[cfg_attr(not(feature = "wasm"), async_trait)]
impl MessageActor for Message {
    async fn handler(&self, handler: &MessageHandler, ctx: ActorContext<Self>) -> Result<()> {
        #[cfg_attr(feature = "wasm", async_recursion(?Send))]
        #[cfg_attr(not(feature = "wasm"), async_recursion)]
        pub async fn inner(
            handler: &MessageHandler,
            relay: MessageRelay<Message>,
            prev: Did,
        ) -> Result<()> {
            let data = relay.data.clone();
            match data {
                Message::JoinDHT(msg) => handler.join_chord(relay, prev, msg).await,
                Message::ConnectNodeSend(msg) => handler.connect_node(relay, prev, msg).await,
                Message::ConnectNodeReport(msg) => handler.connected_node(relay, prev, msg).await,
                Message::AlreadyConnected(msg) => handler.already_connected(relay, prev, msg).await,
                Message::FindSuccessorSend(msg) => handler.find_successor(relay, prev, msg).await,
                Message::FindSuccessorReport(msg) => {
                    handler.found_successor(relay, prev, msg).await
                }
                Message::NotifyPredecessorSend(msg) => {
                    handler.notify_predecessor(relay, prev, msg).await
                }
                Message::NotifyPredecessorReport(msg) => {
                    handler.notified_predecessor(relay, prev, msg).await
                }
                Message::SearchVNode(msg) => handler.search_vnode(relay, prev, msg).await,
                Message::FoundVNode(msg) => handler.found_vnode(relay, prev, msg).await,
                Message::StoreVNode(msg) => handler.store_vnode(relay, prev, msg).await,
                Message::MultiCall(msg) => {
                    for message in msg.messages {
                        let payload = relay.map(&handler.swarm.session(), message.clone())?;
                        inner(handler, payload, prev).await.unwrap_or(());
                    }
                    Ok(())
                }
                x => Err(Error::MessageHandlerUnsupportMessageType(format!(
                    "{:?}",
                    x
                ))),
            }
        }
        let relay = ctx.relay;
        let prev = ctx.prev;
        inner(handler, relay, prev).await
    }
}

#[cfg(not(feature = "wasm"))]
mod listener {
    use super::MessageHandler;
    use crate::types::message::MessageListener;
    use async_trait::async_trait;
    use std::sync::Arc;

    use futures_util::pin_mut;
    use futures_util::stream::StreamExt;

    #[async_trait]
    impl MessageListener for MessageHandler {
        async fn listen(self: Arc<Self>) {
            let relay_messages = self.swarm.iter_messages();
            pin_mut!(relay_messages);
            while let Some(relay_message) = relay_messages.next().await {
                if relay_message.is_expired() || !relay_message.verify() {
                    log::error!("Cannot verify msg or it's expired: {:?}", relay_message);
                    continue;
                }
                let addr = relay_message.addr.into();
                if let Err(e) = self.handle_message_relay(relay_message, addr).await {
                    log::error!("Error in handle_message: {}", e);
                    continue;
                }
            }
        }
    }
}

#[cfg(feature = "wasm")]
mod listener {
    use super::MessageHandler;
    use crate::poll;
    use crate::types::message::MessageListener;
    use async_trait::async_trait;
    use std::sync::Arc;
    use wasm_bindgen_futures::spawn_local;

    #[async_trait(?Send)]
    impl MessageListener for MessageHandler {
        async fn listen(self: Arc<Self>) {
            let handler = Arc::clone(&self);
            let func = move || {
                let handler = Arc::clone(&handler);
                spawn_local(Box::pin(async move {
                    handler.listen_once().await;
                }));
            };
            poll!(func, 200);
        }
    }
}
