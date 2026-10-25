//! The token half of the credentials: RS256 JWT assertions, the two
//! OAuth grants, and the expiry-aware cache behind the bearer source.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use craft_core::error::ProviderError;
use rsa::pkcs1v15::SigningKey;
use rsa::signature::{SignatureEncoding, Signer};
use serde::Deserialize;
use sha2::Sha256;
use tokio::sync::Mutex;

use super::Credential;
use super::tagged;

/// The OAuth token endpoint the ADC flows exchange credentials at.
pub(super) const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// The cloud-wide scope a service-account JWT requests; the local ADC
/// file created by the gcloud login flow carries the same scope.
pub(super) const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// The documented lifetime of a service-account JWT assertion.
const JWT_LIFETIME_SECS: u64 = 3600;

/// A cached access token stays live this far ahead of its expiry; inside
/// the margin the next caller refreshes instead of racing the deadline.
const EXPIRY_MARGIN_SECS: u64 = 60;

/// The documented token response.
#[derive(Deserialize)]
struct WireToken {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// The live token cache: the token and the epoch second it dies at.
#[derive(Clone)]
struct Cached {
    access_token: String,
    expires_at: u64,
}

/// One credential bound to one token endpoint, holding at most one live
/// access token. Refreshes serialize under an async mutex, so concurrent
/// callers share one exchange instead of stampeding the endpoint.
pub struct TokenSource {
    client: reqwest::Client,
    token_url: String,
    credential: Credential,
    cache: Mutex<Option<Cached>>,
}

impl TokenSource {
    /// Mounts the token source over the ADC credential and the
    /// documented token endpoint.
    pub fn new(credential: Credential) -> Result<Self, ProviderError> {
        Self::at_endpoint(credential, TOKEN_URL)
    }

    /// The same mount over an explicit endpoint; tests point it at a
    /// loopback mock.
    pub fn at_endpoint(credential: Credential, token_url: &str) -> Result<Self, ProviderError> {
        Ok(Self {
            client: crate::transport::client()?,
            token_url: token_url.to_owned(),
            credential,
            cache: Mutex::new(None),
        })
    }

    /// A bearer access token: the cached one until shortly before its
    /// expiry, else one fresh exchange; refreshes serialize under the
    /// cache lock so concurrent callers share one exchange.
    pub async fn access_token(&self) -> Result<String, ProviderError> {
        if let Some(token) = self.cached_token()? {
            return Ok(token);
        }
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.as_ref() {
            let now = unix_now()?;
            if cached.expires_at > now.saturating_add(EXPIRY_MARGIN_SECS) {
                return Ok(cached.access_token.clone());
            }
        }
        let token = self.exchange().await?;
        *cache = Some(token.clone());
        Ok(token.access_token)
    }

    /// The cached token while it stays outside the expiry margin; a lock
    /// already held by a refresh is an empty probe, not a wait.
    fn cached_token(&self) -> Result<Option<String>, ProviderError> {
        let now = unix_now()?;
        let guard = self.cache.try_lock().ok();
        let Some(cached) = guard.as_deref().and_then(Option::as_ref) else {
            return Ok(None);
        };
        if cached.expires_at > now.saturating_add(EXPIRY_MARGIN_SECS) {
            return Ok(Some(cached.access_token.clone()));
        }
        Ok(None)
    }

    /// Posts the credential's grant and reads the token response; the
    /// shared ladder retries the retryable rows (timeouts, 5xx, rate
    /// limits) and the OAuth failure body refines the classification.
    async fn exchange(&self) -> Result<Cached, ProviderError> {
        let params = grant_params(&self.credential, &self.token_url)?;
        let token_url = self.token_url.clone();
        let response = crate::retry::climb(
            &self.client,
            move |client| {
                client
                    .post(&token_url)
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .form(&params)
            },
            token_failure,
            craft_core::error::RETRY_PATIENCE_DEFAULT,
        )
        .await?;
        let body = crate::retry::read_capped(response, crate::retry::ERROR_BODY_CAP_BYTES).await;
        let token: WireToken =
            serde_json::from_str(&body).map_err(|err| tagged("token response", err))?;
        let expires_at = unix_now()?.saturating_add(token.expires_in.unwrap_or(JWT_LIFETIME_SECS));
        Ok(Cached {
            access_token: token.access_token,
            expires_at,
        })
    }
}

/// Seconds since the Unix epoch; a pre-epoch clock is a named error
/// rather than a silently backward-dated assertion.
fn unix_now() -> Result<u64, ProviderError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .map_err(|err| tagged("system clock", err))
}

/// The documented identity claims of one assertion: the key id paired
/// with the signing key when the credential carries one, the service
/// account as issuer and subject, the token endpoint as audience, and
/// the issue second.
struct Assertion<'a> {
    key_id: Option<&'a str>,
    iss: &'a str,
    aud: &'a str,
    iat: u64,
}

/// The form params of the credential's OAuth grant: the freshly signed
/// JWT assertion for a service account, the refresh-token grant for a
/// user credential.
fn grant_params(
    credential: &Credential,
    token_url: &str,
) -> Result<Vec<(String, String)>, ProviderError> {
    match credential {
        Credential::ServiceAccount(account) => {
            let assertion = Assertion {
                key_id: account.private_key_id.as_deref(),
                iss: &account.client_email,
                aud: token_url,
                iat: unix_now()?,
            };
            Ok(vec![
                (
                    String::from("grant_type"),
                    String::from("urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ),
                (
                    String::from("assertion"),
                    sign_jwt(&account.signing_key, &assertion)?,
                ),
            ])
        }
        Credential::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => Ok(vec![
            (String::from("grant_type"), String::from("refresh_token")),
            (String::from("client_id"), client_id.clone()),
            (String::from("client_secret"), client_secret.clone()),
            (String::from("refresh_token"), refresh_token.clone()),
        ]),
    }
}

/// Signs one RS256 JWT: base64url(header).base64url(claims) over the
/// documented claim set, then the signature joins with a dot.
fn sign_jwt(
    signing_key: &SigningKey<Sha256>,
    assertion: &Assertion<'_>,
) -> Result<String, ProviderError> {
    let mut header = serde_json::json!({"alg": "RS256", "typ": "JWT"});
    if let Some(key_id) = assertion.key_id {
        header["kid"] = serde_json::Value::from(key_id);
    }
    let claims = serde_json::json!({
        "iss": assertion.iss,
        "sub": assertion.iss,
        "scope": CLOUD_PLATFORM_SCOPE,
        "aud": assertion.aud,
        "iat": assertion.iat,
        "exp": assertion.iat.saturating_add(JWT_LIFETIME_SECS),
    });
    let encode = |value: &serde_json::Value| -> Result<String, ProviderError> {
        serde_json::to_vec(value)
            .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
            .map_err(|err| tagged("JWT serialization", err))
    };
    let head = encode(&header)?;
    let body = encode(&claims)?;
    let signature = signing_key
        .try_sign(format!("{head}.{body}").as_bytes())
        .map_err(|err| tagged("JWT signing", err))?;
    Ok(format!(
        "{head}.{body}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_vec())
    ))
}

/// The token endpoint's non-2xx rows: the OAuth error body names the
/// rejection, client-side statuses fail fast as auth rejections, and
/// the retryable ones ride the ladder.
async fn token_failure(response: reqwest::Response) -> ProviderError {
    let status = response.status();
    let body = crate::retry::read_capped(response, crate::retry::ERROR_BODY_CAP_BYTES).await;
    let detail = oauth_detail(&body).unwrap_or_else(|| status.to_string());
    match status.as_u16() {
        400 | 401 | 403 => ProviderError::Auth { detail },
        429 => ProviderError::RateLimit { retry_after: None },
        408 => ProviderError::Transient { detail },
        _ if status.is_server_error() => ProviderError::Transient { detail },
        _ => ProviderError::from_http_status(status),
    }
}

/// The OAuth error body's code and description, when present.
fn oauth_detail(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let code = value.get("error").and_then(serde_json::Value::as_str)?;
    let description = value
        .get("error_description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    Some(format!("{code}: {description}"))
}
