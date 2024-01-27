use std::sync::Arc;

use grammers_client::{
    types::{Chat, Message},
    Client, Update,
};
use tokio::sync::RwLock;

fn usage() -> String {
    format!("Usage: ./summarize <number of messages to summarize>

We don't store your messages. We store only latest {} message ids that will be used to fetch messages and discard them after summarization.", 
consts::MESSAGE_TO_STORE)
}

use crate::{consts, db::Db, openai::processor::Command};

pub struct Processor {
    client: Client,
    db: Arc<RwLock<Db>>,
    sender_channel: tokio::sync::mpsc::Sender<Command>,
}

impl Processor {
    pub fn new(
        client: Client,
        db: Arc<RwLock<Db>>,
        sender: tokio::sync::mpsc::Sender<Command>,
    ) -> Self {
        Self {
            client,
            db,
            sender_channel: sender,
        }
    }

    pub async fn process_updates(&mut self) -> anyhow::Result<()> {
        while let Some(update) = self.client.next_update().await? {
            match update {
                Update::NewMessage(message)
                    if !message.outgoing() && matches!(message.chat(), Chat::Group(_)) =>
                {
                    if let Err(err) = self.process_message(message).await {
                        log::error!("Error processing message: {:?}", err)
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn process_message(&mut self, message: Message) -> anyhow::Result<()> {
        let mut splitted_string = message.text().split_whitespace();
        let cmd = if let Some(text) = splitted_string.next() {
            text
        } else {
            return Ok(());
        };
        let is_bot = message
            .sender()
            .map(|s| match s {
                Chat::User(user) => user.is_bot(),
                _ => false,
            })
            .unwrap_or(false);

        if cmd == "/help" {
            self.client.send_message(&message.chat(), usage()).await?;
        } else if cmd == "/summarize" {
            self.summarize(message).await?;
        } else if cmd.starts_with("/") || is_bot {
        } else {
            self.db
                .write()
                .await
                .add_message_id(message.chat().id(), message.id())?;
        }

        Ok(())
    }

    async fn summarize(&mut self, message: Message) -> anyhow::Result<()> {
        let mut splitted_string = message.text().split_whitespace();

        let count = splitted_string
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(consts::DEFAULT_SUMMARY_LENGTH)
            .min(consts::MESSAGE_TO_STORE);

        let sender = if let Some(sender) = message.sender() {
            if self
                .client
                .send_message(&sender, format!("Summarizing {count} messages..."))
                .await
                .is_err()
            {
                self.client
                    .send_message(
                        message.chat(),
                        "Couldn't send you a message. Please, start a conversation with me first.",
                    )
                    .await?;
                return Ok(());
            } else {
                sender
            }
        } else {
            self.client
                .send_message(
                    message.chat(),
                    "Sender is unknown. Check your privacy settings.",
                )
                .await?;
            return Ok(());
        };
        self.sender_channel
            .send(Command::Summarize {
                chat: message.chat(),
                recipient: sender,
                message_count: count,
            })
            .await?;

        Ok(())
    }
}
