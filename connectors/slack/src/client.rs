use anyhow::{Result, anyhow};
use omni_connector_sdk::{RateLimiter, RetryableError};
use reqwest::Client;
use serde::Deserialize;
use std::fmt;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::models::{
    ChatGetPermalinkResponse, ConversationInfoResponse, ConversationsHistoryResponse,
    ConversationsListResponse, ConversationsMembersResponse, SlackFile, UsersListResponse,
};

const DEFAULT_SLACK_API_BASE: &str = "https://slack.com/api";
const DEFAULT_SLACK_MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;

fn slack_max_download_bytes() -> u64 {
    std::env::var("SLACK_MAX_DOWNLOAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SLACK_MAX_DOWNLOAD_BYTES)
}

async fn read_response_bytes_limited(
    mut response: reqwest::Response,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>> {
    if response
        .content_length()
        .is_some_and(|content_length| content_length > max_bytes)
    {
        return Ok(None);
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let exceeds_limit = bytes
            .len()
            .checked_add(chunk.len())
            .is_none_or(|next_len| next_len as u64 > max_bytes);
        if exceeds_limit {
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(Some(bytes))
}

/// Subset of every Slack response we use to detect API-level errors before
/// attempting to deserialize into the typed success shape (which would fail
/// with a misleading `missing field` for the absent success-only fields).
#[derive(Deserialize)]
struct SlackErrorEnvelope {
    ok: bool,
    error: Option<String>,
    needed: Option<String>,
    provided: Option<String>,
}

#[derive(Debug)]
struct SlackApiError {
    code: String,
    needed: Option<String>,
    provided: Option<String>,
}

impl SlackApiError {
    fn missing_conversation_types(&self) -> Vec<&'static str> {
        if self.code != "missing_scope" {
            return Vec::new();
        }

        self.needed
            .as_deref()
            .into_iter()
            .flat_map(|scopes| scopes.split(|c: char| c == ',' || c.is_whitespace()))
            .filter_map(|scope| match scope {
                "im:read" => Some("im"),
                "mpim:read" => Some("mpim"),
                _ => None,
            })
            .collect()
    }
}

impl From<SlackErrorEnvelope> for SlackApiError {
    fn from(envelope: SlackErrorEnvelope) -> Self {
        Self {
            code: envelope.error.unwrap_or_else(|| "unknown".to_string()),
            needed: envelope.needed,
            provided: envelope.provided,
        }
    }
}

impl fmt::Display for SlackApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Slack API error: {}", self.code)?;
        match (self.needed.as_deref(), self.provided.as_deref()) {
            (Some(needed), Some(provided)) => {
                write!(f, " (needed: {needed}; provided: {provided})")
            }
            (Some(needed), None) => write!(f, " (needed: {needed})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for SlackApiError {}

pub(crate) struct ConversationListSession {
    conversation_types: Vec<&'static str>,
}

impl Default for ConversationListSession {
    fn default() -> Self {
        Self {
            conversation_types: vec!["public_channel", "private_channel", "mpim", "im"],
        }
    }
}

pub struct SlackClient {
    client: Client,
    rate_limiter: RateLimiter,
    base_url: String,
}

impl SlackClient {
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_SLACK_API_BASE.to_string())
    }

    pub fn with_base_url(base_url: String) -> Self {
        Self {
            client: Client::new(),
            // Slack Tier 3 allows ~50 req/min; 1 req/sec keeps us safely under.
            rate_limiter: RateLimiter::new(1, 5),
            base_url,
        }
    }

    fn extract_retry_after(response: &reqwest::Response) -> Duration {
        response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(1))
    }

    async fn make_request<T>(&self, url: &str, token: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        debug!("Making request to: {}", url);

        self.rate_limiter
            .execute_with_retry(|| async {
                let response = self
                    .client
                    .get(url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("Content-Type", "application/json")
                    .send()
                    .await
                    .map_err(|e| RetryableError::Transient(e.into()))?;

                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    return Err(RetryableError::RateLimited {
                        retry_after: Self::extract_retry_after(&response),
                        message: format!("Slack API rate limited: {}", url),
                    });
                }

                if !response.status().is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    return Err(RetryableError::Permanent(anyhow!(
                        "API request failed: {}",
                        error_text
                    )));
                }

                let response_text = response
                    .text()
                    .await
                    .map_err(|e| RetryableError::Transient(e.into()))?;
                debug!("Response: {}", response_text);

                // Slack API errors come back as 200s with `{ok:false, error:"..."}`
                // and omit the success-shape fields the typed responses require.
                // Surface the `error` cleanly here so callers see e.g.
                // "Slack API error: missing_scope (needed: mpim:read)" instead
                // of a confusing `missing field 'channels'` from serde.
                if let Ok(envelope) = serde_json::from_str::<SlackErrorEnvelope>(&response_text)
                    && !envelope.ok
                {
                    return Err(RetryableError::Permanent(anyhow!(SlackApiError::from(
                        envelope
                    ))));
                }

                serde_json::from_str(&response_text).map_err(|e| {
                    RetryableError::Permanent(anyhow!("Failed to parse response: {}", e))
                })
            })
            .await
    }

    pub async fn list_conversations(
        &self,
        token: &str,
        cursor: Option<&str>,
    ) -> Result<ConversationsListResponse> {
        let mut session = ConversationListSession::default();
        self.list_conversations_in_session(token, cursor, &mut session)
            .await
    }

    pub(crate) async fn list_conversations_in_session(
        &self,
        token: &str,
        cursor: Option<&str>,
        session: &mut ConversationListSession,
    ) -> Result<ConversationsListResponse> {
        loop {
            let mut url = format!(
                "{}/conversations.list?types={}&limit=200",
                self.base_url,
                session.conversation_types.join(",")
            );

            if let Some(cursor) = cursor {
                url.push_str(&format!("&cursor={}", cursor));
            }

            let response: ConversationsListResponse = match self.make_request(&url, token).await {
                Ok(response) => response,
                Err(error) => {
                    let Some(api_error) = error.downcast_ref::<SlackApiError>() else {
                        return Err(error);
                    };
                    let missing_types = api_error.missing_conversation_types();
                    let previous_len = session.conversation_types.len();
                    session
                        .conversation_types
                        .retain(|kind| !missing_types.contains(kind));
                    if session.conversation_types.len() == previous_len {
                        return Err(error);
                    }

                    warn!(
                        "Slack conversations.list is missing scope(s) {}; retrying without unsupported type(s) {} (remaining: {})",
                        api_error.needed.as_deref().unwrap_or("unknown"),
                        missing_types.join(","),
                        session.conversation_types.join(",")
                    );
                    continue;
                }
            };

            if !response.ok {
                return Err(anyhow!(
                    "conversations.list failed: {}",
                    response.error.unwrap_or("Unknown error".to_string())
                ));
            }

            info!("Found {} channels", response.channels.len());
            return Ok(response);
        }
    }

    pub async fn get_conversation_history(
        &self,
        token: &str,
        channel_id: &str,
        cursor: Option<&str>,
        oldest: Option<&str>,
        latest: Option<&str>,
    ) -> Result<ConversationsHistoryResponse> {
        let mut url = format!(
            "{}/conversations.history?channel={}&limit=200",
            self.base_url, channel_id
        );

        if let Some(cursor) = cursor {
            url.push_str(&format!("&cursor={}", cursor));
        }
        if let Some(oldest) = oldest {
            url.push_str(&format!("&oldest={}", oldest));
        }
        if let Some(latest) = latest {
            url.push_str(&format!("&latest={}", latest));
        }

        let response: ConversationsHistoryResponse = self.make_request(&url, token).await?;

        if !response.ok {
            return Err(anyhow!(
                "conversations.history failed: {}",
                response.error.unwrap_or("Unknown error".to_string())
            ));
        }

        debug!(
            "Retrieved {} messages from channel {}",
            response.messages.len(),
            channel_id
        );
        Ok(response)
    }

    pub async fn get_thread_replies(
        &self,
        token: &str,
        channel_id: &str,
        thread_ts: &str,
        cursor: Option<&str>,
    ) -> Result<ConversationsHistoryResponse> {
        let mut url = format!(
            "{}/conversations.replies?channel={}&ts={}&limit=200",
            self.base_url, channel_id, thread_ts
        );

        if let Some(cursor) = cursor {
            url.push_str(&format!("&cursor={}", cursor));
        }

        let response: ConversationsHistoryResponse = self.make_request(&url, token).await?;

        if !response.ok {
            return Err(anyhow!(
                "conversations.replies failed: {}",
                response.error.unwrap_or("Unknown error".to_string())
            ));
        }

        debug!(
            "Retrieved {} thread replies for ts {}",
            response.messages.len(),
            thread_ts
        );
        Ok(response)
    }

    pub async fn join_conversation(&self, token: &str, channel_id: &str) -> Result<()> {
        let url = format!("{}/conversations.join", self.base_url);
        let payload = serde_json::json!({ "channel": channel_id });

        self.rate_limiter
            .execute_with_retry(|| async {
                let response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("Content-Type", "application/json")
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| RetryableError::Transient(e.into()))?;

                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    return Err(RetryableError::RateLimited {
                        retry_after: Self::extract_retry_after(&response),
                        message: "Slack API rate limited: conversations.join".to_string(),
                    });
                }

                let body: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| RetryableError::Transient(e.into()))?;
                if body.get("ok") == Some(&serde_json::Value::Bool(true)) {
                    Ok(())
                } else {
                    let err = body["error"].as_str().unwrap_or("Unknown error");
                    Err(RetryableError::Permanent(anyhow!(
                        "conversations.join failed: {}",
                        err
                    )))
                }
            })
            .await
    }

    pub async fn list_users(&self, token: &str, cursor: Option<&str>) -> Result<UsersListResponse> {
        let mut url = format!("{}/users.list?limit=200", self.base_url);

        if let Some(cursor) = cursor {
            url.push_str(&format!("&cursor={}", cursor));
        }

        let response: UsersListResponse = self.make_request(&url, token).await?;

        if !response.ok {
            return Err(anyhow!(
                "users.list failed: {}",
                response.error.unwrap_or("Unknown error".to_string())
            ));
        }

        info!("Found {} users", response.members.len());
        Ok(response)
    }

    pub async fn get_conversation_info(
        &self,
        token: &str,
        channel_id: &str,
    ) -> Result<crate::models::SlackChannel> {
        let url = format!(
            "{}/conversations.info?channel={}",
            self.base_url, channel_id
        );

        let response: ConversationInfoResponse = self.make_request(&url, token).await?;

        if !response.ok {
            return Err(anyhow!(
                "conversations.info failed: {}",
                response.error.unwrap_or("Unknown error".to_string())
            ));
        }

        Ok(response.channel)
    }

    pub async fn get_permalink(
        &self,
        token: &str,
        channel_id: &str,
        message_ts: &str,
    ) -> Result<String> {
        let url = format!(
            "{}/chat.getPermalink?channel={}&message_ts={}",
            self.base_url, channel_id, message_ts
        );

        let response: ChatGetPermalinkResponse = self.make_request(&url, token).await?;

        if !response.ok {
            return Err(anyhow!(
                "chat.getPermalink failed: {}",
                response.error.unwrap_or("Unknown error".to_string())
            ));
        }

        Ok(response.permalink)
    }

    pub async fn get_conversation_members(
        &self,
        token: &str,
        channel_id: &str,
        cursor: Option<&str>,
    ) -> Result<ConversationsMembersResponse> {
        let mut url = format!(
            "{}/conversations.members?channel={}&limit=200",
            self.base_url, channel_id
        );

        if let Some(cursor) = cursor {
            url.push_str(&format!("&cursor={}", cursor));
        }

        let response: ConversationsMembersResponse = self.make_request(&url, token).await?;

        if !response.ok {
            return Err(anyhow!(
                "conversations.members failed: {}",
                response.error.unwrap_or("Unknown error".to_string())
            ));
        }

        debug!(
            "Retrieved {} members from channel {}",
            response.members.len(),
            channel_id
        );
        Ok(response)
    }

    /// Download a file's bytes via its `url_private_download`. Returns
    /// `Some((bytes, content_type))` on success, `None` if the file has no
    /// download URL or the download returned a non-success HTTP status (e.g.
    /// the file was deleted, or the bot lost access). The caller decides what
    /// to do with the bytes — typically `ctx.extract_and_store_content` to
    /// route through the connector-manager's extractor (Docling for binary,
    /// utf8 decode for text).
    pub async fn download_file(
        &self,
        token: &str,
        file: &SlackFile,
    ) -> Result<Option<(Vec<u8>, String)>> {
        let max_download_bytes = slack_max_download_bytes();
        if file.size > 0 && file.size as u64 > max_download_bytes {
            warn!(
                "Skipping oversized Slack file {} ({} bytes > {} byte download limit)",
                file.id, file.size, max_download_bytes
            );
            return Ok(None);
        }

        let Some(download_url) = &file.url_private_download else {
            return Ok(None);
        };

        debug!("Downloading file: {} ({})", file.display_name(), file.id);

        self.rate_limiter
            .execute_with_retry(|| async {
                let response = self
                    .client
                    .get(download_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send()
                    .await
                    .map_err(|e| RetryableError::Transient(e.into()))?;

                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    return Err(RetryableError::RateLimited {
                        retry_after: Self::extract_retry_after(&response),
                        message: format!(
                            "Slack API rate limited downloading file: {}",
                            file.display_name()
                        ),
                    });
                }

                if !response.status().is_success() {
                    warn!(
                        "Failed to download file {}: HTTP {}",
                        file.display_name(),
                        response.status()
                    );
                    return Ok(None);
                }

                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|ct| ct.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();

                let Some(bytes) = read_response_bytes_limited(response, max_download_bytes)
                    .await
                    .map_err(RetryableError::Transient)?
                else {
                    warn!(
                        "Skipping oversized Slack file {} (response exceeds {} byte download limit)",
                        file.id, max_download_bytes
                    );
                    return Ok(None);
                };

                Ok(Some((bytes, content_type)))
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    const DEFAULT_MAX_DOWNLOAD_BYTES: i64 = 50 * 1024 * 1024;

    struct TestHttpServer {
        url: String,
        hits: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<String>>>,
        handle: JoinHandle<()>,
    }

    impl TestHttpServer {
        async fn start(response: &'static [u8]) -> Self {
            Self::start_sequence(vec![response]).await
        }

        async fn start_sequence(responses: Vec<&'static [u8]>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let hits = Arc::new(AtomicUsize::new(0));
            let server_hits = hits.clone();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let server_requests = requests.clone();
            let handle = tokio::spawn(async move {
                for response in responses {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    server_hits.fetch_add(1, Ordering::SeqCst);

                    let mut request = [0_u8; 2048];
                    let bytes_read = stream.read(&mut request).await.unwrap_or_default();
                    server_requests
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&request[..bytes_read]).into_owned());
                    let _ = stream.write_all(response).await;
                    let _ = stream.shutdown().await;
                }
            });

            Self {
                url: format!("http://{}", addr),
                hits,
                requests,
                handle,
            }
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for TestHttpServer {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    fn test_file(url: String, size: i64) -> SlackFile {
        SlackFile {
            id: "F_TEST".to_string(),
            name: "test.bin".to_string(),
            title: None,
            mimetype: Some("application/octet-stream".to_string()),
            size,
            url_private: None,
            url_private_download: Some(url),
            permalink: None,
        }
    }

    #[tokio::test]
    async fn oversized_file_metadata_skips_before_http_request() {
        let server = TestHttpServer::start(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx").await;
        let client = SlackClient::new();
        let file = test_file(server.url.clone(), DEFAULT_MAX_DOWNLOAD_BYTES + 1);

        let result = client.download_file("test-token", &file).await.unwrap();

        assert!(result.is_none(), "oversized files must be skipped");
        assert_eq!(
            server.hits.load(Ordering::SeqCst),
            0,
            "metadata size must be checked before issuing the download"
        );
    }

    #[tokio::test]
    async fn response_content_length_above_limit_is_rejected_before_body_read() {
        let server =
            TestHttpServer::start(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nabcde").await;
        let response = reqwest::get(&server.url).await.unwrap();

        let bytes = read_response_bytes_limited(response, 4).await.unwrap();

        assert!(bytes.is_none(), "declared oversized bodies must be skipped");
    }

    #[tokio::test]
    async fn chunked_response_above_limit_is_rejected_while_streaming() {
        let server = TestHttpServer::start(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
        )
        .await;
        let response = reqwest::get(&server.url).await.unwrap();

        let bytes = read_response_bytes_limited(response, 4).await.unwrap();

        assert!(
            bytes.is_none(),
            "streamed bodies must stop once they exceed the download limit"
        );
    }

    #[tokio::test]
    async fn chunked_response_at_limit_is_accepted() {
        let server = TestHttpServer::start(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
        )
        .await;
        let response = reqwest::get(&server.url).await.unwrap();

        let bytes = read_response_bytes_limited(response, 5)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(bytes, b"abcde");
    }

    #[tokio::test]
    async fn download_file_below_limit_returns_body_and_content_type() {
        let server = TestHttpServer::start(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nabcde",
        )
        .await;
        let client = SlackClient::new();
        let file = test_file(server.url.clone(), 5);

        let (bytes, content_type) = client
            .download_file("test-token", &file)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(bytes, b"abcde");
        assert_eq!(content_type, "text/plain");
        assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn conversation_list_drops_only_types_with_missing_scopes() {
        let server = TestHttpServer::start_sequence(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":false,\"error\":\"missing_scope\",\"needed\":\"im:read\",\"provided\":\"channels:read,groups:read\"}",
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":false,\"error\":\"missing_scope\",\"needed\":\"mpim:read\",\"provided\":\"channels:read,groups:read\"}",
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":true,\"channels\":[],\"response_metadata\":{\"next_cursor\":\"\"},\"error\":null}",
        ])
        .await;
        let client = SlackClient::with_base_url(server.url.clone());

        let response = client.list_conversations("test-token", None).await.unwrap();

        assert!(response.channels.is_empty());
        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        assert!(
            requests[0].contains("types=public_channel,private_channel,mpim,im&limit=200"),
            "the first request must preserve IM and MPIM coverage"
        );
        assert!(
            requests[1].contains("types=public_channel,private_channel,mpim&limit=200"),
            "missing im:read must remove only the im type"
        );
        assert!(
            requests[2].contains("types=public_channel,private_channel&limit=200"),
            "missing mpim:read must then remove only the mpim type"
        );
    }

    #[tokio::test]
    async fn conversation_list_reuses_scope_fallback_across_pages() {
        let server = TestHttpServer::start_sequence(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":false,\"error\":\"missing_scope\",\"needed\":\"im:read\"}",
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":false,\"error\":\"missing_scope\",\"needed\":\"mpim:read\"}",
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":true,\"channels\":[],\"response_metadata\":{\"next_cursor\":\"next\"},\"error\":null}",
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":true,\"channels\":[],\"response_metadata\":{\"next_cursor\":\"\"},\"error\":null}",
        ])
        .await;
        let client = SlackClient::with_base_url(server.url.clone());
        let mut session = ConversationListSession::default();

        client
            .list_conversations_in_session("test-token", None, &mut session)
            .await
            .unwrap();
        client
            .list_conversations_in_session("test-token", Some("next"), &mut session)
            .await
            .unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 4);
        assert!(
            requests[3].contains("types=public_channel,private_channel&limit=200&cursor=next"),
            "later pages must reuse the already degraded conversation types"
        );
    }
}
