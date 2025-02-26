use super::*;
use jwst::{sync_encode_update, MapSubscription, Workspace};
use lru_time_cache::LruCache;
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::{broadcast::Sender, RwLock};
use y_sync::{
    awareness::{Event, Subscription},
    sync::Message as YMessage,
};
use yrs::{
    updates::encoder::{Encode, Encoder, EncoderV1},
    UpdateSubscription,
};

#[derive(Clone)]
pub enum BroadcastType {
    BroadcastAwareness(Vec<u8>),
    BroadcastContent(Vec<u8>),
    CloseUser(String),
    CloseAll,
}

type Broadcast = Sender<BroadcastType>;
pub type BroadcastChannels = RwLock<HashMap<String, Broadcast>>;

pub struct Subscriptions {
    _doc: Option<UpdateSubscription>,
    _awareness: Subscription<Event>,
    _metadata: MapSubscription,
}

pub async fn subscribe(workspace: &mut Workspace, sender: Broadcast) {
    let awareness = {
        let sender = sender.clone();
        let workspace_id = workspace.id();

        let dedup_cache = Arc::new(Mutex::new(LruCache::with_expiry_duration_and_capacity(
            Duration::from_micros(100),
            128,
        )));

        workspace
            .on_awareness_update(move |awareness, e| {
                trace!(
                    "workspace awareness changed: {}, {:?}",
                    workspace_id,
                    [e.added(), e.updated(), e.removed()].concat()
                );
                if let Ok(update) = awareness
                    .update_with_clients([e.added(), e.updated(), e.removed()].concat())
                    .map(|update| {
                        let mut encoder = EncoderV1::new();
                        YMessage::Awareness(update).encode(&mut encoder);
                        encoder.to_vec()
                    })
                {
                    let mut dedup_cache = dedup_cache.lock().unwrap_or_else(|e| e.into_inner());
                    if !dedup_cache.contains_key(&update) {
                        if sender
                            .send(BroadcastType::BroadcastAwareness(update.clone()))
                            .is_err()
                        {
                            info!("broadcast channel {workspace_id} has been closed",)
                        }
                        dedup_cache.insert(update, ());
                    }
                }
            })
            .await
    };
    let doc = {
        let workspace_id = workspace.id();
        workspace.observe(move |_, e| {
            trace!(
                "workspace {} changed: {}bytes",
                workspace_id,
                &e.update.len()
            );
            let update = sync_encode_update(&e.update);
            if sender
                .send(BroadcastType::BroadcastContent(update))
                .is_err()
            {
                info!("broadcast channel {workspace_id} has been closed",)
            }
        })
    };
    let metadata = workspace.observe_metadata(move |_, _e| {
        // context
        //     .user_channel
        //     .update_workspace(ws_id.clone(), context.clone());
    });

    let sub = Subscriptions {
        _awareness: awareness,
        _doc: doc,
        _metadata: metadata,
    };

    // TODO: this is a hack to prevent the subscription from being dropped
    // just keep the ownership
    std::mem::forget(sub);
}
