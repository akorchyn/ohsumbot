use grammers_client::types::Message;
use openai_api_rs::v1::{
    api::Client,
    chat_completion::{self, ChatCompletionMessage, ChatCompletionRequest},
    common::GPT3_5_TURBO,
};

const PROMPT: &str = r#"Summarize the content and discussions from the provided chat messages into a concise and clear overview (TL;DR). The summary should capture the essence and context of the main topics, key points, and any notable discussions, reflecting the sequence and depth of the conversation.

* **Language Priority**: Summarize in Ukrainian or English, aligning with the original message languages. Ukrainian has priority.
* **Brevity with Clarity**: Ensure the summary is brief yet comprehensive, clearly conveying the discussion's essence without unnecessary detail or complex language.
* **Objective Tone**: Maintain an objective and factual tone, reflecting the discussions accurately.
* **Format**: Follow any specified summary format, ensuring consistency and readability.
* **Handling Ambiguity**: Address any ambiguous points by closely aligning with the most probable intent or excluding them if they significantly disrupt clarity.

Messages follow this format:
```
1. [Sender]: "message"
2. [Some other or same sender]: "message"
```

Focus on delivering a summary that is easy to understand, devoid of unnecessary information, and true to the original discussion's intent and language.
Please take your time to process the input.

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
