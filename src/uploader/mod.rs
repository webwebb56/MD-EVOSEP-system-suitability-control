//! Cloud uploader with retry logic.
//!
//! Uploads QC payloads to the MD cloud with exponential backoff retry.

use anyhow::Result;
use backoff::ExponentialBackoff;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::config::CloudConfig;
use crate::error::UploadError;
use crate::spool::Spool;
use crate::types::QcPayload;

/// Uploader for sending payloads to the cloud.
#[derive(Clone)]
pub struct Uploader {
    config: CloudConfig,
    client: reqwest::Client,
    spool: Spool,
}

impl Uploader {
    /// Create a new uploader.
    pub fn new(config: &CloudConfig, spool: Spool) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));

        // Configure proxy if set
        if let Some(ref proxy_url) = config.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)?;
            client_builder = client_builder.proxy(proxy);
        }

        // TODO: Configure mTLS with certificate from Windows cert store
        // This would use native-tls or rustls with Windows cert store integration

        let client = client_builder.build()?;

        Ok(Self {
            config: config.clone(),
            client,
            spool,
        })
    }

    /// Run the upload loop.
    pub async fn run(&self) {
        // Recover any uploads that were in progress when we last stopped
        if let Err(e) = self.spool.recover() {
            error!(error = %e, "Failed to recover spool");
        }

        let poll_interval = Duration::from_secs(5);

        loop {
            // Get pending payloads
            let pending = match self.spool.get_pending() {
                Ok(p) => p,
                Err(e) => {
                    error!(error = %e, "Failed to get pending payloads");
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }
            };

            if pending.is_empty() {
                tokio::time::sleep(poll_interval).await;
                continue;
            }

            debug!(count = pending.len(), "Processing pending payloads");

            for path in pending {
                if let Err(e) = self.upload_with_retry(&path).await {
                    error!(
                        path = %path.display(),
                        error = %e,
                        "Upload failed after retries"
                    );
                }
            }
        }
    }

    /// Upload a single payload with retry.
    async fn upload_with_retry(&self, path: &PathBuf) -> Result<(), UploadError> {
        // Move to uploading
        let uploading_path = self.spool.mark_uploading(path)
            .map_err(|e| UploadError::Server {
                status: 0,
                message: e.to_string(),
            })?;

        // Read payload
        let content = std::fs::read_to_string(&uploading_path)
            .map_err(|e| UploadError::Server {
                status: 0,
                message: e.to_string(),
            })?;

        let payload: QcPayload = serde_json::from_str(&content)
            .map_err(|e| UploadError::Server {
                status: 0,
                message: e.to_string(),
            })?;

        // Configure backoff
        let backoff = ExponentialBackoff {
            initial_interval: Duration::from_secs(30),
            max_interval: Duration::from_secs(3600), // 1 hour max
            max_elapsed_time: Some(Duration::from_secs(86400)), // 24 hours total
            ..Default::default()
        };

        // Attempt upload with retry
        let result = backoff::future::retry(backoff, || async {
            match self.upload_payload(&payload).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    warn!(
                        run_id = %payload.run.run_id,
                        error = %e,
                        "Upload attempt failed, will retry"
                    );
                    Err(backoff::Error::transient(e))
                }
            }
        })
        .await;

        match result {
            Ok(()) => {
                self.spool.mark_completed(&uploading_path)
                    .map_err(|e| UploadError::Server {
                        status: 0,
                        message: e.to_string(),
                    })?;
                Ok(())
            }
            Err(e) => {
                // Move to failed after all retries exhausted
                let _ = self.spool.mark_failed(&uploading_path);
                Err(UploadError::RetryExhausted(5))
            }
        }
    }

    /// Upload a single payload (single attempt).
    async fn upload_payload(&self, payload: &QcPayload) -> Result<(), UploadError> {
        let url = format!("{}ingest", self.config.endpoint);

        info!(
            run_id = %payload.run.run_id,
            url = %url,
            "Uploading payload"
        );

        let response = self
            .client
            .post(&url)
            .json(payload)
            .send()
            .await?;

        let status = response.status();

        if status.is_success() {
            info!(
                run_id = %payload.run.run_id,
                "Upload successful"
            );
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(UploadError::Server {
                status: status.as_u16(),
                message: body,
            })
        }
    }
}
