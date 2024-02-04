use std::sync::Arc;

use grammers_client::{Client, Config};
use grammers_session::Session;
use tokio::sync::Mutex;

pub mod consts;
mod db;
mod openai;
mod telegram;

const DB_NAME: &str = "./db/db.sqlite3";
const SESSION_NAME: &str = "./db/session";

#[derive(serde::Deserialize, Debug)]
struct BotInfo {
    // Values required by Telegram.
    tg_api_id: i32,
    tg_api_hash: String,
    bot_token: String,

    // Values required by OpenAI.
    openai_api_key: String,
}

static FIXED_RECONNECT_POLICY: grammers_mtsender::FixedReconnect =
    grammers_mtsender::FixedReconnect {
        attempts: 5,
        delay: std::time::Duration::from_secs(5),
    };

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let db = Arc::new(Mutex::new(db::Db::new_with_file(DB_NAME)?));
    let env: BotInfo = envy::from_env()?;

    let client = Client::connect(Config {
        session: Session::load_file_or_create(SESSION_NAME)?,
        api_id: env.tg_api_id,
        api_hash: env.tg_api_hash,
        params: grammers_client::InitParams {
            catch_up: true,
            reconnection_policy: &FIXED_RECONNECT_POLICY,
            ..Default::default()
        },
    })
    .await?;

    if !client.is_authorized().await? {
        client.bot_sign_in(&env.bot_token).await?;
    }

    let openai_api: openai::api::OpenAIClient = openai::api::OpenAIClient::new(env.openai_api_key);
    let processor = openai::processor::Processor::new(client.clone(), db.clone(), openai_api);
    let (processor_handle, processor_queue) = processor.run().await;

    let mut bot = telegram::Processor::new(client.clone(), db.clone(), processor_queue).await?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("Ctrl-C received, shutting down...");
        }
        r = bot.process_updates() => {
            println!("Error processing updates: {:?}", r);
        }
        _ = processor_handle => {
            println!("Error processing commands");
        }
    }

    Ok(())
}
