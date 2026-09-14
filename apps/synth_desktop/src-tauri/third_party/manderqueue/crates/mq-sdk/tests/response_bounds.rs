use axum::{
    body::{Body, Bytes},
    http::StatusCode,
    response::Response,
    routing::get,
    Router,
};
use mq_core::ThreadId;
use mq_sdk::{MqClient, SdkError};
use tokio::net::TcpListener;

#[tokio::test]
async fn oversized_fixed_and_chunked_bodies_are_refused_without_losing_error_status() {
    for chunked in [false, true] {
        for status in [StatusCode::OK, StatusCode::FORBIDDEN] {
            let app = Router::new().route(
                "/v1/threads/{id}/messages",
                get(move || async move {
                    let body = if chunked {
                        Body::from_stream(futures_util::stream::iter((0..17).map(|_| {
                            Ok::<_, std::convert::Infallible>(Bytes::from(vec![b'x'; 1024 * 1024]))
                        })))
                    } else {
                        Body::from(vec![b'x'; 17 * 1024 * 1024])
                    };
                    Response::builder().status(status).body(body).unwrap()
                }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let client = MqClient::new(format!("http://{address}"), "fixture");
            let error = client
                .read_messages(ThreadId::new(), 0, 1)
                .await
                .unwrap_err();
            match error {
                SdkError::Decode(message) if status == StatusCode::OK => {
                    assert!(message.contains("exceeds limit"))
                }
                SdkError::Api {
                    status: observed,
                    body,
                } if status == StatusCode::FORBIDDEN => {
                    assert_eq!(observed, status);
                    assert_eq!(body, "response body exceeds limit");
                }
                other => panic!("unexpected refusal: {other}"),
            }
            server.abort();
        }
    }
}
