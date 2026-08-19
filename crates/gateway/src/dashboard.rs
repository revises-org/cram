// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

use std::{collections::VecDeque, sync::Arc, sync::Mutex, time::SystemTime};

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::get,
    Json, Router,
};
use cram_vertex::{CompletionEvent, Observer};
use rust_embed::RustEmbed;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[derive(Serialize, Clone)]
struct DashboardUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    reasoning_tokens: u64,
    cached_tokens: Option<u64>,
}

#[derive(Serialize, Clone)]
struct DashboardEvent {
    timestamp: u64, // ms since epoch
    model: String,
    status: u16,
    streamed: bool,
    duration_ms: u128,
    ttfb_ms: Option<u128>,
    usage: Option<DashboardUsage>,
}

impl From<CompletionEvent> for DashboardEvent {
    fn from(ev: CompletionEvent) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        DashboardEvent {
            timestamp,
            model: ev.model,
            status: ev.status,
            streamed: ev.streamed,
            duration_ms: ev.duration.as_millis(),
            ttfb_ms: ev.ttfb.map(|d| d.as_millis()),
            usage: ev.usage.map(|u| DashboardUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                reasoning_tokens: u.reasoning_tokens,
                cached_tokens: u.cached_tokens,
            }),
        }
    }
}

pub struct DashboardObserver {
    tx: broadcast::Sender<DashboardEvent>,
    recent: Arc<Mutex<VecDeque<DashboardEvent>>>,
}

impl DashboardObserver {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            tx,
            recent: Arc::new(Mutex::new(VecDeque::with_capacity(200))),
        }
    }
}

#[async_trait::async_trait]
impl Observer for DashboardObserver {
    async fn on_completion(&self, event: CompletionEvent) {
        let dash_ev = DashboardEvent::from(event);

        {
            let mut lock = self.recent.lock().unwrap();
            if lock.len() >= 200 {
                lock.pop_front();
            }
            lock.push_back(dash_ev.clone());
        }

        let _ = self.tx.send(dash_ev);
    }
}

pub fn router(observer: Arc<DashboardObserver>, shutdown_tx: broadcast::Sender<()>) -> Router {
    Router::new()
        .route("/", get(root_redirect))
        .route("/_cram/", get(serve_index))
        .route("/_cram/pico.min.css", get(serve_css))
        .route("/_cram/history", get(get_history))
        .route("/_cram/events", get(sse_events))
        .with_state((observer, shutdown_tx))
}

async fn root_redirect() -> impl IntoResponse {
    (StatusCode::FOUND, [(header::LOCATION, "/_cram/")])
}

async fn serve_index() -> impl IntoResponse {
    match Assets::get("index.html") {
        Some(content) => ([(header::CONTENT_TYPE, "text/html")], content.data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn serve_css() -> impl IntoResponse {
    match Assets::get("pico.min.css") {
        Some(content) => ([(header::CONTENT_TYPE, "text/css")], content.data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_history(
    State((obs, _)): State<(Arc<DashboardObserver>, broadcast::Sender<()>)>,
) -> Json<Vec<DashboardEvent>> {
    let lock = obs.recent.lock().unwrap();
    Json(lock.iter().cloned().collect())
}

async fn sse_events(
    State((obs, shutdown_tx)): State<(Arc<DashboardObserver>, broadcast::Sender<()>)>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = obs.tx.subscribe();

    let mut shutdown_rx = shutdown_tx.subscribe();
    let shutdown_future = async move {
        let _ = shutdown_rx.recv().await;
    };

    let stream = BroadcastStream::new(rx).filter_map(|res| match res {
        Ok(ev) => {
            let json = serde_json::to_string(&ev).unwrap_or_default();
            Some(Ok(Event::default().data(json)))
        }
        Err(_) => None,
    });

    let stream = async_stream::stream! {
        tokio::pin!(stream);
        tokio::pin!(shutdown_future);
        loop {
            tokio::select! {
                _ = &mut shutdown_future => {
                    break;
                }
                val = stream.next() => {
                    match val {
                        Some(ev) => yield ev,
                        None => break,
                    }
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
