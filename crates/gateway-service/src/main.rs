use axum::body::{Body, to_bytes};
use axum::extract::ws::Message;
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Router, routing::get};
use futures_util::{SinkExt, StreamExt};
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_ADDR: &str = "127.0.0.1:8080";

#[derive(Clone)]
struct Upstream {
    base: String,
    client: reqwest::Client,
}

#[derive(Clone)]
struct GatewayState {
    auth: Upstream,
    catalog: Upstream,
    chat: Upstream,
    orders: Upstream,
    admin: Upstream,
}

async fn forward(upstream: &Upstream, uri: Uri, method: Method, headers: HeaderMap, body: Body) -> Response {
    let mut url = format!("{}{}", upstream.base, uri.path());
    if let Some(query) = uri.query() {
        url.push('?');
        url.push_str(query);
    }

    let mut req = upstream
        .client
        .request(method.clone(), &url)
        .body(match to_bytes(body, 2 * 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!(error = %e, "не удалось прочитать тело запроса");
                return (StatusCode::BAD_REQUEST, "invalid body".to_string()).into_response();
            }
        });

    for (name, value) in headers.iter() {
        if name == HOST || name == CONTENT_LENGTH {
            continue;
        }
        req = req.header(name, value.clone());
    }

    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            let content_type = resp
                .headers()
                .get(CONTENT_TYPE)
                .cloned()
                .unwrap_or(HeaderValue::from_static("application/json"));
            let bytes = match resp.bytes().await {
                Ok(b) => b.to_vec(),
                Err(e) => {
                    tracing::error!(error = %e, "не удалось прочитать ответ апстрима");
                    return StatusCode::BAD_GATEWAY.into_response();
                }
            };
            (status, [(CONTENT_TYPE, content_type)], bytes).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "не удалось обратиться к апстриму {}", upstream.base);
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

async fn proxy_auth(
    State(s): State<GatewayState>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    forward(&s.auth, uri, method, headers, body).await
}

async fn proxy_catalog(
    State(s): State<GatewayState>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    forward(&s.catalog, uri, method, headers, body).await
}

async fn proxy_chat(
    State(s): State<GatewayState>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    forward(&s.chat, uri, method, headers, body).await
}

async fn proxy_orders(
    State(s): State<GatewayState>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    forward(&s.orders, uri, method, headers, body).await
}

async fn proxy_admin(
    State(s): State<GatewayState>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    forward(&s.admin, uri, method, headers, body).await
}

async fn ws_proxy(
    State(s): State<GatewayState>,
    ws: WebSocketUpgrade,
    uri: Uri,
) -> Response {
    let chat_base = s.chat.base.clone();
    let path = uri.path().to_string();
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();

    ws.on_upgrade(move |client_socket| async move {
        let ws_base = if let Some(rest) = chat_base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else if let Some(rest) = chat_base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else {
            chat_base.clone()
        };
        let url = format!("{ws_base}{path}{query}");
        tracing::info!(url, "проксирование WebSocket");

        let backend_result = tokio_tungstenite::connect_async(&url).await;

        match backend_result {
            Ok((backend_socket, _response)) => {
                let (mut client_sender, mut client_receiver) = client_socket.split();
                let (mut backend_sender, mut backend_receiver) = backend_socket.split();

                let mut client_to_backend = tokio::spawn(async move {
                    while let Some(Ok(msg)) = client_receiver.next().await {
                        match msg {
                            Message::Text(text) => {
                                let _ = backend_sender
                                    .send(tokio_tungstenite::tungstenite::Message::text(text.to_string()))
                                    .await;
                            }
                            Message::Binary(bin) => {
                                let _ = backend_sender
                                    .send(tokio_tungstenite::tungstenite::Message::binary(bin.to_vec()))
                                    .await;
                            }
                            Message::Close(_) => {
                                let _ = backend_sender
                                    .send(tokio_tungstenite::tungstenite::Message::Close(None))
                                    .await;
                                break;
                            }
                            Message::Ping(p) => {
                                let _ = backend_sender
                                    .send(tokio_tungstenite::tungstenite::Message::Ping(p))
                                    .await;
                            }
                            Message::Pong(p) => {
                                let _ = backend_sender
                                    .send(tokio_tungstenite::tungstenite::Message::Pong(p))
                                    .await;
                            }
                        }
                    }
                });

                let mut backend_to_client = tokio::spawn(async move {
                    while let Some(Ok(msg)) = backend_receiver.next().await {
                        match msg {
                            tokio_tungstenite::tungstenite::Message::Text(text) => {
                                let _ = client_sender
                                    .send(Message::Text(text.to_string().into()))
                                    .await;
                            }
                            tokio_tungstenite::tungstenite::Message::Binary(bin) => {
                                let _ = client_sender
                                    .send(Message::Binary(bin.to_vec().into()))
                                    .await;
                            }
                            tokio_tungstenite::tungstenite::Message::Close(_) => {
                                let _ = client_sender.send(Message::Close(None)).await;
                                break;
                            }
                            tokio_tungstenite::tungstenite::Message::Ping(p) => {
                                let _ = client_sender.send(Message::Ping(p.to_vec().into())).await;
                            }
                            tokio_tungstenite::tungstenite::Message::Pong(p) => {
                                let _ = client_sender.send(Message::Pong(p.to_vec().into())).await;
                            }
                            _ => {}
                        }
                    }
                });

                tokio::select! {
                    _ = &mut client_to_backend => backend_to_client.abort(),
                    _ = &mut backend_to_client => client_to_backend.abort(),
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "не удалось подключиться к WebSocket бэкенду");
            }
        }
    })
}

fn upstream(base: &str) -> Upstream {
    Upstream {
        base: base.to_string(),
        client: reqwest::Client::new(),
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let addr = std::env::var("SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let auth_url = std::env::var("AUTH_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8081".into());
    let catalog_url =
        std::env::var("CATALOG_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8082".into());
    let chat_url = std::env::var("CHAT_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8083".into());
    let orders_url =
        std::env::var("ORDER_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8084".into());
    let admin_url =
        std::env::var("ADMIN_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8085".into());

    let static_dir = std::env::var("STATIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/static").to_string());

    let state = GatewayState {
        auth: upstream(&auth_url),
        catalog: upstream(&catalog_url),
        chat: upstream(&chat_url),
        orders: upstream(&orders_url),
        admin: upstream(&admin_url),
    };

    let static_files =
        ServeDir::new(&static_dir).fallback(ServeFile::new(format!("{static_dir}/index.html")));

    let app = Router::new()
        .route("/ws", get(ws_proxy))
        .route("/api/auth/{*rest}", any(proxy_auth))
        .route("/api/categories", any(proxy_catalog))
        .route("/api/masters", any(proxy_catalog))
        .route("/api/masters/{*rest}", any(proxy_catalog))
        .route("/api/geocode", any(proxy_catalog))
        .route("/uploads/{*rest}", any(proxy_catalog))
        .route("/api/chats", any(proxy_chat))
        .route("/api/chats/{*rest}", any(proxy_chat))
        .route("/api/orders", any(proxy_orders))
        .route("/api/orders/{*rest}", any(proxy_orders))
        .route("/api/admin", any(proxy_admin))
        .route("/api/admin/{*rest}", any(proxy_admin))
        .route("/api/feedback", any(proxy_admin))
        .route("/health", get(|| async { "ok" }))
        .fallback_service(static_files)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("не удалось занять порт");
    tracing::info!("gateway слушает http://{addr}");
    axum::serve(listener, app).await.expect("ошибка сервера");
}
