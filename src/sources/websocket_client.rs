use crate::Source;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Clone, Debug)]
pub struct WebSocketClientConfig {
    pub url: String,
    pub init_messages: Vec<String>,
    pub buffer_size: usize,
}

pub struct WebSocketClientConfigBuilder {
    url: String,
    init_messages: Vec<String>,
    buffer_size: usize,
}

impl WebSocketClientConfigBuilder {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            init_messages: Vec::new(),
            buffer_size: 256,
        }
    }

    pub fn with_message(mut self, message: &str) -> Self {
        self.init_messages.push(message.to_string());
        self
    }

    pub fn with_messages(mut self, messages: Vec<String>) -> Self {
        self.init_messages = messages;
        self
    }

    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    pub fn build(self) -> WebSocketClientConfig {
        WebSocketClientConfig {
            url: self.url,
            init_messages: self.init_messages,
            buffer_size: self.buffer_size,
        }
    }
}

pub struct WebSocketClient {
    config: WebSocketClientConfig,
    source: Source<String>,
}

impl WebSocketClient {
    pub async fn new(config: WebSocketClientConfig) -> Result<Self> {
        Ok(Self {
            config,
            source: Source::new(),
        })
    }

    pub fn source(&self) -> &Source<String> {
        &self.source
    }

    pub async fn start(&self) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.config.url).await?;
        let (mut write, mut read) = ws_stream.split();

        let _ = self.config.buffer_size;

        for message in &self.config.init_messages {
            write.send(Message::Text(message.clone().into())).await?;
        }

        while let Some(message) = read.next().await {
            match message? {
                Message::Text(text) => {
                    let text = text.to_string();
                    self.source.emit(text);
                }
                Message::Binary(data) => {
                    if let Ok(text) = String::from_utf8(data.to_vec()) {
                        self.source.emit(text);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }

        Ok(())
    }
}
