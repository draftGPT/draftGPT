//! Read-only HTTP access to the LCU.
//!
//! There is deliberately no generic request method: only GETs, plus the client's
//! `/help` introspection (a POST that only reads the schema). Nothing here can
//! change client state.

use serde_json::Value;

use super::TransportError;
use super::tls::client_config;
use crate::discovery::Credentials;

/// `/help` output formats. Neither is complete on its own: `Full` has types
/// and fields but no route, `Console` has the URL and HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpFormat {
    Full,
    Console,
}

impl HelpFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Console => "Console",
        }
    }
}

#[derive(Clone)]
pub struct LcuHttp {
    client: reqwest::Client,
    base: String,
    auth: String,
}

impl std::fmt::Debug for LcuHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LcuHttp")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl LcuHttp {
    pub fn new(creds: &Credentials) -> Result<Self, TransportError> {
        let tls = (*client_config()?).clone();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .http1_only()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            client,
            base: format!("https://127.0.0.1:{}", creds.port),
            auth: creds.auth_header(),
        })
    }

    /// GET a resource. Returns `None` on 404 (e.g. the champ select session when
    /// not in champ select).
    pub async fn get_json(&self, path: &str) -> Result<Option<Value>, TransportError> {
        let resp = self
            .client
            .get(format!("{}{}", self.base, path))
            .header("Authorization", &self.auth)
            .header("Accept", "application/json")
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(resp.error_for_status()?.json().await?))
    }

    /// Client build version, e.g. `16.21.829.8620`.
    pub async fn client_version(&self) -> Result<String, TransportError> {
        let v = self
            .get_json("/system/v1/builds")
            .await?
            .ok_or_else(|| TransportError::Unexpected("/system/v1/builds returned 404".into()))?;
        v.get("version")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| TransportError::Unexpected("no version in /system/v1/builds".into()))
    }

    /// `POST /Help`: index of all functions, types and events the client knows.
    pub async fn help_index(&self) -> Result<Value, TransportError> {
        self.help(None).await
    }

    /// `POST /help?target=<name>&format=<format>`: description of one item.
    pub async fn help_target(
        &self,
        target: &str,
        format: HelpFormat,
    ) -> Result<Value, TransportError> {
        self.help(Some((target, format))).await
    }

    async fn help(&self, target: Option<(&str, HelpFormat)>) -> Result<Value, TransportError> {
        let req = match target {
            None => self.client.post(format!("{}/Help", self.base)),
            Some((t, format)) => self
                .client
                .post(format!("{}/help", self.base))
                .query(&[("target", t), ("format", format.as_str())]),
        };
        let resp = req
            .header("Authorization", &self.auth)
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }
}
