use std::sync::Arc;

use futures::future::join;
use grammers_client::types::Chat;
use grammers_client::Client;
use tokio::sync::RwLock;

use crate::consts;
use crate::db::Db;
use crate::openai::api::OpenAIClient;

pub struct Processor {
    client: Client,
    db: Arc<RwLock<Db>>,
    openai: OpenAIClient,
}

#[derive(Clone)]
pub enum GPTLenght {
    Short,
    Medium,
    Long,
}

#[derive(Clone)]
pub enum Command {
    Summarize {
        chat: Chat,
        recipient: Chat,
        message_count: u32,
        gpt_length: GPTLenght,
        mentione_by_user: Option<String>,
    },
    SendPrompt {
        recipient: Chat,
        prompt: String,
        gpt_length: GPTLenght,
    },
}

struct CommandResult {
    new_commands: Vec<Command>,
    should_retry: bool,
}

impl Processor {
    // Creates processor and writing stream
    pub fn new(client: Client, db: Arc<RwLock<Db>>, openai: OpenAIClient) -> Self {
        Self { client, db, openai }
    }

    pub async fn run(
        mut self,
    ) -> (
        impl std::future::Future<Output = ((), ())>,
        tokio::sync::mpsc::Sender<Command>,
    ) {
        let queue = Arc::new(RwLock::new(Vec::<Command>::new()));
        let (tx, mut rx) = tokio::sync::mpsc::channel(1000);

        let msg_handler = {
            let queue = queue.clone();

            async move {
                loop {
                    let command = rx.recv().await;
                    match command {
                        Some(command) => {
                            let mut queue = queue.write().await;
                            log::info!("Received command: adding to queue");
                            queue.push(command);
                        }
                        None => break,
                    }
                }
            }
        };

        let processor = {
            async move {
                // Read from the front of the queue process and remove
                loop {
                    // Check if there is a command in the queue
                    let command = {
                        let queue = queue.read().await;
                        queue.first().cloned()
                    };
                    if let Some(command) = command {
                        log::info!("Processing command");
                        match self.process_command(command).await {
                            Ok(result) => {
                                if result.should_retry {
                                    log::info!(
                                        "The command should be retried. Sleeping for 60 secs"
                                    );
                                    // We probably hit the rate limit, so we should wait a bit
                                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                                } else {
                                    // We should send the new commands to the queue
                                    let mut queue = queue.write().await;
                                    queue.extend(result.new_commands);
                                    queue.remove(0);
                                }
                            }
                            Err(e) => {
                                log::error!("Error processing command: {e}");
                                let mut queue = queue.write().await;
                                queue.remove(0);
                            }
                        }
                    } else {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        };
        return (join(msg_handler, processor), tx);
    }

    async fn process_command(&mut self, command: Command) -> anyhow::Result<CommandResult> {
        match command {
            Command::Summarize {
                chat,
                recipient,
                message_count,
                gpt_length,
                mentione_by_user,
            } => {
                self.prepare_summary_prompts(
                    chat,
                    recipient,
                    message_count,
                    gpt_length,
                    mentione_by_user,
                )
                .await
            }
            Command::SendPrompt {
                recipient,
                prompt,
                gpt_length,
            } => {
                log::info!("Sending prompt");
                let result = self.openai.send_prompt(prompt, gpt_length);
                match result {
                    Ok(result) => {
                        let message = result.choices[0].message.content.as_ref().unwrap();
                        self.client
                            .send_message(&recipient, message.to_string())
                            .await
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(CommandResult {
                            new_commands: vec![],
                            should_retry: false,
                        })
                    }
                    Err(e) => {
                        log::error!("Error sending prompt: {:?}", e);
                        Ok(CommandResult {
                            new_commands: vec![],
                            should_retry: true,
                        })
                    }
                }
            }
        }
    }

    async fn prepare_summary_prompts(
        &self,
        chat: Chat,
        recipient: Chat,
        message_count: u32,
        gpt_length: GPTLenght,
        mentioned_by_user: Option<String>,
    ) -> anyhow::Result<CommandResult> {
        log::info!("Proccessing summarize command");
        let chat = &chat;
        let messages_id_to_load: Vec<i32> = self
            .db
            .read()
            .await
            .get_messages_id(chat.id(), message_count)?;
        let mut messages = Vec::with_capacity(message_count as usize);
        for i in 0..(messages_id_to_load.len() / consts::TELEGRAM_MAX_MESSAGE_FETCH + 1) {
            let minimum = i * consts::TELEGRAM_MAX_MESSAGE_FETCH;
            let maximum =
                ((i + 1) * consts::TELEGRAM_MAX_MESSAGE_FETCH).min(messages_id_to_load.len());
            let fetch_slice = &messages_id_to_load[minimum..maximum];
            let fetched_messages = self
                .client
                .get_messages_by_id(chat, fetch_slice)
                .await?
                .into_iter()
                .flatten()
                .filter(|message| {
                    if let Some(mentioned_by_user) = mentioned_by_user.as_ref() {
                        if let Some(sender) = message.sender() {
                            if let Chat::User(user) = sender {
                                if user.username() == Some(mentioned_by_user) {
                                    return true;
                                }
                            }
                        }
                        return false;
                    }
                    true
                })
                .collect::<Vec<_>>();
            messages.extend(fetched_messages);
        }
        if messages.is_empty() {
            self.client
                .send_message(recipient, "No messages found")
                .await?;
            return Ok(CommandResult {
                new_commands: vec![],
                should_retry: false,
            });
        }

        log::info!(
            "Creating prompts for summarization within {} messages",
            messages.len()
        );
        let prompts = self
            .openai
            .prepare_summarize_prompts(&messages)
            .into_iter()
            .map(|prompt| -> Command {
                Command::SendPrompt {
                    recipient: recipient.clone(),
                    prompt,
                    gpt_length: gpt_length.clone(),
                }
            })
            .collect();
        Ok(CommandResult {
            new_commands: prompts,
            should_retry: false,
        })
    }
}
