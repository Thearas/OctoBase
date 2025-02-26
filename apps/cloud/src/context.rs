use cloud_components::{FirebaseContext, KeyContext, MailContext};
use cloud_database::CloudDatabase;
use jwst::SearchResults;
use jwst_logger::{error, warn};
use jwst_rpc::{BroadcastChannels, BroadcastType, RpcContextImpl};
use jwst_storage::JwstStorage;
use std::collections::HashMap;
use tokio::sync::{Mutex, RwLock};

use crate::api::UserChannel;

pub struct Context {
    pub key: KeyContext,
    pub firebase: Mutex<FirebaseContext>,
    pub mail: MailContext,
    pub db: CloudDatabase,
    pub storage: JwstStorage,
    pub user_channel: UserChannel,
    pub channel: BroadcastChannels,
}

impl Context {
    pub async fn new() -> Context {
        let database_url = dotenvy::var("DATABASE_URL");
        if database_url.is_err() {
            warn!("!!! no database url provided, use memory database !!!");
            warn!("!!! please set DATABASE_URL in .env file or environmental variable to save your data !!!");
        }

        Self {
            // =========== database ===========
            db: CloudDatabase::init_pool(
                database_url
                    .as_deref()
                    .unwrap_or("sqlite::memory:?cache=shared"),
            )
            .await
            .expect("Cannot create cloud database"),
            storage: JwstStorage::new(
                database_url
                    .map(|db| format!("{db}_binary"))
                    .as_deref()
                    .unwrap_or("sqlite::memory:?cache=shared"),
            )
            .await
            .expect("Cannot create storage"),
            // =========== auth ===========
            key: KeyContext::new(dotenvy::var("SIGN_KEY").ok()).expect("Cannot create key context"),
            firebase: Mutex::new(FirebaseContext::new(
                dotenvy::var("FIREBASE_PROJECT_ID")
                    .map(|id| vec![id])
                    .unwrap_or_else(|_| {
                        vec!["pathfinder-52392".into(), "quiet-sanctuary-370417".into()]
                    }),
            )),
            // =========== mail ===========
            mail: MailContext::new(
                dotenvy::var("MAIL_ACCOUNT").ok(),
                dotenvy::var("MAIL_PASSWORD").ok(),
            ),
            // =========== sync channel ===========
            channel: RwLock::new(HashMap::new()),
            user_channel: UserChannel::new(),
        }
    }

    pub async fn search_workspace(
        &self,
        workspace_id: String,
        query_string: &str,
    ) -> Result<SearchResults, Box<dyn std::error::Error>> {
        let workspace_id = workspace_id.to_string();

        match self.storage.get_workspace(workspace_id.clone()).await {
            Ok(workspace) => {
                let search_results = workspace.search(query_string)?;
                Ok(search_results)
            }
            Err(e) => {
                error!("cannot get workspace: {}", e);
                Err(Box::new(e))
            }
        }
    }

    // TODO: this should be moved to another module
    pub async fn close_websocket(&self, workspace: String, user: String) {
        let mut closed = vec![];
        let event = BroadcastType::CloseUser(user);
        for (channel, tx) in self.channel.read().await.iter() {
            if channel == &workspace {
                if tx.receiver_count() <= 1 {
                    closed.push(channel.clone());
                }
                let _ = tx.send(event.clone());
            }
        }
        for channel in closed {
            self.channel.write().await.remove(&channel);
        }
    }

    // TODO: this should be moved to another module
    pub async fn close_websocket_by_workspace(&self, workspace: String) {
        let mut closed = vec![];
        for (id, tx) in self.channel.read().await.iter() {
            if id == &workspace {
                closed.push(id.clone());
                let _ = tx.send(BroadcastType::CloseAll);
            }
        }
        for channel in closed {
            self.channel.write().await.remove(&channel);
        }
    }
}

impl RpcContextImpl<'_> for Context {
    fn get_storage(&self) -> &JwstStorage {
        &self.storage
    }

    fn get_channel(&self) -> &BroadcastChannels {
        &self.channel
    }
}
