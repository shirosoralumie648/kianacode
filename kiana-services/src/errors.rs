use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("API error: {0}")]
    Api(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Rate limit error: {0}")]
    RateLimit(String),

    #[error("Prompt too long: actual={actual_tokens:?}, limit={limit_tokens:?}")]
    PromptTooLong {
        actual_tokens: Option<u32>,
        limit_tokens: Option<u32>,
        raw_message: String,
    },

    #[error("Credit balance too low")]
    CreditBalanceLow,

    #[error("Request timeout")]
    Timeout,

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Request too large")]
    RequestTooLarge,

    #[error("Invalid model: {0}")]
    InvalidModel(String),

    #[error("PDF error: {0}")]
    Pdf(String),

    #[error("Image too large: {0}")]
    ImageTooLarge(String),

    #[error("OAuth token revoked")]
    OAuthTokenRevoked,

    #[error("Organization not allowed")]
    OrgNotAllowed,

    #[error("Organization disabled")]
    OrgDisabled,

    #[error("HTTP error: status={status}, body={body}")]
    Http { status: u16, body: String },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type ServiceResult<T> = Result<T, ServiceError>;

impl ServiceError {
    pub fn classify(&self) -> &'static str {
        match self {
            ServiceError::Timeout => "api_timeout",
            ServiceError::RateLimit(_) => "rate_limit",
            ServiceError::PromptTooLong { .. } => "prompt_too_long",
            ServiceError::CreditBalanceLow => "credit_balance_low",
            ServiceError::Auth(_) => "auth_error",
            ServiceError::OAuthTokenRevoked => "token_revoked",
            ServiceError::OrgNotAllowed => "oauth_org_not_allowed",
            ServiceError::OrgDisabled => "org_disabled",
            ServiceError::InvalidModel(_) => "invalid_model",
            ServiceError::Pdf(_) => "pdf_error",
            ServiceError::ImageTooLarge(_) => "image_too_large",
            ServiceError::RequestTooLarge => "request_too_large",
            ServiceError::Connection(_) => "connection_error",
            ServiceError::Http { status, .. } if *status >= 500 => "server_error",
            ServiceError::Http { status, .. } if *status >= 400 => "client_error",
            _ => "unknown",
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            ServiceError::Timeout | ServiceError::Connection(_) | ServiceError::RateLimit(_) => {
                true
            }
            ServiceError::Http { status, .. } if *status == 529 || *status == 503 => true,
            _ => false,
        }
    }
}
