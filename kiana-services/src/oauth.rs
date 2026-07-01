use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
}

pub struct OAuthClient {
    config: OAuthConfig,
    client: reqwest::Client,
}

impl OAuthClient {
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub async fn refresh_token(
        &self,
        refresh_token: &str,
    ) -> crate::errors::ServiceResult<OAuthTokens> {
        let response = self
            .client
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.config.client_id),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(crate::errors::ServiceError::Auth(format!(
                "OAuth refresh failed: {}",
                response.status()
            )));
        }

        let tokens: OAuthTokens = response.json().await?;
        Ok(tokens)
    }
}
