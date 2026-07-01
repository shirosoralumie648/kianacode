use crate::errors::ServiceError;
use std::fmt;

/// Classification of API error types, matching TypeScript classifyAPIError().
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ApiErrorKind {
    Aborted,
    ApiTimeout,
    Repeated529,
    CapacityOffSwitch,
    RateLimit,
    ServerOverload,
    PromptTooLong,
    PdfTooLarge,
    PdfPasswordProtected,
    ImageTooLarge,
    ToolUseMismatch,
    UnexpectedToolResult,
    DuplicateToolUseId,
    InvalidModel,
    CreditBalanceLow,
    InvalidApiKey,
    TokenRevoked,
    OauthOrgNotAllowed,
    AuthError,
    BedrockModelAccess,
    ServerError,
    ClientError,
    SslCertError,
    ConnectionError,
    Unknown,
}

impl fmt::Display for ApiErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ApiErrorKind::Aborted => "aborted",
            ApiErrorKind::ApiTimeout => "api_timeout",
            ApiErrorKind::Repeated529 => "repeated_529",
            ApiErrorKind::CapacityOffSwitch => "capacity_off_switch",
            ApiErrorKind::RateLimit => "rate_limit",
            ApiErrorKind::ServerOverload => "server_overload",
            ApiErrorKind::PromptTooLong => "prompt_too_long",
            ApiErrorKind::PdfTooLarge => "pdf_too_large",
            ApiErrorKind::PdfPasswordProtected => "pdf_password_protected",
            ApiErrorKind::ImageTooLarge => "image_too_large",
            ApiErrorKind::ToolUseMismatch => "tool_use_mismatch",
            ApiErrorKind::UnexpectedToolResult => "unexpected_tool_result",
            ApiErrorKind::DuplicateToolUseId => "duplicate_tool_use_id",
            ApiErrorKind::InvalidModel => "invalid_model",
            ApiErrorKind::CreditBalanceLow => "credit_balance_low",
            ApiErrorKind::InvalidApiKey => "invalid_api_key",
            ApiErrorKind::TokenRevoked => "token_revoked",
            ApiErrorKind::OauthOrgNotAllowed => "oauth_org_not_allowed",
            ApiErrorKind::AuthError => "auth_error",
            ApiErrorKind::BedrockModelAccess => "bedrock_model_access",
            ApiErrorKind::ServerError => "server_error",
            ApiErrorKind::ClientError => "client_error",
            ApiErrorKind::SslCertError => "ssl_cert_error",
            ApiErrorKind::ConnectionError => "connection_error",
            ApiErrorKind::Unknown => "unknown",
        };
        write!(f, "{}", s)
    }
}

/// Structured API error with HTTP status and classification.
#[derive(Debug)]
pub struct ApiError {
    pub kind: ApiErrorKind,
    pub status: Option<u16>,
    pub message: String,
    pub headers: std::collections::HashMap<String, String>,
}

impl ApiError {
    pub fn new(kind: ApiErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            status: None,
            message: message.into(),
            headers: std::collections::HashMap::new(),
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    pub fn classify(status: Option<u16>, message: &str) -> ApiErrorKind {
        if message == "Request was aborted." {
            return ApiErrorKind::Aborted;
        }
        if message.to_lowercase().contains("timeout") {
            return ApiErrorKind::ApiTimeout;
        }
        if message.contains("prompt is too long")
            || message.to_lowercase().contains("prompt is too long")
        {
            return ApiErrorKind::PromptTooLong;
        }
        if message.to_lowercase().contains("credit balance") {
            return ApiErrorKind::CreditBalanceLow;
        }
        if message.contains("x-api-key") {
            return ApiErrorKind::InvalidApiKey;
        }
        if message.contains("OAuth token has been revoked") {
            return ApiErrorKind::TokenRevoked;
        }
        if message.contains("OAuth authentication is currently not allowed") {
            return ApiErrorKind::OauthOrgNotAllowed;
        }

        match status {
            Some(529) | Some(503) => ApiErrorKind::ServerOverload,
            Some(429) => ApiErrorKind::RateLimit,
            Some(401) | Some(403) => ApiErrorKind::AuthError,
            Some(408) | Some(409) => ApiErrorKind::ApiTimeout,
            Some(s) if s >= 500 => ApiErrorKind::ServerError,
            Some(s) if s >= 400 => ApiErrorKind::ClientError,
            _ => ApiErrorKind::Unknown,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(status) => write!(f, "{} (HTTP {}): {}", self.kind, status, self.message),
            None => write!(f, "{}: {}", self.kind, self.message),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<ApiError> for ServiceError {
    fn from(err: ApiError) -> Self {
        match err.kind {
            ApiErrorKind::RateLimit => ServiceError::RateLimit(err.message),
            ApiErrorKind::ApiTimeout => ServiceError::Timeout,
            ApiErrorKind::AuthError | ApiErrorKind::InvalidApiKey => {
                ServiceError::Auth(err.message)
            }
            ApiErrorKind::TokenRevoked => ServiceError::OAuthTokenRevoked,
            ApiErrorKind::OauthOrgNotAllowed => ServiceError::OrgNotAllowed,
            ApiErrorKind::CreditBalanceLow => ServiceError::CreditBalanceLow,
            ApiErrorKind::PromptTooLong => {
                let (actual, limit) = crate::api::parse_prompt_too_long_tokens(&err.message);
                ServiceError::PromptTooLong {
                    actual_tokens: actual,
                    limit_tokens: limit,
                    raw_message: err.message,
                }
            }
            ApiErrorKind::ConnectionError | ApiErrorKind::SslCertError => {
                ServiceError::Connection(err.message)
            }
            _ => ServiceError::Http {
                status: err.status.unwrap_or(0),
                body: err.message,
            },
        }
    }
}
