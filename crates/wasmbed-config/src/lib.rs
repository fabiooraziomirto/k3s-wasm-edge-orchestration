//! Wasmbed Configuration Management
//! 
//! This crate provides configuration management for the Wasmbed platform,
//! allowing dynamic configuration of gateways, devices, and applications.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use anyhow::{Result, Context};

/// Platform metadata configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformConfig {
    pub name: String,
    pub version: String,
    pub description: String,
}

/// Gateway configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub name: String,
    pub endpoint: String,
    pub tls_port: u16,
    pub http_port: u16,
    pub max_devices: u32,
    pub region: String,
    pub enabled: bool,
}

/// Gateway defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayDefaults {
    pub max_devices: u32,
    pub heartbeat_interval: u32,
    pub connection_timeout: u32,
    pub enrollment_timeout: u32,
    pub tls_port: u16,
    pub http_port: u16,
    pub region: String,
}

/// Gateway configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewaySection {
    pub defaults: GatewayDefaults,
    pub instances: Vec<GatewayConfig>,
}

/// Device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub name: String,
    pub device_type: String,
    pub architecture: String,
    pub gateway: String,
    pub enabled: bool,
}

/// Device defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDefaults {
    pub architecture: String,
    pub capabilities: Vec<String>,
    pub heartbeat_interval: u32,
    pub connection_retry_interval: u32,
    pub max_retries: u32,
}

/// Device type configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceType {
    pub architecture: String,
    pub memory: String,
    pub storage: String,
    pub capabilities: Vec<String>,
}

/// Device configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSection {
    pub defaults: DeviceDefaults,
    pub types: HashMap<String, DeviceType>,
    pub instances: Vec<DeviceConfig>,
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationConfig {
    pub name: String,
    pub description: String,
    pub wasm_bytes: String,
    pub target_devices: Vec<String>,
    pub config: HashMap<String, serde_json::Value>,
    pub enabled: bool,
}

/// Application defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationDefaults {
    pub max_size: String,
    pub timeout: u32,
    pub retries: u32,
    pub validation: ValidationConfig,
}

/// Validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    pub enabled: bool,
    pub strict: bool,
}

/// Application template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationTemplate {
    pub name: String,
    pub description: String,
    pub wasm_bytes: String,
    pub target_devices: Vec<String>,
    pub config: HashMap<String, serde_json::Value>,
}

/// Application configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationSection {
    pub defaults: ApplicationDefaults,
    pub templates: HashMap<String, ApplicationTemplate>,
    pub instances: Vec<ApplicationConfig>,
}

/// Infrastructure configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructureConfig {
    pub ca: CertificateAuthorityConfig,
    pub secret_store: SecretStoreConfig,
    pub monitoring: MonitoringConfig,
    pub database: DatabaseConfig,
}

/// Certificate Authority configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateAuthorityConfig {
    pub enabled: bool,
    pub cert_path: String,
    pub key_path: String,
    pub validity_days: u32,
}

/// Secret Store configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretStoreConfig {
    pub enabled: bool,
    pub store_type: String,
    pub namespace: String,
}

/// Monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub enabled: bool,
    pub metrics: MetricsConfig,
    pub logging: LoggingConfig,
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub port: u16,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub enabled: bool,
    pub level: String,
    pub format: String,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub enabled: bool,
    pub db_type: String,
    pub endpoints: Vec<String>,
    pub namespace: String,
}

/// Kubernetes configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubernetesConfig {
    pub namespace: String,
    pub crds: Vec<String>,
    pub rbac: Vec<String>,
}

/// Dashboard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub enabled: bool,
    pub port: u16,
    pub theme: String,
    pub real_time_updates: bool,
    pub polling_interval: u32,
    pub websocket: WebSocketConfig,
}

/// WebSocket configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    pub enabled: bool,
    pub port: u16,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub tls: TlsConfig,
    pub authentication: AuthenticationConfig,
    pub authorization: AuthorizationConfig,
}

/// TLS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub enabled: bool,
    pub min_version: String,
    pub cipher_suites: Vec<String>,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationConfig {
    pub enabled: bool,
    pub auth_type: String,
    pub required: bool,
}

/// Authorization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationConfig {
    pub enabled: bool,
    pub rbac: bool,
    pub policies: Vec<String>,
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub connection_pool: ConnectionPoolConfig,
    pub cache: CacheConfig,
    pub rate_limit: RateLimitConfig,
}

/// Connection pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    pub max_connections: u32,
    pub idle_timeout: u32,
    pub max_lifetime: u32,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub enabled: bool,
    pub ttl: u32,
    pub max_size: String,
}

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

/// Development configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopmentConfig {
    pub debug: bool,
    pub verbose: bool,
    pub mock_data: bool,
    pub hot_reload: bool,
    pub profiling: bool,
}

/// Main Wasmbed configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmbedConfig {
    pub platform: PlatformConfig,
    pub gateways: GatewaySection,
    pub devices: DeviceSection,
    pub applications: ApplicationSection,
    pub infrastructure: InfrastructureConfig,
    pub kubernetes: KubernetesConfig,
    pub dashboard: DashboardConfig,
    pub security: SecurityConfig,
    pub performance: PerformanceConfig,
    pub development: DevelopmentConfig,
}

impl WasmbedConfig {
    /// Load configuration from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;
        
        let config: WasmbedConfig = serde_yaml::from_str(&content)
            .with_context(|| "Failed to parse YAML configuration")?;
        
        Ok(config)
    }
    
    /// Save configuration to file
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_yaml::to_string(self)
            .with_context(|| "Failed to serialize configuration")?;
        
        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file: {:?}", path.as_ref()))?;
        
        Ok(())
    }
    
    /// Get gateway configuration by name
    pub fn get_gateway(&self, name: &str) -> Option<&GatewayConfig> {
        self.gateways.instances.iter().find(|g| g.name == name)
    }
    
    /// Get device configuration by name
    pub fn get_device(&self, name: &str) -> Option<&DeviceConfig> {
        self.devices.instances.iter().find(|d| d.name == name)
    }
    
    /// Get application configuration by name
    pub fn get_application(&self, name: &str) -> Option<&ApplicationConfig> {
        self.applications.instances.iter().find(|a| a.name == name)
    }
    
    /// Get enabled gateways
    pub fn get_enabled_gateways(&self) -> Vec<&GatewayConfig> {
        self.gateways.instances.iter().filter(|g| g.enabled).collect()
    }
    
    /// Get enabled devices
    pub fn get_enabled_devices(&self) -> Vec<&DeviceConfig> {
        self.devices.instances.iter().filter(|d| d.enabled).collect()
    }
    
    /// Get enabled applications
    pub fn get_enabled_applications(&self) -> Vec<&ApplicationConfig> {
        self.applications.instances.iter().filter(|a| a.enabled).collect()
    }
    
    /// Add new gateway
    pub fn add_gateway(&mut self, gateway: GatewayConfig) {
        self.gateways.instances.push(gateway);
    }
    
    /// Add new device
    pub fn add_device(&mut self, device: DeviceConfig) {
        self.devices.instances.push(device);
    }
    
    /// Add new application
    pub fn add_application(&mut self, application: ApplicationConfig) {
        self.applications.instances.push(application);
    }
    
    /// Remove gateway by name
    pub fn remove_gateway(&mut self, name: &str) -> bool {
        if let Some(pos) = self.gateways.instances.iter().position(|g| g.name == name) {
            self.gateways.instances.remove(pos);
            true
        } else {
            false
        }
    }
    
    /// Remove device by name
    pub fn remove_device(&mut self, name: &str) -> bool {
        if let Some(pos) = self.devices.instances.iter().position(|d| d.name == name) {
            self.devices.instances.remove(pos);
            true
        } else {
            false
        }
    }
    
    /// Remove application by name
    pub fn remove_application(&mut self, name: &str) -> bool {
        if let Some(pos) = self.applications.instances.iter().position(|a| a.name == name) {
            self.applications.instances.remove(pos);
            true
        } else {
            false
        }
    }
    
    /// Update gateway configuration
    pub fn update_gateway(&mut self, name: &str, gateway: GatewayConfig) -> bool {
        if let Some(pos) = self.gateways.instances.iter().position(|g| g.name == name) {
            self.gateways.instances[pos] = gateway;
            true
        } else {
            false
        }
    }
    
    /// Update device configuration
    pub fn update_device(&mut self, name: &str, device: DeviceConfig) -> bool {
        if let Some(pos) = self.devices.instances.iter().position(|d| d.name == name) {
            self.devices.instances[pos] = device;
            true
        } else {
            false
        }
    }
    
    /// Update application configuration
    pub fn update_application(&mut self, name: &str, application: ApplicationConfig) -> bool {
        if let Some(pos) = self.applications.instances.iter().position(|a| a.name == name) {
            self.applications.instances[pos] = application;
            true
        } else {
            false
        }
    }
}

/// Default configuration
impl Default for WasmbedConfig {
    fn default() -> Self {
        Self {
            platform: PlatformConfig {
                name: "Wasmbed".to_string(),
                version: "1.0.0".to_string(),
                description: "WebAssembly runtime platform for edge devices".to_string(),
            },
            gateways: GatewaySection {
                defaults: GatewayDefaults {
                    max_devices: 100,
                    heartbeat_interval: 30,
                    connection_timeout: 60,
                    enrollment_timeout: 300,
                    tls_port: 30452,
                    http_port: 30453,
                    region: "us-west-1".to_string(),
                },
                instances: vec![],
            },
            devices: DeviceSection {
                defaults: DeviceDefaults {
                    architecture: "xtensa".to_string(),
                    capabilities: vec!["wasm".to_string(), "tls".to_string(), "enrollment".to_string()],
                    heartbeat_interval: 30,
                    connection_retry_interval: 10,
                    max_retries: 5,
                },
                types: HashMap::new(),
                instances: vec![],
            },
            applications: ApplicationSection {
                defaults: ApplicationDefaults {
                    max_size: "1MB".to_string(),
                    timeout: 300,
                    retries: 3,
                    validation: ValidationConfig {
                        enabled: true,
                        strict: true,
                    },
                },
                templates: HashMap::new(),
                instances: vec![],
            },
            infrastructure: InfrastructureConfig {
                ca: CertificateAuthorityConfig {
                    enabled: true,
                    cert_path: "certs/ca-cert.pem".to_string(),
                    key_path: "certs/ca-key.pem".to_string(),
                    validity_days: 365,
                },
                secret_store: SecretStoreConfig {
                    enabled: true,
                    store_type: "kubernetes".to_string(),
                    namespace: "wasmbed".to_string(),
                },
                monitoring: MonitoringConfig {
                    enabled: true,
                    metrics: MetricsConfig {
                        enabled: true,
                        port: 9090,
                    },
                    logging: LoggingConfig {
                        enabled: true,
                        level: "info".to_string(),
                        format: "json".to_string(),
                    },
                },
                database: DatabaseConfig {
                    enabled: true,
                    db_type: "etcd".to_string(),
                    endpoints: vec!["localhost:2379".to_string()],
                    namespace: "wasmbed".to_string(),
                },
            },
            kubernetes: KubernetesConfig {
                namespace: "wasmbed".to_string(),
                crds: vec![
                    "device-crd.yaml".to_string(),
                    "application-crd.yaml".to_string(),
                    "gateway-crd.yaml".to_string(),
                ],
                rbac: vec![
                    "device-controller-rbac.yaml".to_string(),
                    "application-controller-rbac.yaml".to_string(),
                    "gateway-controller-rbac.yaml".to_string(),
                ],
            },
            dashboard: DashboardConfig {
                enabled: true,
                port: 3000,
                theme: "light".to_string(),
                real_time_updates: true,
                polling_interval: 5000,
                websocket: WebSocketConfig {
                    enabled: true,
                    port: 3001,
                },
            },
            security: SecurityConfig {
                tls: TlsConfig {
                    enabled: true,
                    min_version: "1.2".to_string(),
                    cipher_suites: vec![
                        "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384".to_string(),
                        "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256".to_string(),
                    ],
                },
                authentication: AuthenticationConfig {
                    enabled: true,
                    auth_type: "certificate".to_string(),
                    required: true,
                },
                authorization: AuthorizationConfig {
                    enabled: true,
                    rbac: true,
                    policies: vec!["admin".to_string(), "operator".to_string(), "viewer".to_string()],
                },
            },
            performance: PerformanceConfig {
                connection_pool: ConnectionPoolConfig {
                    max_connections: 100,
                    idle_timeout: 300,
                    max_lifetime: 3600,
                },
                cache: CacheConfig {
                    enabled: true,
                    ttl: 300,
                    max_size: "100MB".to_string(),
                },
                rate_limit: RateLimitConfig {
                    enabled: true,
                    requests_per_minute: 1000,
                    burst_size: 100,
                },
            },
            development: DevelopmentConfig {
                debug: false,
                verbose: false,
                mock_data: false,
                hot_reload: true,
                profiling: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_serialization() {
        let config = WasmbedConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let deserialized: WasmbedConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.platform.name, deserialized.platform.name);
    }

    #[test]
    fn test_gateway_operations() {
        let mut config = WasmbedConfig::default();
        
        let gateway = GatewayConfig {
            name: "test-gateway".to_string(),
            endpoint: "127.0.0.1:30452".to_string(),
            tls_port: 30452,
            http_port: 30453,
            max_devices: 50,
            region: "us-west-1".to_string(),
            enabled: true,
        };
        
        config.add_gateway(gateway.clone());
        assert!(config.get_gateway("test-gateway").is_some());
        
        config.update_gateway("test-gateway", gateway.clone());
        assert!(config.get_gateway("test-gateway").is_some());
        
        assert!(config.remove_gateway("test-gateway"));
        assert!(config.get_gateway("test-gateway").is_none());
    }
}
