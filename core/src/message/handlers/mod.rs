#![warn(missing_docs)]
/// Message Flow:
/// +---------+    +--------------------------------+
/// | Message | -> | MessageHandler.handler_payload |
/// +---------+    +--------------------------------+
///                 ||                            ||
///     +--------------------------+  +--------------------------+
///     | Builtin Message Callback |  |  Custom Message Callback |
///     +--------------------------+  +--------------------------+
use std::sync::Arc;

use async_recursion::async_recursion;
use async_trait::async_trait;

use super::CustomMessage;
use super::MaybeEncrypted;
use super::Message;
use super::MessagePayload;
use crate::dht::vnode::VirtualNode;
use crate::dht::Did;
use crate::dht::PeerRing;
use crate::err::Error;
use crate::err::Result;

/// Operator and Handler for Connection
pub mod connection;
/// Operator and Handler for CustomMessage
pub mod custom;
/// Operator and handler for DHT stablization
pub mod stabilization;
/// Operator and Handler for Storage
pub mod storage;
/// Operator and Handler for Subring
pub mod subring;

/// Trait of message callback.
#[cfg_attr(feature = "wasm", async_trait(?Send))]
#[cfg_attr(not(feature = "wasm"), async_trait)]
pub trait MessageCallback {
    /// Message handler for custom message
    async fn custom_message(
        &self,
        handler: &MessageHandler,
        ctx: &MessagePayload<Message>,
        msg: &MaybeEncrypted<CustomMessage>,
    );
    /// Message handler for builtin message
    async fn builtin_message(&self, handler: &MessageHandler, ctx: &MessagePayload<Message>);
}

/// Trait of message validator.
#[cfg_attr(feature = "wasm", async_trait(?Send))]
#[cfg_attr(not(feature = "wasm"), async_trait)]
pub trait MessageValidator {
    /// Externality validator
    async fn validate(
        &self,
        handler: &MessageHandler,
        ctx: &MessagePayload<Message>,
    ) -> Option<String>;
}

/// Boxed Callback, for non-wasm, it should be Sized, Send and Sync.
#[cfg(not(feature = "wasm"))]
pub type CallbackFn = Box<dyn MessageCallback + Send + Sync>;

/// Boxed Callback
#[cfg(feature = "wasm")]
pub type CallbackFn = Box<dyn MessageCallback>;

/// Boxed Validator
#[cfg(not(feature = "wasm"))]
pub type ValidatorFn = Box<dyn MessageValidator + Send + Sync>;

/// Boxed Validator, for non-wasm, it should be Sized, Send and Sync.
#[cfg(feature = "wasm")]
pub type ValidatorFn = Box<dyn MessageValidator>;

/// MessageHandlerEvent that will be handled by Swarm.
#[derive(Debug)]
pub enum MessageHandlerEvent {
    Connect(Did),
    Disconnect(Did),
    AnswerOffer,
    AcceptAnswer,
    ForwardPayload,
    JoinDHT(Did),
    SendDirectMessage(Message, Did),
    SendMessage(Message, Did),
    SendReportMessage(Message),
    ResetDestination(Did),
    SyncVNodeWithSuccessor(Did),
    StorageStore(VirtualNode),
}

/// MessageHandler will manage resources.
#[derive(Clone)]
pub struct MessageHandler {
    /// DHT implement chord algorithm.
    dht: Arc<PeerRing>,
    /// CallbackFn implement `customMessage` and `builtin_message`.
    callback: Arc<Option<CallbackFn>>,
    /// A specific validator implement ValidatorFn.
    validator: Arc<Option<ValidatorFn>>,
}

/// Generic trait for handle message ,inspired by Actor-Model.
#[cfg_attr(feature = "wasm", async_trait(?Send))]
#[cfg_attr(not(feature = "wasm"), async_trait)]
pub trait HandleMsg<T> {
    /// Message handler.
    async fn handle(
        &self,
        ctx: &MessagePayload<Message>,
        msg: &T,
    ) -> Result<Vec<MessageHandlerEvent>>;
}

impl MessageHandler {
    /// Create a new MessageHandler Instance.
    pub fn new(
        dht: Arc<PeerRing>,
        callback: Option<CallbackFn>,
        validator: Option<ValidatorFn>,
    ) -> Self {
        Self {
            dht,
            callback: Arc::new(callback),
            validator: Arc::new(validator),
        }
    }

    /// Invoke callback, which will be call after builtin handler.
    async fn invoke_callback(&self, payload: &MessagePayload<Message>) -> Result<()> {
        if let Some(ref cb) = *self.callback {
            match payload.data {
                Message::CustomMessage(ref msg) => {
                    if self.dht.did == payload.relay.destination {
                        tracing::debug!("INVOKE CUSTOM MESSAGE CALLBACK {}", &payload.tx_id);
                        cb.custom_message(self, payload, msg).await
                    }
                }
                _ => cb.builtin_message(self, payload).await,
            };
        } else if let Message::CustomMessage(ref msg) = payload.data {
            if self.dht.did == payload.relay.destination {
                tracing::warn!("No callback registered, skip invoke_callback of {:?}", msg);
            }
        }
        Ok(())
    }

    /// Validate message.
    async fn validate(&self, payload: &MessagePayload<Message>) -> Result<()> {
        if let Some(ref v) = *self.validator {
            v.validate(self, payload)
                .await
                .map(|info| Err(Error::InvalidMessage(info)))
                .unwrap_or(Ok(()))?;
        };
        Ok(())
    }

    /// Handle builtin message.
    #[cfg_attr(feature = "wasm", async_recursion(?Send))]
    #[cfg_attr(not(feature = "wasm"), async_recursion)]
    pub async fn handle_message(
        &self,
        payload: &MessagePayload<Message>,
    ) -> Result<Vec<MessageHandlerEvent>> {
        #[cfg(test)]
        {
            println!("{} got msg {}", self.dht.did, &payload.data);
        }
        tracing::debug!("START HANDLE MESSAGE: {} {}", &payload.tx_id, &payload.data);

        self.validate(payload).await?;

        let events = match &payload.data {
            Message::JoinDHT(ref msg) => self.handle(payload, msg).await,
            Message::LeaveDHT(ref msg) => self.handle(payload, msg).await,
            Message::ConnectNodeSend(ref msg) => self.handle(payload, msg).await,
            Message::ConnectNodeReport(ref msg) => self.handle(payload, msg).await,
            Message::FindSuccessorSend(ref msg) => self.handle(payload, msg).await,
            Message::FindSuccessorReport(ref msg) => self.handle(payload, msg).await,
            Message::NotifyPredecessorSend(ref msg) => self.handle(payload, msg).await,
            Message::NotifyPredecessorReport(ref msg) => self.handle(payload, msg).await,
            Message::SearchVNode(ref msg) => self.handle(payload, msg).await,
            Message::FoundVNode(ref msg) => self.handle(payload, msg).await,
            Message::SyncVNodeWithSuccessor(ref msg) => self.handle(payload, msg).await,
            Message::OperateVNode(ref msg) => self.handle(payload, msg).await,
            Message::CustomMessage(ref msg) => self.handle(payload, msg).await,
        }?;

        tracing::debug!("INVOKE CALLBACK {}", &payload.tx_id);
        if let Err(e) = self.invoke_callback(payload).await {
            tracing::warn!("invoke callback error: {}", e);
        }

        tracing::debug!("FINISH HANDLE MESSAGE {}", &payload.tx_id);
        Ok(events)
    }
}

#[cfg(not(feature = "wasm"))]
#[cfg(test)]
pub mod tests {
    use futures::lock::Mutex;
    use tokio::time::sleep;
    use tokio::time::Duration;

    use super::*;
    use crate::dht::Did;
    use crate::ecc::SecretKey;
    use crate::message::MessageHandler;
    use crate::tests::default::prepare_node;
    use crate::tests::manually_establish_connection;

    #[derive(Clone)]
    struct MessageCallbackInstance {
        #[allow(clippy::type_complexity)]
        handler_messages: Arc<Mutex<Vec<(Did, Vec<u8>)>>>,
    }

    #[tokio::test]
    async fn test_custom_message_handling() -> Result<()> {
        let key1 = SecretKey::random();
        let key2 = SecretKey::random();

        let (did1, _dht1, swarm1, _handler1, _path1) = prepare_node(key1).await;
        let (did2, _dht2, swarm2, _handler2, _path2) = prepare_node(key2).await;

        manually_establish_connection(&swarm1, &swarm2).await?;

        #[async_trait]
        impl MessageCallback for MessageCallbackInstance {
            async fn custom_message(
                &self,
                handler: &MessageHandler,
                ctx: &MessagePayload<Message>,
                msg: &MaybeEncrypted<CustomMessage>,
            ) {
                let decrypted_msg = handler.decrypt_msg(msg).unwrap();
                self.handler_messages
                    .lock()
                    .await
                    .push((ctx.addr, decrypted_msg.0));
                println!("{:?}, {:?}, {:?}", ctx, ctx.addr, msg);
            }

            async fn builtin_message(
                &self,
                _handler: &MessageHandler,
                ctx: &MessagePayload<Message>,
            ) {
                println!("{:?}, {:?}", ctx, ctx.addr);
            }
        }

        let msg_callback1 = MessageCallbackInstance {
            handler_messages: Arc::new(Mutex::new(vec![])),
        };
        let msg_callback2 = MessageCallbackInstance {
            handler_messages: Arc::new(Mutex::new(vec![])),
        };
        let cb1: CallbackFn = Box::new(msg_callback1.clone());
        let cb2: CallbackFn = Box::new(msg_callback2.clone());

        let handler1 = Arc::new(swarm1.create_message_handler(Some(cb1), None));
        let handler2 = Arc::new(swarm2.create_message_handler(Some(cb2), None));

        let h1 = handler1.clone();
        let h2 = handler2.clone();
        tokio::spawn(async { h1.listen().await });
        tokio::spawn(async { h2.listen().await });

        println!("waiting for data channel ready");
        sleep(Duration::from_secs(5)).await;

        println!("sending messages");
        handler1
            .send_message(
                Message::custom("Hello world 1 to 2 - 1".as_bytes(), None)?,
                did2,
            )
            .await
            .unwrap();

        handler1
            .send_message(
                Message::custom("Hello world 1 to 2 - 2".as_bytes(), None)?,
                did2,
            )
            .await?;

        handler2
            .send_message(
                Message::custom("Hello world 2 to 1 - 1".as_bytes(), None)?,
                did1,
            )
            .await?;

        handler1
            .send_message(
                Message::custom("Hello world 1 to 2 - 3".as_bytes(), None)?,
                did2,
            )
            .await?;

        handler2
            .send_message(
                Message::custom("Hello world 2 to 1 - 2".as_bytes(), None)?,
                did1,
            )
            .await?;

        sleep(Duration::from_secs(5)).await;

        assert_eq!(msg_callback1.handler_messages.lock().await.as_slice(), &[
            (did2, "Hello world 2 to 1 - 1".as_bytes().to_vec()),
            (did2, "Hello world 2 to 1 - 2".as_bytes().to_vec())
        ]);

        assert_eq!(msg_callback2.handler_messages.lock().await.as_slice(), &[
            (did1, "Hello world 1 to 2 - 1".as_bytes().to_vec()),
            (did1, "Hello world 1 to 2 - 2".as_bytes().to_vec()),
            (did1, "Hello world 1 to 2 - 3".as_bytes().to_vec())
        ]);

        Ok(())
    }

    pub async fn assert_no_more_msg(
        node1: &MessageHandler,
        node2: &MessageHandler,
        node3: &MessageHandler,
    ) {
        tokio::select! {
            _ = node1.listen_once() => unreachable!("node1 should not receive any message"),
            _ = node2.listen_once() => unreachable!("node2 should not receive any message"),
            _ = node3.listen_once() => unreachable!("node3 should not receive any message"),
            _ = sleep(Duration::from_secs(3)) => {}
        }
    }

    pub async fn wait_for_msgs(
        node1: &MessageHandler,
        node2: &MessageHandler,
        node3: &MessageHandler,
    ) {
        loop {
            tokio::select! {
                _ = node1.listen_once() => {}
                _ = node2.listen_once() => {}
                _ = node3.listen_once() => {}
                _ = sleep(Duration::from_secs(3)) => break
            }
        }
    }
}
