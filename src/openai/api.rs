use grammers_client::types::Message;
use openai_api_rs::v1::{
    api::Client,
    chat_completion::{self, ChatCompletionMessage, ChatCompletionRequest, ChatCompletionResponse},
    common::GPT3_5_TURBO,
};

use crate::consts;

use super::processor::GPTLenght;

const PROMPT: &str = r#"You are sumarization bot that helps telegram users to keep track of the conversation.
Summarize the messages from the chat below. The chat follow chronological order.
The summary will be sent to the user who requested it and should be easy to read and understand.
The summary will be sent as a message to the user who requested it.
The summary should be written using language the same as in input messages. If you are not sure, use ukrainian language.
The summary should be brief and should display the main idea of provided messages.

Create a summary from the message below using rules provided above:
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

    pub fn prepare_summarize_prompts(&self, messages: &[Message]) -> Vec<String> {
        if messages.is_empty() {
            return vec![];
        }

        let mut prompt = vec![];
        let mut msg = PROMPT.to_string();
        for (i, message) in messages.iter().rev().enumerate() {
            let new_line = format!(
                "{}. [@{}]: \"{}\"\n",
                i + 1,
                message
                    .sender()
                    .and_then(|s| s.username().map(ToString::to_string))
                    .unwrap_or("Unknown".to_string()),
                message.text()
            );
            if msg.len() + new_line.len() > consts::TOKEN_LIMITS_PER_MESSAGE {
                msg.push_str("```");
                prompt.push(msg);
                msg = PROMPT.to_string() + &new_line;
            } else {
                msg.push_str(&new_line);
            }
        }
        msg.push_str("```");
        prompt.push(msg);
        prompt
    }

    pub fn send_prompt(
        &self,
        prompt: String,
        gpt_length: GPTLenght,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let client: Client = Client::new(self.api_key.clone());

        let max_tokens = match gpt_length {
            GPTLenght::Short => 32,
            GPTLenght::Medium => 64,
            GPTLenght::Long => 128,
        };

        let req = ChatCompletionRequest::new(
            GPT3_5_TURBO.to_string(),
            vec![ChatCompletionMessage {
                role: chat_completion::MessageRole::assistant,
                content: prompt,
                name: Some("Sumbot".to_string()),
                function_call: None,
            }],
        )
        .max_tokens(max_tokens)
        .temperature(0.5)
        .top_p(0.5);

        let result = client.chat_completion(req)?;
        if result.choices.is_empty() || result.choices[0].message.content.is_none() {
            return Err(anyhow::anyhow!("Failed to summarize the chat"));
        }
        Ok(result)
    }
}
