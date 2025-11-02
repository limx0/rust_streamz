//! Deribit trade-side classifier example.
//!
//! Connects to Deribit's WebSocket API using two separate streams: one for the
//! incremental order book and one for trades. Whenever a trade arrives we look at
//! the most recent best bid/ask from the order book stream and classify the
//! trade as BUY (>= best ask), SELL (<= best bid), or UNKNOWN otherwise.
//!
//! Run with:
//! ```bash
//! cargo run --example deribit_trade_classifier --features example -- BTC-PERPETUAL
//! ```

use anyhow::Result;
use rust_streamz::sources::websocket_client::{WebSocketClient, WebSocketClientConfigBuilder};
use rust_streamz::EngineBuilder;
use serde_json::{json, Value};
use std::env;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default)]
struct OrderBookSnapshot {
    best_bid: Option<f64>,
    best_ask: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TradeSide {
    Buy,
    Sell,
    Unknown,
}

// impl fmt::Display for TradeSide {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         let text = match self {
//             TradeSide::Buy => "BUY",
//             TradeSide::Sell => "SELL",
//             TradeSide::Unknown => "UNKNOWN",
//         };
//         write!(f, "{}", text)
//     }
// }

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let instrument = env::args()
        .nth(1)
        .unwrap_or_else(|| "BTC-PERPETUAL".to_string());
    println!("Monitoring instrument: {}", instrument);

    let orderbook_config = WebSocketClientConfigBuilder::new("wss://www.deribit.com/ws/api/v2")
        .with_message(&serde_json::to_string(&subscribe_orderbook(&instrument))?)
        .with_buffer_size(1024)
        .build();

    let trades_config = WebSocketClientConfigBuilder::new("wss://www.deribit.com/ws/api/v2")
        .with_message(&serde_json::to_string(&subscribe_trades(&instrument))?)
        .with_buffer_size(1024)
        .build();

    let orderbook_client = WebSocketClient::new(orderbook_config).await?;
    let trades_client = WebSocketClient::new(trades_config).await?;

    let orderbook_stream = orderbook_client.source().to_stream().accumulate(
        OrderBookSnapshot::default(),
        |snapshot, message| {
            println!("{:?}", message);
            apply_orderbook(snapshot, message)
        },
    );

    orderbook_stream.tap(|book| {
        println!("{:?}", book);
    });

    let trades_stream = trades_client.source().to_stream();

    let classification_stream = trades_stream.zip(&orderbook_stream);

    let instrument_for_trades = instrument.clone();
    classification_stream.clone().sink(move |pair| {
        let (trade, snapshot) = pair;
        process_trade(*snapshot, instrument_for_trades.as_str(), trade.as_str());
    });

    let trade_batch_buffer = classification_stream
        .clone()
        .timed_buffer(Duration::from_secs(5));
    trade_batch_buffer.tap(|batch| {
        if !batch.is_empty() {
            println!("Emitting batch of {} trades", batch.len());
        }
    });

    println!("Subscribed to order book and trades channels. Press Ctrl+C to exit.\n");

    EngineBuilder::new()
        .add_stream(orderbook_stream)
        .add_stream(trades_stream)
        .add_stream(classification_stream)
        .add_source_owned("Order book", orderbook_client)
        .add_source_owned("Trades", trades_client)
        .build()
        .run()
        .await?;

    Ok(())
}

fn subscribe_orderbook(instrument: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "public/subscribe",
        "params": {
            "channels": [format!("book.{}.100ms", instrument)]
        }
    })
}

fn subscribe_trades(instrument: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "public/subscribe",
        "params": {
            "channels": [format!("trades.{}.100ms", instrument)]
        }
    })
}

fn process_trade(snapshot: OrderBookSnapshot, instrument: &str, trade_message: &str) {
    let Ok(value) = serde_json::from_str::<Value>(trade_message) else {
        return;
    };

    let Some(data) = value
        .get("params")
        .and_then(|params| params.get("data"))
        .and_then(|entries| entries.as_array())
    else {
        return;
    };

    for trade in data {
        let Some(price) = trade.get("price").and_then(|p| p.as_f64()) else {
            continue;
        };
        let amount = trade.get("amount").and_then(|a| a.as_f64()).unwrap_or(0.0);
        let direction = trade
            .get("direction")
            .and_then(|d| d.as_str())
            .unwrap_or("unknown");

        let side = classify_trade(&snapshot, price);

        println!(
            "[{instrument}] Trade price: {price:.2}, amount: {amount:.4}, direction: {direction}, classified side: {side:?}",
        );
    }
}

fn apply_orderbook(mut snapshot: OrderBookSnapshot, message: &str) -> OrderBookSnapshot {
    let data = match serde_json::from_str::<Value>(message) {
        Ok(value) => {
            if let Some(data) = value.get("params").and_then(|params| params.get("data")) {
                data.clone()
            } else {
                return snapshot;
            }
        }
        Err(err) => {
            println!("ERR={:?}", err);
            return snapshot;
        }
    };
    let old_bid = snapshot.best_bid;
    let old_ask = snapshot.best_ask;

    if let Some(best_bid) = extract_best_bid(&data) {
        snapshot.best_bid = Some(best_bid);
    }

    if let Some(best_ask) = extract_best_ask(&data) {
        snapshot.best_ask = Some(best_ask);
    }

    if snapshot.best_bid != old_bid {
        if let Some(bid) = snapshot.best_bid {
            println!("Best bid updated: {:.2}", bid);
        }
    }

    if snapshot.best_ask != old_ask {
        if let Some(ask) = snapshot.best_ask {
            println!("Best ask updated: {:.2}", ask);
        }
    }

    snapshot
}

fn classify_trade(snapshot: &OrderBookSnapshot, price: f64) -> TradeSide {
    if let Some(ask) = snapshot.best_ask {
        if price >= ask {
            return TradeSide::Buy;
        }
    }

    if let Some(bid) = snapshot.best_bid {
        if price <= bid {
            return TradeSide::Sell;
        }
    }

    TradeSide::Unknown
}

fn extract_best_bid(data: &Value) -> Option<f64> {
    data.get("best_bid_price")
        .and_then(|value| value.as_f64())
        .or_else(|| {
            data.get("bids")
                .and_then(|bids| bids.as_array())
                .and_then(|entries| entries.first())
                .and_then(extract_price_from_level)
        })
}

fn extract_best_ask(data: &Value) -> Option<f64> {
    data.get("best_ask_price")
        .and_then(|value| value.as_f64())
        .or_else(|| {
            data.get("asks")
                .and_then(|asks| asks.as_array())
                .and_then(|entries| entries.first())
                .and_then(extract_price_from_level)
        })
}

fn extract_price_from_level(level: &Value) -> Option<f64> {
    match level {
        Value::Array(values) => values.get(0).and_then(|price| price.as_f64()),
        Value::Object(map) => map.get("price").and_then(|price| price.as_f64()),
        _ => None,
    }
}
