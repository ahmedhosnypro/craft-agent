//! From<reqwest::Error> mapping exercised over loopback connections:
//! refused connects, hand-written status responses, and request timeouts
//! all land in the closed taxonomy.

use craft_core::error::{ProviderError, parse_retry_after};
use std::error::Error;
use std::time::Duration;

type TestError = Box<dyn Error + Send + Sync>;

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_default()
}

#[tokio::test]
async fn refused_connect_maps_to_transport() -> Result<(), TestError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    drop(listener);
    let url = format!("http://{addr}/");
    let err = client()
        .get(&url)
        .send()
        .await
        .err()
        .ok_or("connect to a dead port should fail")?;
    let mapped = ProviderError::from(err);
    assert!(
        matches!(mapped, ProviderError::Transport { .. }),
        "refused connect must map to Transport, got {mapped:?}"
    );
    Ok(())
}

async fn status_server(raw: &'static str) -> Result<String, TestError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let _ = socket.write_all(raw.as_bytes()).await;
        }
    });
    Ok(format!("http://{addr}/"))
}

async fn status_error(url: &str) -> Result<ProviderError, TestError> {
    let response = client().get(url).send().await?;
    let err = response
        .error_for_status()
        .err()
        .ok_or("the hand-written response should carry an error status")?;
    Ok(ProviderError::from(err))
}

#[tokio::test]
async fn unauthorized_response_maps_to_auth() -> Result<(), TestError> {
    let url = status_server("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n").await?;
    let mapped = status_error(&url).await?;
    assert!(
        matches!(mapped, ProviderError::Auth { .. }),
        "got {mapped:?}"
    );
    Ok(())
}

#[tokio::test]
async fn rate_limited_response_maps_without_header_knowledge() -> Result<(), TestError> {
    let url = status_server(
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\n\r\n",
    )
    .await?;
    let mapped = status_error(&url).await?;
    assert!(
        matches!(mapped, ProviderError::RateLimit { retry_after: None }),
        "From cannot see response headers, got {mapped:?}"
    );
    assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
    Ok(())
}

#[tokio::test]
async fn server_error_response_maps_to_transient() -> Result<(), TestError> {
    let url =
        status_server("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n").await?;
    let mapped = status_error(&url).await?;
    assert!(
        matches!(mapped, ProviderError::Transient { .. }),
        "got {mapped:?}"
    );
    Ok(())
}

#[tokio::test]
async fn request_timeout_maps_to_transient() -> Result<(), TestError> {
    // A bound listener that never accepts nor writes: the connect succeeds
    // through the kernel backlog and the response never arrives.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}/");
    let err = client()
        .get(&url)
        .timeout(Duration::from_millis(75))
        .send()
        .await
        .err()
        .ok_or("the silent server should trip the request timeout")?;
    let mapped = ProviderError::from(err);
    assert!(
        matches!(mapped, ProviderError::Transient { .. }),
        "timeout must map to Transient, got {mapped:?}"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_url_maps_to_invalid_request() -> Result<(), TestError> {
    let err = client()
        .get("not a url at all")
        .send()
        .await
        .err()
        .ok_or("a malformed url must fail the request")?;
    let mapped = ProviderError::from(err);
    assert!(
        matches!(mapped, ProviderError::InvalidRequest { .. }),
        "builder failures must map to InvalidRequest, got {mapped:?}"
    );
    Ok(())
}
