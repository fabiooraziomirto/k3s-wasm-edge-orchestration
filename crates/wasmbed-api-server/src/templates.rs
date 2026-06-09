// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use tracing::{error, info, warn};

use crate::{SystemStatus, DeviceInfo, ApplicationInfo, GatewayInfo, monitoring::MetricValue};

/// Dashboard Templates for HTML rendering
#[derive(Debug)]
pub struct DashboardTemplates;

impl DashboardTemplates {
    pub fn new() -> Self {
        Self
    }

    pub async fn render_dashboard(&self, status: &SystemStatus) -> anyhow::Result<String> {
        let html = format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Wasmbed Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 0; padding: 20px; background-color: #f5f5f5; }}
        .container {{ max-width: 1200px; margin: 0 auto; }}
        .header {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .stats {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 20px; margin-bottom: 20px; }}
        .stat-card {{ background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .stat-card h3 {{ margin: 0 0 10px 0; color: #333; }}
        .stat-value {{ font-size: 2em; font-weight: bold; color: #007bff; }}
        .nav {{ background: white; padding: 15px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav a {{ margin-right: 20px; text-decoration: none; color: #007bff; font-weight: bold; }}
        .nav a:hover {{ text-decoration: underline; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Wasmbed Platform Dashboard</h1>
            <p>System Status Overview</p>
        </div>
        
        <div class="nav">
            <a href="/">Dashboard</a>
            <a href="/devices">Devices</a>
            <a href="/applications">Applications</a>
            <a href="/gateways">Gateways</a>
            <a href="/monitoring">Monitoring</a>
        </div>
        
        <div class="stats">
            <div class="stat-card">
                <h3>Devices</h3>
                <div class="stat-value">{}</div>
                <p>Connected: {} | Enrolled: {} | Unreachable: {}</p>
            </div>
            <div class="stat-card">
                <h3>Applications</h3>
                <div class="stat-value">{}</div>
                <p>Running: {} | Pending: {} | Failed: {}</p>
            </div>
            <div class="stat-card">
                <h3>Gateways</h3>
                <div class="stat-value">{}</div>
                <p>Active: {} | Inactive: {}</p>
            </div>
            <div class="stat-card">
                <h3>Infrastructure</h3>
                <div class="stat-value">Healthy</div>
                <p>CA: {} | Secrets: {} | Monitoring: {} | Logging: {}</p>
            </div>
        </div>
        
        <div class="header">
            <h2>System Information</h2>
            <p>Uptime: {} seconds</p>
            <p>Last Update: {:?}</p>
        </div>
    </div>
</body>
</html>
        "#,
            status.devices.total,
            status.devices.connected,
            status.devices.enrolled,
            status.devices.unreachable,
            status.applications.total,
            status.applications.running,
            status.applications.pending,
            status.applications.failed,
            status.gateways.total,
            status.gateways.active,
            status.gateways.inactive,
            status.infrastructure.ca_status,
            status.infrastructure.secret_store_status,
            status.infrastructure.monitoring_status,
            status.infrastructure.logging_status,
            status.uptime,
            status.last_update
        );

        Ok(html)
    }

    pub async fn render_devices(&self, devices: &[DeviceInfo]) -> anyhow::Result<String> {
        let device_rows = devices.iter().map(|device| {
            format!(r#"
                <tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                </tr>
            "#,
                device.device_id,
                device.device_type,
                device.architecture,
                device.status,
                device.last_heartbeat.map(|t| format!("{:?}", t)).unwrap_or_else(|| "Never".to_string()),
                device.gateway_id.as_deref().unwrap_or("None")
            )
        }).collect::<Vec<_>>().join("");

        let html = format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Devices - Wasmbed Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 0; padding: 20px; background-color: #f5f5f5; }}
        .container {{ max-width: 1200px; margin: 0 auto; }}
        .header {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav {{ background: white; padding: 15px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav a {{ margin-right: 20px; text-decoration: none; color: #007bff; font-weight: bold; }}
        .nav a:hover {{ text-decoration: underline; }}
        table {{ width: 100%; border-collapse: collapse; background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        th, td {{ padding: 12px; text-align: left; border-bottom: 1px solid #ddd; }}
        th {{ background-color: #f8f9fa; font-weight: bold; }}
        tr:hover {{ background-color: #f5f5f5; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Devices</h1>
            <p>Manage and monitor connected devices</p>
        </div>
        
        <div class="nav">
            <a href="/">Dashboard</a>
            <a href="/devices">Devices</a>
            <a href="/applications">Applications</a>
            <a href="/gateways">Gateways</a>
            <a href="/monitoring">Monitoring</a>
        </div>
        
        <table>
            <thead>
                <tr>
                    <th>Device ID</th>
                    <th>Type</th>
                    <th>Architecture</th>
                    <th>Status</th>
                    <th>Last Heartbeat</th>
                    <th>Gateway</th>
                </tr>
            </thead>
            <tbody>
                {}
            </tbody>
        </table>
    </div>
</body>
</html>
        "#, device_rows);

        Ok(html)
    }

    pub async fn render_applications(&self, applications: &[ApplicationInfo]) -> anyhow::Result<String> {
        let app_rows = applications.iter().map(|app| {
            format!(r#"
                <tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                </tr>
            "#,
                app.app_id,
                app.name,
                app.image,
                app.status,
                app.deployed_devices.join(", "),
                format!("{:?}", app.created_at)
            )
        }).collect::<Vec<_>>().join("");

        let html = format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Applications - Wasmbed Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 0; padding: 20px; background-color: #f5f5f5; }}
        .container {{ max-width: 1200px; margin: 0 auto; }}
        .header {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav {{ background: white; padding: 15px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav a {{ margin-right: 20px; text-decoration: none; color: #007bff; font-weight: bold; }}
        .nav a:hover {{ text-decoration: underline; }}
        table {{ width: 100%; border-collapse: collapse; background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        th, td {{ padding: 12px; text-align: left; border-bottom: 1px solid #ddd; }}
        th {{ background-color: #f8f9fa; font-weight: bold; }}
        tr:hover {{ background-color: #f5f5f5; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Applications</h1>
            <p>Manage deployed applications</p>
        </div>
        
        <div class="nav">
            <a href="/">Dashboard</a>
            <a href="/devices">Devices</a>
            <a href="/applications">Applications</a>
            <a href="/gateways">Gateways</a>
            <a href="/monitoring">Monitoring</a>
        </div>
        
        <table>
            <thead>
                <tr>
                    <th>App ID</th>
                    <th>Name</th>
                    <th>Image</th>
                    <th>Status</th>
                    <th>Deployed Devices</th>
                    <th>Created At</th>
                </tr>
            </thead>
            <tbody>
                {}
            </tbody>
        </table>
    </div>
</body>
</html>
        "#, app_rows);

        Ok(html)
    }

    pub async fn render_gateways(&self, gateways: &[GatewayInfo]) -> anyhow::Result<String> {
        let gateway_rows = gateways.iter().map(|gateway| {
            format!(r#"
                <tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                </tr>
            "#,
                gateway.gateway_id,
                gateway.endpoint,
                gateway.status,
                gateway.connected_devices,
                gateway.enrolled_devices
            )
        }).collect::<Vec<_>>().join("");

        let html = format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Gateways - Wasmbed Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 0; padding: 20px; background-color: #f5f5f5; }}
        .container {{ max-width: 1200px; margin: 0 auto; }}
        .header {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav {{ background: white; padding: 15px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav a {{ margin-right: 20px; text-decoration: none; color: #007bff; font-weight: bold; }}
        .nav a:hover {{ text-decoration: underline; }}
        table {{ width: 100%; border-collapse: collapse; background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        th, td {{ padding: 12px; text-align: left; border-bottom: 1px solid #ddd; }}
        th {{ background-color: #f8f9fa; font-weight: bold; }}
        tr:hover {{ background-color: #f5f5f5; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Gateways</h1>
            <p>Monitor gateway status and connections</p>
        </div>
        
        <div class="nav">
            <a href="/">Dashboard</a>
            <a href="/devices">Devices</a>
            <a href="/applications">Applications</a>
            <a href="/gateways">Gateways</a>
            <a href="/monitoring">Monitoring</a>
        </div>
        
        <table>
            <thead>
                <tr>
                    <th>Gateway ID</th>
                    <th>Endpoint</th>
                    <th>Status</th>
                    <th>Connected Devices</th>
                    <th>Enrolled Devices</th>
                </tr>
            </thead>
            <tbody>
                {}
            </tbody>
        </table>
    </div>
</body>
</html>
        "#, gateway_rows);

        Ok(html)
    }

    pub async fn render_monitoring(&self, metrics: &[MetricValue]) -> anyhow::Result<String> {
        let metric_rows = metrics.iter().map(|metric| {
            format!(r#"
                <tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                </tr>
            "#,
                metric.name,
                metric.value,
                format!("{:?}", metric.timestamp)
            )
        }).collect::<Vec<_>>().join("");

        let html = format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Monitoring - Wasmbed Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 0; padding: 20px; background-color: #f5f5f5; }}
        .container {{ max-width: 1200px; margin: 0 auto; }}
        .header {{ background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav {{ background: white; padding: 15px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .nav a {{ margin-right: 20px; text-decoration: none; color: #007bff; font-weight: bold; }}
        .nav a:hover {{ text-decoration: underline; }}
        table {{ width: 100%; border-collapse: collapse; background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        th, td {{ padding: 12px; text-align: left; border-bottom: 1px solid #ddd; }}
        th {{ background-color: #f8f9fa; font-weight: bold; }}
        tr:hover {{ background-color: #f5f5f5; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Monitoring</h1>
            <p>System metrics and health monitoring</p>
        </div>
        
        <div class="nav">
            <a href="/">Dashboard</a>
            <a href="/devices">Devices</a>
            <a href="/applications">Applications</a>
            <a href="/gateways">Gateways</a>
            <a href="/monitoring">Monitoring</a>
        </div>
        
        <table>
            <thead>
                <tr>
                    <th>Metric Name</th>
                    <th>Value</th>
                    <th>Timestamp</th>
                </tr>
            </thead>
            <tbody>
                {}
            </tbody>
        </table>
    </div>
</body>
</html>
        "#, metric_rows);

        Ok(html)
    }
}
