//! HTTP client policy for usage collection in the app.

use reqwest::Client;

use crate::error::{AppError, Result};
use crate::vendor::HTTP_CLIENT_TIMEOUT;

pub(crate) fn http_client() -> Result<Client> {
    Client::builder()
        .timeout(HTTP_CLIENT_TIMEOUT)
        .redirect(crate::vendor::same_origin_redirect_policy())
        .build()
        .map_err(|e| AppError::Other(format!("http client init: {e}")))
}
