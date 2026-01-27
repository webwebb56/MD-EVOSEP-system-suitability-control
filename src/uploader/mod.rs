//! Cloud uploader with retry logic and mTLS.
//!
//! Uploads QC payloads to the MD cloud with exponential backoff retry.
//! Uses mutual TLS (mTLS) with client certificates from Windows cert store.

use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::config::CloudConfig;
use crate::error::UploadError;
use crate::spool::Spool;
use crate::types::QcPayload;

/// Retry configuration per spec:
/// Attempt 1: immediate
/// Attempt 2: 30s ± 10s
/// Attempt 3: 2m ± 30s
/// Attempt 4: 10m ± 2m
/// Attempt 5: 1h ± 10m
const RETRY_DELAYS_SECS: [(u64, u64); 5] = [
    (0, 0),       // Attempt 1: immediate
    (20, 40),     // Attempt 2: 30s ± 10s
    (90, 150),    // Attempt 3: 2m ± 30s
    (480, 720),   // Attempt 4: 10m ± 2m
    (3000, 4200), // Attempt 5: 1h ± 10m
];

/// Uploader for sending payloads to the cloud.
#[derive(Clone)]
pub struct Uploader {
    config: CloudConfig,
    client: reqwest::Client,
    spool: Spool,
    /// Cached API token for Bearer auth
    api_token: Option<String>,
}

impl Uploader {
    /// Create a new uploader with mTLS or Bearer token support.
    pub fn new(config: &CloudConfig, spool: Spool) -> Result<Self> {
        let client = Self::build_client(config)?;
        let api_token = config.api_token.clone();

        if api_token.is_some() {
            info!("Bearer token authentication configured");
        }

        Ok(Self {
            config: config.clone(),
            client,
            spool,
            api_token,
        })
    }

    /// Build the HTTP client with mTLS if certificate is configured.
    fn build_client(config: &CloudConfig) -> Result<reqwest::Client> {
        let mut client_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));

        // Configure proxy if set
        if let Some(ref proxy_url) = config.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)?;
            client_builder = client_builder.proxy(proxy);
        }

        // Configure mTLS if certificate thumbprint is provided
        if let Some(ref thumbprint) = config.certificate_thumbprint {
            let identity = Self::load_identity_from_cert_store(thumbprint)?;
            client_builder = client_builder.identity(identity);
            info!(thumbprint = %thumbprint, "mTLS client certificate configured");
        } else if config.api_token.is_none() {
            warn!("No authentication configured (no certificate thumbprint or API token)");
        }

        Ok(client_builder.build()?)
    }

    /// Load client identity from Windows certificate store.
    #[cfg(windows)]
    fn load_identity_from_cert_store(thumbprint: &str) -> Result<reqwest::Identity> {
        use std::io::Read;

        // Normalize thumbprint (remove spaces, uppercase)
        let thumbprint = thumbprint.replace(" ", "").to_uppercase();

        // Use certutil or PowerShell to export the certificate with private key
        // This is a workaround since reqwest doesn't directly support Windows cert store

        // For production, consider using native-tls with schannel backend
        // or rustls with a custom certificate resolver

        // Export cert + key to PKCS#12 format
        let temp_dir = std::env::temp_dir();
        let pfx_path = temp_dir.join(format!("mdqc_cert_{}.pfx", &thumbprint[..8]));

        // Use PowerShell to export (requires the cert to be exportable)
        let output = std::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    r#"$cert = Get-ChildItem -Path Cert:\LocalMachine\My | Where-Object {{ $_.Thumbprint -eq '{}' }};
                    if ($cert) {{
                        $pwd = ConvertTo-SecureString -String 'mdqc_temp_pwd' -Force -AsPlainText;
                        Export-PfxCertificate -Cert $cert -FilePath '{}' -Password $pwd | Out-Null;
                        Write-Output 'OK'
                    }} else {{
                        Write-Error 'Certificate not found'
                    }}"#,
                    thumbprint,
                    pfx_path.display()
                ),
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to export certificate: {}", stderr);
        }

        // Read the PFX file
        let mut pfx_data = Vec::new();
        std::fs::File::open(&pfx_path)?.read_to_end(&mut pfx_data)?;

        // Clean up temp file
        let _ = std::fs::remove_file(&pfx_path);

        // Create identity from PFX
        let identity = reqwest::Identity::from_pkcs12_der(&pfx_data, "mdqc_temp_pwd")?;

        Ok(identity)
    }

    /// Load client identity - stub for non-Windows platforms.
    #[cfg(not(windows))]
    fn load_identity_from_cert_store(thumbprint: &str) -> Result<reqwest::Identity> {
        // On non-Windows, look for a PEM file in the config directory
        let cert_path = crate::config::paths::data_dir()
            .join("certs")
            .join(format!("{}.pem", thumbprint));

        if cert_path.exists() {
            let pem_data = std::fs::read(&cert_path)?;
            Ok(reqwest::Identity::from_pem(&pem_data)?)
        } else {
            anyhow::bail!(
                "Certificate not found. On non-Windows, place PEM file at: {}",
                cert_path.display()
            )
        }
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

    /// Upload a single payload with exactly 5 retry attempts per spec.
    async fn upload_with_retry(&self, path: &PathBuf) -> Result<(), UploadError> {
        // Move to uploading
        let uploading_path = self
            .spool
            .mark_uploading(path)
            .map_err(|e| UploadError::Server {
                status: 0,
                message: e.to_string(),
            })?;

        // Read payload
        let content =
            std::fs::read_to_string(&uploading_path).map_err(|e| UploadError::Server {
                status: 0,
                message: e.to_string(),
            })?;

        let payload: QcPayload =
            serde_json::from_str(&content).map_err(|e| UploadError::Server {
                status: 0,
                message: e.to_string(),
            })?;

        // Attempt upload with exactly 5 retries per spec
        let mut _last_error = None;

        for (attempt, (min_delay, max_delay)) in RETRY_DELAYS_SECS.iter().enumerate() {
            // Apply delay (with jitter) for attempts after the first
            if attempt > 0 {
                let delay = if max_delay > min_delay {
                    use rand::Rng;
                    let jitter = rand::thread_rng().gen_range(*min_delay..=*max_delay);
                    Duration::from_secs(jitter)
                } else {
                    Duration::from_secs(*min_delay)
                };

                info!(
                    run_id = %payload.run.run_id,
                    attempt = attempt + 1,
                    delay_secs = delay.as_secs(),
                    "Retrying upload after delay"
                );
                tokio::time::sleep(delay).await;
            }

            match self.upload_payload(&payload).await {
                Ok(()) => {
                    self.spool.mark_completed(&uploading_path).map_err(|e| {
                        UploadError::Server {
                            status: 0,
                            message: e.to_string(),
                        }
                    })?;
                    return Ok(());
                }
                Err(e) => {
                    warn!(
                        run_id = %payload.run.run_id,
                        attempt = attempt + 1,
                        error = %e,
                        "Upload attempt failed"
                    );
                    _last_error = Some(e);
                }
            }
        }

        // All 5 attempts exhausted - move to failed
        let _ = self.spool.mark_failed(&uploading_path);
        Err(UploadError::RetryExhausted(5))
    }

    /// Upload a single payload (single attempt).
    async fn upload_payload(&self, payload: &QcPayload) -> Result<(), UploadError> {
        let url = format!("{}ingest", self.config.endpoint);

        info!(
            run_id = %payload.run.run_id,
            correlation_id = %payload.correlation_id,
            url = %url,
            "Uploading payload"
        );

        // Build request with optional Bearer token
        let mut request = self.client.post(&url).json(payload);

        if let Some(ref token) = self.api_token {
            request = request.header("Authorization", format!("Bearer {}", token));
            debug!("Added Bearer token authentication header");
        }

        let response = request.send().await?;

        let status = response.status();

        if status.is_success() {
            info!(
                run_id = %payload.run.run_id,
                "Upload successful"
            );
            Ok(())
        } else if status.as_u16() == 401 || status.as_u16() == 403 {
            let body = response.text().await.unwrap_or_default();
            Err(UploadError::Authentication(format!(
                "status {}: {}",
                status.as_u16(),
                body
            )))
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(UploadError::Server {
                status: status.as_u16(),
                message: body,
            })
        }
    }
}
