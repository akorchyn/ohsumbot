use std::sync::Arc;

use grammers_client::{
    types::{Chat, Message, User},
    Client, Update,
};
use tokio::sync::Mutex;

fn usage() -> String {
    format!("Usage: ./summarize <number of messages to summarize>

We don't store your messages. We store only latest {} message ids that will be used to fetch messages and discard them after summarization.", 
consts::MESSAGE_TO_STORE)
}

use crate::{
    consts,
    db::Db,
    openai::processor::{Command, GPTLenght},
};

pub struct Processor {
    client: Client,
    db: Arc<Mutex<Db>>,
    sender_channel: tokio::sync::mpsc::Sender<Command>,
    me: User,
}

impl Processor {
    pub async fn new(
        client: Client,
        db: Arc<Mutex<Db>>,
        sender: tokio::sync::mpsc::Sender<Command>,
    ) -> anyhow::Result<Self> {
        let me = client.get_me().await?;
        Ok(Self {
            client,
            db,
            sender_channel: sender,
            me,
        })
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
        let (cmd, bot_name) = if let Some(text) = splitted_string.next() {
            let mut split = text.split('@');
            let cmd = split.next().unwrap_or("");
            let bot_name = split.next();
            (cmd, bot_name)
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

        if bot_name.is_some() && bot_name != Some("") && bot_name != self.me.username() {
            return Ok(());
        }

        if cmd == "/help" {
            self.client.send_message(&message.chat(), usage()).await?;
        } else if cmd == "/summarize" || cmd == "/small" || cmd == "/medium" || cmd == "/large" {
            let length = match cmd {
                "/summarize" => GPTLenght::Medium,
                "/small" => GPTLenght::Short,
                "/medium" => GPTLenght::Medium,
                "/large" => GPTLenght::Long,
                _ => unreachable!(),
            };
            self.summarize(message, length).await?;
        } else if cmd.starts_with('/') || is_bot {
        } else {
            self.db
                .lock()
                .await
                .add_message_id(message.chat().id(), message.id())?;
        }

        Ok(())
    }

    async fn summarize(&mut self, message: Message, gpt_length: GPTLenght) -> anyhow::Result<()> {
        let mut splitted_string = message.text().split_whitespace();

        let count = splitted_string
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(consts::DEFAULT_SUMMARY_LENGTH)
            .min(consts::MESSAGE_TO_STORE);

        let filter_by_user = splitted_string
            .nth(2)
            .and_then(|s| s.parse::<String>().ok())
            .map(|s| s.trim_start_matches('@').to_string());

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
                gpt_length,
                mentione_by_user: filter_by_user,
            })
            .await?;

        Ok(())
    }
}
