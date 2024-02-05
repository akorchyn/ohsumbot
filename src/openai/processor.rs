use std::sync::Arc;

use futures::future::join;
use grammers_client::types::{Chat, Media, Message};
use grammers_client::Client;
use mime::Mime;
use tokio::sync::{Mutex, RwLock};

use crate::consts;
use crate::db::Db;
use crate::openai::api::OpenAIClient;

pub use super::api::GPTLenght;
use super::api::Prompt;

pub struct Processor {
    client: Client,
    db: Arc<Mutex<Db>>,
    openai: OpenAIClient,
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
    SummarizeMessage {
        chat: Chat,
        recipient: Chat,
        message_id: i32,
        gpt_length: GPTLenght,
    },
    SendPrompt {
        recipient: Chat,
        prompt: Prompt,
    },
    Ask {
        chat: Chat,
        recipient: Chat,
        question: String,
        message_count: u32,
        gpt_length: GPTLenght,
    },
}

struct CommandResult {
    new_commands: Vec<Command>,
}

impl Processor {
    // Creates processor and writing stream
    pub fn new(client: Client, db: Arc<Mutex<Db>>, openai: OpenAIClient) -> Self {
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
                                let mut queue = queue.write().await;
                                queue.extend(result.new_commands);
                                queue.remove(0);
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
        (join(msg_handler, processor), tx)
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
                self.prepare_summary_prompt(
                    chat,
                    recipient,
                    message_count,
                    gpt_length,
                    mentione_by_user,
                )
                .await
            }
            Command::SummarizeMessage {
                chat,
                recipient,
                message_id,
                gpt_length,
            } => {
                self.summarize_message(chat, recipient, message_id, gpt_length)
                    .await
            }
            Command::Ask {
                chat,
                recipient,
                question,
                message_count,
                gpt_length,
            } => {
                self.ask_on_summary(chat, recipient, question, message_count, gpt_length)
                    .await
            }
            Command::SendPrompt { recipient, prompt } => {
                log::info!("Sending prompt");
                let result = self.openai.send_prompt(prompt);
                match result {
                    Ok(result) => {
                        let message = result.choices[0].message.as_ref().unwrap().content.as_ref();
                        self.client
                            .send_message(&recipient, message)
                            .await
                            .map_err(|e| anyhow::anyhow!(e))?;
                    }
                    Err(e) => {
                        log::error!("Error sending prompt: {:?}", e);
                        self.client
                            .send_message(
                                recipient,
                                "Failed to summarize the chat. Try again later",
                            )
                            .await?;
                    }
                }
                Ok(CommandResult {
                    new_commands: vec![],
                })
            }
        }
    }

    async fn ask_on_summary(
        &self,
        chat: Chat,
        recipient: Chat,
        question: String,
        message_count: u32,
        gpt_length: GPTLenght,
    ) -> anyhow::Result<CommandResult> {
        let messages = self.load_messages(&chat, message_count, None).await?;
        if messages.is_empty() {
            self.client
                .send_message(recipient, "No messages found")
                .await?;
            return Ok(CommandResult {
                new_commands: vec![],
            });
        }

        let prompt = self
            .openai
            .prepare_question_prompt(&messages, &question, gpt_length)
            .into_iter()
            .map(|prompt| -> Command {
                Command::SendPrompt {
                    recipient: recipient.clone(),
                    prompt,
                }
            })
            .collect();
        Ok(CommandResult {
            new_commands: prompt,
        })
    }

    async fn summarize_message(
        &self,
        chat: Chat,
        recipient: Chat,
        message_id: i32,
        gpt_length: GPTLenght,
    ) -> anyhow::Result<CommandResult> {
        let message = self
            .client
            .get_messages_by_id(&chat, &[message_id])
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut commands = vec![];

        if let [message, ..] = message.as_slice() {
            if let Some(media) = message.media() {
                commands.extend(
                    self.process_media(message, media, recipient.clone(), gpt_length)
                        .await?,
                );
            }

            if !message.text().is_empty() {
                let prompt = self
                    .openai
                    .prepare_text_summary(message.text(), gpt_length)
                    .into_iter()
                    .map(|prompt| -> Command {
                        Command::SendPrompt {
                            recipient: recipient.clone(),
                            prompt,
                        }
                    });
                commands.extend(prompt);
            }
        }

        if commands.is_empty() {
            self.client
                .send_message(recipient, "No messages found. Please be aware that messages from bots are not available.")
                .await?;
        }

        Ok(CommandResult {
            new_commands: commands,
        })
    }

    async fn process_media(
        &self,
        message: &Message,
        media: Media,
        recipient: Chat,
        gpt_length: GPTLenght,
    ) -> anyhow::Result<Vec<Command>> {
        match media {
            Media::Document(document)
                if document.mime_type().map(|s| {
                    let mime: Result<Mime, _> = s.parse();
                    if let Ok(mime) = mime {
                        mime.type_() == mime::AUDIO || mime.type_() == mime::VIDEO
                    } else {
                        false
                    }
                }) == Some(true) =>
            {
                // Checked above
                log::info!("Downloading media");
                let mime: Mime = document.mime_type().unwrap().parse().unwrap();
                let extension = mime.subtype().as_str();
                let is_video = mime.type_() == mime::VIDEO;
                let save_path = format!("{}/{}.{}", consts::MEDIA_DIR, message.id(), extension);
                let downloaded = message.download_media(&save_path).await?;
                if !downloaded {
                    self.client
                        .send_message(recipient, "Failed to download media")
                        .await?;
                    return Ok(vec![]);
                }

                let file = if is_video {
                    log::info!("Converting video to audio");
                    let destination = format!("{}/{}.mp3", consts::MEDIA_DIR, message.id());
                    if !tokio::process::Command::new("ffmpeg")
                        .args([
                            "-i",
                            &save_path,
                            "-vn",
                            "-acodec",
                            "libmp3lame",
                            "-b:a",
                            "128k",
                            &destination,
                        ])
                        .status()
                        .await?
                        .success()
                    {
                        self.client
                            .send_message(recipient, "Failed to convert video to audio")
                            .await?;
                        return Ok(vec![]);
                    }
                    destination
                } else {
                    save_path.clone()
                };
                log::info!("Converting audio to text");
                let text = self.openai.audio_to_text(&file)?;

                // Remove the file
                tokio::fs::remove_file(&file).await?;
                if is_video {
                    tokio::fs::remove_file(&save_path).await?;
                }

                log::info!("Summarizing transcribed text");
                if let Some(text) = text.text {
                    let result = self
                        .openai
                        .prepare_text_summary(&text, gpt_length)
                        .into_iter()
                        .map(|prompt| Command::SendPrompt {
                            recipient: recipient.clone(),
                            prompt,
                        })
                        .collect();
                    Ok(result)
                } else {
                    self.client
                        .send_message(recipient, "Failed to transcribe audio")
                        .await?;
                    Ok(vec![])
                }
            }
            _ => {
                self.client
                    .send_message(recipient, "Unsupported media type")
                    .await?;
                Ok(vec![])
            }
        }
    }

    async fn prepare_summary_prompt(
        &self,
        chat: Chat,
        recipient: Chat,
        message_count: u32,
        gpt_length: GPTLenght,
        mentioned_by_user: Option<String>,
    ) -> anyhow::Result<CommandResult> {
        log::info!("Proccessing summarize command");
        let chat = &chat;

        let messages = self
            .load_messages(chat, message_count, mentioned_by_user)
            .await?;

        if messages.is_empty() {
            self.client
                .send_message(recipient, "No messages found")
                .await?;
            return Ok(CommandResult {
                new_commands: vec![],
            });
        }

        log::info!(
            "Creating prompts for summarization within {} messages",
            messages.len()
        );
        let prompts = self
            .openai
            .prepare_summarize_prompts_from_messages(&messages, gpt_length)
            .into_iter()
            .map(|prompt| -> Command {
                Command::SendPrompt {
                    recipient: recipient.clone(),
                    prompt,
                }
            })
            .collect();
        Ok(CommandResult {
            new_commands: prompts,
        })
    }

    async fn load_messages(
        &self,
        chat: &Chat,
        message_count: u32,
        mentioned_by_user: Option<String>,
    ) -> anyhow::Result<Vec<Message>> {
        let messages_id_to_load: Vec<i32> = self
            .db
            .lock()
            .await
            .get_messages_id(chat.id(), message_count)?;
        let mut messages = Vec::with_capacity(messages_id_to_load.len() as usize);
        for i in 0..(messages_id_to_load.len() / consts::TELEGRAM_MAX_MESSAGE_FETCH + 1) {
            let minimum = i * consts::TELEGRAM_MAX_MESSAGE_FETCH;
            let maximum =
                ((i + 1) * consts::TELEGRAM_MAX_MESSAGE_FETCH).min(messages_id_to_load.len());
            if minimum == maximum {
                break;
            }

            let fetch_slice = &messages_id_to_load[minimum..maximum];
            let fetched_messages = self
                .client
                .get_messages_by_id(chat, fetch_slice)
                .await?
                .into_iter()
                .flatten()
                .filter(|message| {
                    if let Some(mentioned_by_user) = mentioned_by_user.as_ref() {
                        if let Some(Chat::User(user)) = message.sender() {
                            if user.username() == Some(mentioned_by_user) {
                                return true;
                            }
                        }
                        return false;
                    }
                    true
                })
                .collect::<Vec<_>>();
            messages.extend(fetched_messages);
        }
        Ok(messages)
    }
}
