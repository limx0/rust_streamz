#[cfg(feature = "requests")]
use crate::sources::http_client::{JsonPollingHttpClient, PollingHttpClient};
#[cfg(feature = "websockets")]
use crate::sources::websocket_client::WebSocketClient;
use crate::{Stream, TimedBuffer, TimedEmitter};
use anyhow::{anyhow, Result};
use futures_util::future::pending;
use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
#[cfg(feature = "requests")]
use serde::de::DeserializeOwned;
use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

pub trait EngineSource: 'static {
    fn run<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>;
}

pub struct EngineBuilder {
    streams: Vec<Box<dyn Any>>, // hold onto streams to keep pipelines alive
    sources: Vec<(String, Arc<dyn EngineSource>)>,
    timed_emitters: Vec<Rc<dyn TimedEmitter>>,
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self {
            streams: Vec::new(),
            sources: Vec::new(),
            timed_emitters: Vec::new(),
        }
    }

    pub fn add_stream<T>(mut self, stream: Stream<T>) -> Self
    where
        T: 'static,
    {
        self.streams.push(Box::new(stream));
        self
    }

    pub fn add_source<S>(mut self, label: impl Into<String>, source: Arc<S>) -> Self
    where
        S: EngineSource,
    {
        self.sources
            .push((label.into(), source as Arc<dyn EngineSource>));
        self
    }

    pub fn add_source_owned<S>(self, label: impl Into<String>, source: S) -> Self
    where
        S: EngineSource,
    {
        self.add_source(label, Arc::new(source))
    }

    pub fn add_timed_buffer<T>(mut self, buffer: TimedBuffer<T>) -> Self
    where
        T: Clone + 'static,
    {
        self.streams.push(Box::new(buffer.stream()));
        self.timed_emitters.push(buffer.as_timed_emitter());
        self
    }

    pub fn build(self) -> Engine {
        Engine {
            streams: self.streams,
            sources: self.sources,
            timed_emitters: self.timed_emitters,
        }
    }
}

#[cfg(feature = "websockets")]
impl EngineSource for WebSocketClient {
    fn run<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        Box::pin(async move { self.start().await })
    }
}

#[cfg(feature = "requests")]
impl EngineSource for PollingHttpClient {
    fn run<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        Box::pin(async move { self.start().await })
    }
}

#[cfg(feature = "requests")]
impl<T> EngineSource for JsonPollingHttpClient<T>
where
    T: DeserializeOwned + Clone + 'static,
{
    fn run<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        Box::pin(async move { self.start().await })
    }
}

pub struct Engine {
    #[allow(dead_code)]
    streams: Vec<Box<dyn Any>>,
    sources: Vec<(String, Arc<dyn EngineSource>)>,
    timed_emitters: Vec<Rc<dyn TimedEmitter>>,
}

impl Engine {
    pub async fn run(self) -> Result<()> {
        if self.sources.is_empty() {
            println!("No sources registered; waiting for Ctrl+C to exit.");
            tokio::signal::ctrl_c().await?;
            return Ok(());
        }

        let tasks = FuturesUnordered::new();

        let mut timers: Vec<TimerEntry> = self
            .timed_emitters
            .iter()
            .map(|emitter| TimerEntry {
                period: emitter.period(),
                next_tick: Instant::now() + emitter.period(),
                emitter: emitter.clone(),
            })
            .collect();

        for (label, source) in &self.sources {
            let label_clone = label.clone();
            let source_clone = Arc::clone(source);
            tasks.push(async move { source_clone.run().await.map_err(|err| (label_clone, err)) });
        }

        tokio::pin!(tasks);

        loop {
            let next_timer = timers.iter().map(|timer| timer.next_tick).min();

            tokio::select! {
                res = tasks.next() => {
                    match res {
                        Some(Ok(_)) => continue,
                        Some(Err((label, err))) => return Err(anyhow!("{} source error: {}", label, err)),
                        None => {
                            println!("All sources completed.");
                            return Ok(());
                        }
                    }
                }
                triggered = async {
                    if let Some(instant) = next_timer {
                        tokio::time::sleep_until(instant).await;
                        true
                    } else {
                        pending::<()>().await;
                        false
                    }
                } => {
                    if triggered {
                        let now = Instant::now();
                        for timer in timers.iter_mut() {
                            if now >= timer.next_tick {
                                timer.emitter.flush();
                                while timer.next_tick <= now {
                                    timer.next_tick += timer.period;
                                }
                            }
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nReceived interrupt. Shutting down engine...");
                    return Ok(());
                }
            }
        }
    }
}

struct TimerEntry {
    period: Duration,
    next_tick: Instant,
    emitter: Rc<dyn TimedEmitter>,
}
