use grammers_client::types::Message;
use openai_api_rs::v1::{
    api::Client,
    chat_completion::{self, ChatCompletionMessage, ChatCompletionRequest},
    common::GPT3_5_TURBO,
};

const PROMPT: &str = r#"Please summarize the content and discussions from the provided messages in this chat into a single concise summary (TL;DR). Provide an overview of the main topics, key points, and any notable discussions that took place within these messages. Please use the language that was used in the input for a summary.
The summary shouldn't be very long but should be long enough to cover the main points of the messages. The summary should be written in a way that is easy to understand and doesn't contain any unnecessary information.

Supported languages is Ukrainian, English.

PLEASE TRANSLATE SUMMARY TO THE LANGUAGE OF THE INPUT MESSAGES BUT IT SHOULD BE IN SUPPORTED LANGUAGES. DO NOT TRANSLATE THE INPUT MESSAGES TO ENGLISH AND DO NOT USE ENGLISH IN SUMMARY IF THE MESSAGES ARE NOT IN ENGLISH.
THE SUMMARY SHOULD REPRESENT INTENTION AND THE SENSE OF THE INPUT MESSAGES.
PLEASE DONT FORGET TO USE @ before the username of the sender.

The messages will be provided in the next format.
```
1. [Sender]: "message"
2. [Some other or same sender]: "message"
```

Messages to summarize:
```
"#;

#[derive(Clone)]
pub struct OpenAIClient {
    api_key: String,
}

impl OpenAIClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub fn summarize(&self, messages: &[Message]) -> anyhow::Result<String> {
        if messages.is_empty() {
            return Ok("No messages to summarize".to_string());
        }

        let client: Client = Client::new(self.api_key.clone());
        log::info!("Summarizing {} messages", messages.len());

        let mut prompt = String::from(PROMPT);
        for (i, message) in messages.iter().enumerate().rev() {
            prompt.push_str(&format!(
                "{}. [@{}]: \"{}\"\n",
                i + 1,
                message
                    .sender()
                    .and_then(|s| s.username().map(ToString::to_string))
                    .unwrap_or("Unknown".to_string()),
                message.text()
            ));
        }
        prompt.push_str("```");

        let req = ChatCompletionRequest::new(
            GPT3_5_TURBO.to_string(),
            vec![ChatCompletionMessage {
                role: chat_completion::MessageRole::assistant,
                content: prompt,
                name: Some("Sumbot".to_string()),
                function_call: None,
            }],
        );

        let result = client.chat_completion(req)?;
        if result.choices.is_empty() || result.choices[0].message.content.is_none() {
            return Ok("Failed to summarize the chat".to_string());
        }
        Ok(result.choices[0].message.content.as_ref().unwrap().clone())
    }
}
