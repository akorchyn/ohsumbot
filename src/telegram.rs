use grammers_client::{
    types::{Chat, Message},
    Client, Update,
};

const USAGE: &str = "Usage: ./summarize <number of messages to summarize>

We don't store your messages. We store only latest 200 message ids that will be used to fetch messages and discard them after summarization.
";

use crate::{db::Db, openai::OpenAIClient};

pub struct Processor {
    client: Client,
    openai: OpenAIClient,
    db: Db,
}

impl Processor {
    pub fn new(client: Client, db: Db, openai: OpenAIClient) -> Self {
        Self { client, db, openai }
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
                _ => log::info!("Update: {:?}", &update),
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
        log::info!("Command: {:?}", cmd);

        if cmd == "/help" {
            self.client.send_message(&message.chat(), USAGE).await?;
        } else if cmd == "/summarize" {
            self.summarize(message).await?;
        } else {
            let is_bot = message
                .sender()
                .map(|s| match s {
                    Chat::User(user) => user.is_bot(),
                    _ => false,
                })
                .unwrap_or(false);
            if !is_bot {
                self.db.add_message_id(message.chat().id(), message.id())?;
            }
        }

        Ok(())
    }

    async fn summarize(&mut self, message: Message) -> anyhow::Result<()> {
        let mut splitted_string = message.text().split_whitespace();

        let count = if let Some(amount) = splitted_string.nth(1).and_then(|s| s.parse::<u32>().ok())
        {
            amount.min(200)
        } else {
            self.client.send_message(message.chat(), "You should provide a number of messages to summarize. But we have a cap of 200").await?;
            return Ok(());
        };

        let sender = if let Some(sender) = message.sender() {
            if let Err(_) = self.client.send_message(&sender, "Summarizing...").await {
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

        let messages_id_to_load: Vec<i32> = self.db.get_messages_id(message.chat().id(), count)?;

        let messages = self
            .client
            .get_messages_by_id(message.chat(), &messages_id_to_load)
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        tokio::spawn(Self::summarization(
            self.client.clone(),
            self.openai.clone(),
            sender,
            messages,
        ));

        Ok(())
    }

    async fn summarization(
        client: Client,
        openai_client: OpenAIClient,
        chat: Chat,
        messages: Vec<Message>,
    ) {
        let summary = openai_client
            .summarize(&messages)
            .unwrap_or("Failed to summarize the chat".to_string());
        if let Err(e) = client.send_message(chat, summary).await {
            log::error!("Error sending message: {:?}", e);
        }
    }
}
