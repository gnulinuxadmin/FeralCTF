use axum::{
    extract::ws::{Message, WebSocket},
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::{AppState, models::scoreboard::TeamScore};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    NewSolve {
        team: String,
        challenge: String,
        points: i64,
        first_blood: bool,
    },
    Announcement {
        title: String,
        body: String,
    },
    StateChange {
        started: bool,
        ended: bool,
        frozen: bool,
    },
    ScoreUpdate {
        scoreboard: Vec<TeamScore>,
    },
}

pub struct WsHub {
    pub tx: broadcast::Sender<WsEvent>,
}

impl WsHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.tx.send(event); // no-op when no receivers
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.tx.subscribe()
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}

// GET /ws — no auth required; events are public scoreboard data
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.ws_hub.subscribe();
    // mpsc outbox: broadcast receiver and WebSocket sender are in separate tasks
    // so recv() and send() never hold conflicting &mut borrows of `socket`.
    let (outbox_tx, mut outbox_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    tokio::spawn({
        let outbox_tx = outbox_tx.clone();
        async move {
            let mut ping_timer = tokio::time::interval(Duration::from_secs(30));
            ping_timer.tick().await; // skip immediate first tick
            loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        Ok(event) => {
                            if let Ok(json) = serde_json::to_string(&event)
                                && outbox_tx.send(Message::Text(json.into())).is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    },
                    _ = ping_timer.tick() => {
                        if outbox_tx.send(Message::Ping(vec![].into())).is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            msg = outbox_rx.recv() => match msg {
                Some(m) => {
                    if socket.send(m).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(_)) => {} // clients don't send meaningful messages
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_event_serializes_with_type_tag() {
        let ev = WsEvent::NewSolve {
            team: "A".into(),
            challenge: "pwn".into(),
            points: 500,
            first_blood: true,
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""type":"new_solve""#));
        assert!(json.contains(r#""first_blood":true"#));
    }

    #[tokio::test]
    async fn hub_broadcast_received_by_subscriber() {
        let hub = WsHub::new();
        let mut rx = hub.subscribe();
        hub.broadcast(WsEvent::Announcement {
            title: "hello".into(),
            body: "world".into(),
        });
        assert!(matches!(
            rx.try_recv().unwrap(),
            WsEvent::Announcement { .. }
        ));
    }

    #[tokio::test]
    async fn lagged_receiver_is_silently_skipped() {
        let (tx, mut rx) = broadcast::channel::<WsEvent>(2);
        for _ in 0..10 {
            let _ = tx.send(WsEvent::StateChange {
                started: true,
                ended: false,
                frozen: false,
            });
        }
        assert!(matches!(
            rx.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
    }
}
