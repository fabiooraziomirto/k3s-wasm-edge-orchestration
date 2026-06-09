// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use axum::{
    extract::{Path, State},
    http::{HeaderValue, Method, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, sync::RwLock};
use tracing::{error, info, warn};

mod certificate_authority;
mod secret_store;
mod monitoring;
mod logging;

use certificate_authority::CertificateAuthority;
use secret_store::SecretStore;
use monitoring::MonitoringService;
use logging::LoggingService;

#[derive(Parser)]
#[command(name = "wasmbed-infrastructure")]
#[command(about = "Wasmbed Infrastructure - Certificate Authority, Secret Store, Monitoring & Logging")]
struct Args {
    #[arg(long, env = "WASMBED_INFRASTRUCTURE_PORT", default_value = "30460")]
    port: u16,
    #[arg(long, env = "WASMBED_INFRASTRUCTURE_MONITORING_PORT", default_value = "9090")]
    monitoring_port: u16,
    #[arg(long, env = "WASMBED_INFRASTRUCTURE_LOGGING_PORT", default_value = "8080")]
    logging_port: u16,
}

/// Infrastructure configuration
#[derive(Debug, Clone)]
pub struct InfrastructureConfig {
    pub ca_cert_path: String,
    pub ca_key_path: String,
    pub secret_store_path: String,
    pub api_port: u16,
    pub monitoring_port: u16,
    pub logging_port: u16,
}

impl Default for InfrastructureConfig {
    fn default() -> Self {
        Self {
            ca_cert_path: "/etc/wasmbed/ca.crt".to_string(),
            ca_key_path: "/etc/wasmbed/ca.key".to_string(),
            secret_store_path: "/var/lib/wasmbed/secrets".to_string(),
            api_port: 30460,
            monitoring_port: 9090,
            logging_port: 8080,
        }
    }
}

/// Infrastructure state
#[derive(Debug, Clone)]
pub struct InfrastructureState {
    pub config: InfrastructureConfig,
    pub ca: Arc<CertificateAuthority>,
    pub secret_store: Arc<SecretStore>,
    pub monitoring: Arc<MonitoringService>,
    pub logging: Arc<LoggingService>,
    pub certificates: Arc<RwLock<HashMap<String, CertificateInfo>>>,
    pub secrets: Arc<RwLock<HashMap<String, SecretInfo>>>,
}

/// Certificate information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateInfo {
    pub cert_id: String,
    pub subject: String,
    pub issuer: String,
    pub not_before: SystemTime,
    pub not_after: SystemTime,
    pub public_key: String,
    pub status: CertificateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CertificateStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "revoked")]
    Revoked,
    #[serde(rename = "expired")]
    Expired,
}

/// Secret information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretInfo {
    pub secret_id: String,
    pub name: String,
    pub namespace: String,
    pub created_at: SystemTime,
    pub last_accessed: Option<SystemTime>,
    pub access_count: u64,
}

/// Infrastructure API handlers
pub struct InfrastructureApi;

impl InfrastructureApi {
    /// Health check endpoint
    pub async fn health() -> Result<Json<serde_json::Value>, StatusCode> {
        Ok(Json(serde_json::json!({
            "status": "healthy",
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        })))
    }

    /// Get logs endpoint
    pub async fn get_logs(State(state): State<Arc<InfrastructureState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        let logs = state.logging.get_logs(Some(50)).await;
        Ok(Json(serde_json::json!({
            "logs": logs,
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        })))
    }

    /// Get infrastructure status
    pub async fn get_status(State(state): State<Arc<InfrastructureState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        let certificates = state.certificates.read().await;
        let secrets = state.secrets.read().await;

        let status = serde_json::json!({
            "ca_status": "active",
            "secret_store_status": "active",
            "monitoring_status": "active",
            "logging_status": "active",
            "total_certificates": certificates.len(),
            "active_certificates": certificates.values()
                .filter(|c| c.status == CertificateStatus::Active)
                .count(),
            "total_secrets": secrets.len(),
            "uptime": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        });

        Ok(Json(status))
    }

    /// Get metrics endpoint
    pub async fn get_metrics(State(state): State<Arc<InfrastructureState>>) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
        info!("Getting metrics from monitoring service");
        let mut metrics = Vec::new();
        
        // Get system metrics from monitoring service
        let system_metrics = state.monitoring.get_system_metrics().await;
        info!("Retrieved {} system metrics", system_metrics.len());
        
        for (name, value) in system_metrics {
            metrics.push(serde_json::json!({
                "name": name,
                "value": value,
                "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                "labels": {}
            }));
        }
        
        info!("Returning {} metrics", metrics.len());
        Ok(Json(metrics))
    }

    /// Get all certificates
    pub async fn get_certificates(State(state): State<Arc<InfrastructureState>>) -> Result<Json<Vec<CertificateInfo>>, StatusCode> {
        let certificates = state.certificates.read().await;
        let cert_list: Vec<CertificateInfo> = certificates.values().cloned().collect();
        Ok(Json(cert_list))
    }

    /// Get certificate by ID
    pub async fn get_certificate(
        Path(cert_id): Path<String>,
        State(state): State<Arc<InfrastructureState>>,
    ) -> Result<Json<CertificateInfo>, StatusCode> {
        let certificates = state.certificates.read().await;
        match certificates.get(&cert_id) {
            Some(cert) => Ok(Json(cert.clone())),
            None => Err(StatusCode::NOT_FOUND),
        }
    }

    /// Generate new certificate
    pub async fn generate_certificate(
        State(state): State<Arc<InfrastructureState>>,
        Json(request): Json<CertificateRequest>,
    ) -> Result<Json<CertificateResponse>, StatusCode> {
        info!("Generating certificate for subject: {}", request.subject);

        let cert_id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now();
        let not_after = now + Duration::from_secs(365 * 24 * 60 * 60); // 1 year

        let certificate = CertificateInfo {
            cert_id: cert_id.clone(),
            subject: request.subject.clone(),
            issuer: "Wasmbed CA".to_string(),
            not_before: now,
            not_after,
            public_key: "generated-public-key".to_string(),
            status: CertificateStatus::Active,
        };

        // Store certificate
        {
            let mut certificates = state.certificates.write().await;
            certificates.insert(cert_id.clone(), certificate.clone());
        }

        info!("Certificate generated: {}", cert_id);

        Ok(Json(CertificateResponse {
            cert_id,
            certificate: certificate,
        }))
    }

    /// Get all secrets
    pub async fn get_secrets(State(state): State<Arc<InfrastructureState>>) -> Result<Json<Vec<SecretInfo>>, StatusCode> {
        let secrets = state.secrets.read().await;
        let secret_list: Vec<SecretInfo> = secrets.values().cloned().collect();
        Ok(Json(secret_list))
    }

    /// Get secret by ID
    pub async fn get_secret(
        Path(secret_id): Path<String>,
        State(state): State<Arc<InfrastructureState>>,
    ) -> Result<Json<SecretInfo>, StatusCode> {
        let secrets = state.secrets.read().await;
        match secrets.get(&secret_id) {
            Some(secret) => Ok(Json(secret.clone())),
            None => Err(StatusCode::NOT_FOUND),
        }
    }

    /// Create new secret
    pub async fn create_secret(
        State(state): State<Arc<InfrastructureState>>,
        Json(request): Json<SecretRequest>,
    ) -> Result<Json<SecretResponse>, StatusCode> {
        info!("Creating secret: {}", request.name);

        let secret_id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now();

        let secret = SecretInfo {
            secret_id: secret_id.clone(),
            name: request.name.clone(),
            namespace: request.namespace.clone(),
            created_at: now,
            last_accessed: None,
            access_count: 0,
        };

        // Store secret
        {
            let mut secrets = state.secrets.write().await;
            secrets.insert(secret_id.clone(), secret.clone());
        }

        info!("Secret created: {}", secret_id);

        Ok(Json(SecretResponse {
            secret_id,
            secret: secret,
        }))
    }
}

#[derive(Debug, Serialize)]
pub struct InfrastructureStatus {
    pub ca_status: String,
    pub secret_store_status: String,
    pub monitoring_status: String,
    pub logging_status: String,
    pub total_certificates: u32,
    pub active_certificates: u32,
    pub total_secrets: u32,
    pub uptime: u64,
}

#[derive(Debug, Deserialize)]
pub struct CertificateRequest {
    pub subject: String,
    pub validity_days: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CertificateResponse {
    pub cert_id: String,
    pub certificate: CertificateInfo,
}

#[derive(Debug, Deserialize)]
pub struct SecretRequest {
    pub name: String,
    pub namespace: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct SecretResponse {
    pub secret_id: String,
    pub secret: SecretInfo,
}

/// Infrastructure implementation
#[derive(Debug, Clone)]
pub struct Infrastructure {
    config: InfrastructureConfig,
    state: Arc<InfrastructureState>,
}

impl Infrastructure {
    pub fn new(config: InfrastructureConfig) -> anyhow::Result<Self> {
        let ca = Arc::new(CertificateAuthority::new(&config.ca_cert_path, &config.ca_key_path)?);
        let secret_store = Arc::new(SecretStore::new(&config.secret_store_path)?);
        let monitoring = Arc::new(MonitoringService::new());
        let logging = Arc::new(LoggingService::new());

        let state = Arc::new(InfrastructureState {
            config: config.clone(),
            ca,
            secret_store,
            monitoring,
            logging,
            certificates: Arc::new(RwLock::new(HashMap::new())),
            secrets: Arc::new(RwLock::new(HashMap::new())),
        });

        Ok(Self { config, state })
    }

    pub async fn run(self) -> anyhow::Result<()> {
        info!("Starting Wasmbed Infrastructure...");

        let monitoring = self.state.monitoring.clone();
        let logging = self.state.logging.clone();
        let api = self.clone();

        // Start monitoring service
        let monitoring_handle = tokio::spawn(async move {
            monitoring.run().await;
        });

        // Start logging service
        let logging_handle = tokio::spawn(async move {
            logging.run().await;
        });

        // Start REST API server
        let api_handle = tokio::spawn(async move {
            if let Err(e) = api.run_rest_api().await {
                error!("REST API server error: {}", e);
            }
        });

        info!("Infrastructure started successfully");
        info!("Monitoring: http://localhost:{}", self.config.monitoring_port);
        info!("Logging: http://localhost:{}", self.config.logging_port);

        // Wait for all tasks
        tokio::try_join!(monitoring_handle, logging_handle, api_handle)?;

        Ok(())
    }

    async fn run_rest_api(self) -> anyhow::Result<()> {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers(Any);

        let app = Router::new()
            .route("/health", get(InfrastructureApi::health))
            .route("/logs", get(InfrastructureApi::get_logs))
            .route("/api/v1/status", get(InfrastructureApi::get_status))
            .route("/api/v1/metrics", get(InfrastructureApi::get_metrics))
            .route("/api/v1/certificates", get(InfrastructureApi::get_certificates))
            .route("/api/v1/certificates/:cert_id", get(InfrastructureApi::get_certificate))
            .route("/api/v1/certificates", post(InfrastructureApi::generate_certificate))
            .route("/api/v1/secrets", get(InfrastructureApi::get_secrets))
            .route("/api/v1/secrets/:secret_id", get(InfrastructureApi::get_secret))
            .route("/api/v1/secrets", post(InfrastructureApi::create_secret))
            .layer(cors)
            .with_state(self.state);

        let addr = SocketAddr::from(([0, 0, 0, 0], self.config.api_port));
        
        info!("Starting Infrastructure REST API server on {}", addr);
        
        let listener = TcpListener::bind(&addr).await?;
        info!("Infrastructure API server listening on {}", addr);
        axum::serve(listener, app).await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    
    let config = InfrastructureConfig {
        ca_cert_path: "/etc/wasmbed/ca.crt".to_string(),
        ca_key_path: "/etc/wasmbed/ca.key".to_string(),
        secret_store_path: "/var/lib/wasmbed/secrets".to_string(),
        api_port: args.port,
        monitoring_port: args.monitoring_port,
        logging_port: args.logging_port,
    };
    
    let infrastructure = Infrastructure::new(config)?;
    
    infrastructure.run().await?;
    
    Ok(())
}
