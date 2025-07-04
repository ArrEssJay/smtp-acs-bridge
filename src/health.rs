#[cfg(feature = "health-server")]
use warp::{Filter, Reply};

use crate::metrics::MetricsCollector;
use anyhow::Result;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tracing::{error, info, instrument};

// Health check status
#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub timestamp: u64,
    pub uptime_seconds: Option<u64>,
    pub version: String,
    pub metrics: Option<HealthMetrics>,
}

#[derive(Debug, Serialize)]
pub struct HealthMetrics {
    pub connections_total: u64,
    pub connections_active: u64,
    pub emails_sent_total: u64,
    pub emails_failed_total: u64,
    pub success_rate_percent: f64,
    pub average_response_time_ms: Option<u64>,
}

impl Default for HealthStatus {
    fn default() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            status: "healthy".to_string(),
            timestamp,
            uptime_seconds: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            metrics: None,
        }
    }
}

impl HealthStatus {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn with_metrics(mut self, collector: &MetricsCollector) -> Self {
        let metrics_snapshot = collector.get_snapshot().await;
        
        self.uptime_seconds = metrics_snapshot.get_uptime().map(|d| d.as_secs());
        self.metrics = Some(HealthMetrics {
            connections_total: metrics_snapshot.connections_total,
            connections_active: metrics_snapshot.connections_active,
            emails_sent_total: metrics_snapshot.emails_sent_total,
            emails_failed_total: metrics_snapshot.emails_failed_total,
            success_rate_percent: metrics_snapshot.get_success_rate() * 100.0,
            average_response_time_ms: metrics_snapshot
                .get_average_response_time()
                .map(|d| d.as_millis() as u64),
        });
        
        self
    }
}

// Start a health check HTTP server on a separate port
#[cfg(feature = "health-server")]
pub async fn start_health_server(
    bind_addr: std::net::SocketAddr,
    metrics_collector: MetricsCollector,
) -> Result<()> {
    let health = warp::path("health")
        .and(warp::get())
        .and(with_metrics(metrics_collector.clone()))
        .and_then(health_handler);

    let metrics = warp::path("metrics")
        .and(warp::get())
        .and(with_metrics(metrics_collector.clone()))
        .and_then(metrics_handler);

    let readiness = warp::path("ready")
        .and(warp::get())
        .and(with_metrics(metrics_collector))
        .and_then(readiness_handler);

    let routes = health.or(metrics).or(readiness);

    info!(bind_addr = %bind_addr, "Starting health check server");

    warp::serve(routes)
        .run(bind_addr)
        .await;

    Ok(())
}

#[cfg(feature = "health-server")]
fn with_metrics(
    metrics: MetricsCollector,
) -> impl Filter<Extract = (MetricsCollector,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || metrics.clone())
}

#[cfg(feature = "health-server")]
#[instrument(skip(metrics))]
async fn health_handler(metrics: MetricsCollector) -> Result<impl Reply, warp::Rejection> {
    let health_status = HealthStatus::new().with_metrics(&metrics).await;
    Ok(warp::reply::json(&health_status))
}

#[cfg(feature = "health-server")]
#[instrument(skip(metrics))]
async fn metrics_handler(metrics: MetricsCollector) -> Result<impl Reply, warp::Rejection> {
    let metrics_snapshot = metrics.get_snapshot().await;
    Ok(warp::reply::json(&metrics_snapshot.to_serializable()))
}

#[cfg(feature = "health-server")]
#[instrument(skip(metrics))]
async fn readiness_handler(metrics: MetricsCollector) -> Result<impl Reply, warp::Rejection> {
    // Simple readiness check - server is ready if it can serve requests
    let mut status = HealthStatus::new();
    
    // Check if we've had any recent failures
    let metrics_snapshot = metrics.get_snapshot().await;
    if metrics_snapshot.get_success_rate() < 0.5 && metrics_snapshot.emails_sent_total > 10 {
        status.status = "degraded".to_string();
    }
    
    status = status.with_metrics(&metrics).await;
    Ok(warp::reply::json(&status))
}

// Simple TCP health check that doesn't require HTTP
pub async fn simple_health_check(bind_addr: std::net::SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    info!(bind_addr = %bind_addr, "Starting simple TCP health check server");

    loop {
        match listener.accept().await {
            Ok((mut stream, addr)) => {
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
                    if let Err(e) = stream.write_all(response).await {
                        error!(client_addr = %addr, error = %e, "Failed to write health check response");
                    }
                });
            }
            Err(e) => {
                error!(error = %e, "Failed to accept health check connection");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_status_creation() {
        let health = HealthStatus::new();
        assert_eq!(health.status, "healthy");
        assert_eq!(health.version, env!("CARGO_PKG_VERSION"));
        assert!(health.timestamp > 0);
    }

    #[tokio::test]
    async fn test_health_status_with_metrics() {
        let collector = MetricsCollector::new();
        collector.increment_emails_sent().await;
        collector.increment_connections().await;

        let health = HealthStatus::new().with_metrics(&collector).await;
        
        assert!(health.metrics.is_some());
        let metrics = health.metrics.unwrap();
        assert_eq!(metrics.emails_sent_total, 1);
        assert_eq!(metrics.connections_total, 1);
    }
}
