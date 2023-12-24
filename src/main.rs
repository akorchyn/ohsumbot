use grammers_client::{Client, Config};
use grammers_session::Session;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();
    let db = db::Db::new_with_file(DB_NAME)?;
    let env: BotInfo = envy::from_env()?;

    let client = Client::connect(Config {
        session: Session::load_file_or_create(SESSION_NAME)?,
        api_id: env.tg_api_id,
        api_hash: env.tg_api_hash,
        params: Default::default(),
    })
    .await?;

    if !client.is_authorized().await? {
        client.bot_sign_in(&env.bot_token).await?;
    }

    let openai = openai::OpenAIClient::new(env.openai_api_key);

    let mut processor = telegram::Processor::new(client.clone(), db, openai);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("Ctrl-C received, shutting down...");
        }
        r = processor.process_updates() => {
            println!("Error processing updates: {:?}", r);
        }
    }

    Ok(())
}
