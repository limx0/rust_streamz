use crate::Source;
use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};

#[derive(Clone, Debug)]
pub struct PollingHttpClientConfig {
    pub url: String,
    pub period: Duration,
    pub headers: HeaderMap,
    pub method: HttpMethod,
    pub body: Option<String>,
}

impl PollingHttpClientConfig {
    pub fn new(url: &str, period: Duration) -> Self {
        Self {
            url: url.to_string(),
            period,
            headers: HeaderMap::new(),
            method: HttpMethod::Get,
            body: None,
        }
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Result<Self> {
        let name = HeaderName::from_bytes(key.as_bytes())?;
        let value = HeaderValue::from_str(value)?;
        self.headers.insert(name, value);
        Ok(self)
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Result<Self> {
        for (key, value) in headers {
            let name = HeaderName::from_bytes(key.as_bytes())?;
            let value = HeaderValue::from_str(&value)?;
            self.headers.insert(name, value);
        }
        Ok(self)
    }

    pub fn with_method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

#[derive(Clone, Debug)]
pub enum HttpMethod {
    Get,
    Post,
}

pub struct PollingHttpClient {
    client: reqwest::Client,
    config: PollingHttpClientConfig,
    source: Source<String>,
}

impl PollingHttpClient {
    pub async fn new(config: PollingHttpClientConfig) -> Result<Self> {
        let client = reqwest::Client::builder().no_proxy().build()?;

        Ok(Self {
            client,
            config,
            source: Source::new(),
        })
    }

    pub fn source(&self) -> &Source<String> {
        &self.source
    }

    pub async fn start(&self) -> Result<()> {
        let mut ticker = interval(self.config.period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        // Perform an immediate poll before entering the interval loop.
        self.poll_once().await?;

        loop {
            ticker.tick().await;
            self.poll_once().await?;
        }
    }

    async fn poll_once(&self) -> Result<()> {
        let mut request = match self.config.method {
            HttpMethod::Get => self.client.get(&self.config.url),
            HttpMethod::Post => self.client.post(&self.config.url),
        };

        if !self.config.headers.is_empty() {
            request = request.headers(self.config.headers.clone());
        }
        if let Some(body) = &self.config.body {
            request = request.body(body.clone());
        }

        let response = request.send().await?;
        let text = response.text().await?;
        self.source.emit(text);
        Ok(())
    }
}

pub struct JsonPollingHttpClient<T> {
    inner: PollingHttpClient,
    source: Source<T>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> JsonPollingHttpClient<T>
where
    T: DeserializeOwned + Clone + 'static,
{
    pub async fn new(config: PollingHttpClientConfig) -> Result<Self> {
        Ok(Self {
            inner: PollingHttpClient::new(config).await?,
            source: Source::new(),
            _marker: std::marker::PhantomData,
        })
    }

    pub fn source(&self) -> &Source<T> {
        &self.source
    }

    pub async fn start(&self) -> Result<()> {
        let mut ticker = interval(self.inner.config.period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        self.poll_once().await?;
        loop {
            ticker.tick().await;
            self.poll_once().await?;
        }
    }

    async fn poll_once(&self) -> Result<()> {
        let mut request = match self.inner.config.method {
            HttpMethod::Get => self.inner.client.get(&self.inner.config.url),
            HttpMethod::Post => self.inner.client.post(&self.inner.config.url),
        };

        if !self.inner.config.headers.is_empty() {
            request = request.headers(self.inner.config.headers.clone());
        }
        if let Some(body) = &self.inner.config.body {
            request = request.body(body.clone());
        }
        let response = request.send().await?;
        let value = response.json::<T>().await?;
        self.source.emit(value);
        Ok(())
    }
}
