use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

// Metrics collection for the SMTP relay
#[derive(Debug, Default)]
pub struct Metrics {
    pub connections_total: u64,
    pub connections_active: u64,
    pub emails_sent_total: u64,
    pub emails_failed_total: u64,
    pub bytes_processed_total: u64,
    pub response_times: Vec<Duration>,
    pub errors_by_type: std::collections::HashMap<String, u64>,
    pub uptime_start: Option<Instant>,
}

// Serializable version of metrics for JSON output
#[derive(Debug, Serialize)]
pub struct SerializableMetrics {
    pub connections_total: u64,
    pub connections_active: u64,
    pub emails_sent_total: u64,
    pub emails_failed_total: u64,
    pub bytes_processed_total: u64,
    pub response_times_count: usize,
    pub errors_by_type: std::collections::HashMap<String, u64>,
    pub uptime_seconds: Option<u64>,
    pub average_response_time_ms: Option<u64>,
    pub success_rate_percent: f64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            uptime_start: Some(Instant::now()),
            ..Default::default()
        }
    }

    pub fn increment_connections(&mut self) {
        self.connections_total += 1;
        self.connections_active += 1;
    }

    pub fn decrement_active_connections(&mut self) {
        if self.connections_active > 0 {
            self.connections_active -= 1;
        }
    }

    pub fn increment_emails_sent(&mut self) {
        self.emails_sent_total += 1;
    }

    pub fn increment_emails_failed(&mut self) {
        self.emails_failed_total += 1;
    }

    pub fn add_bytes_processed(&mut self, bytes: u64) {
        self.bytes_processed_total += bytes;
    }

    pub fn record_response_time(&mut self, duration: Duration) {
        // Keep only the last 1000 response times to prevent unbounded growth
        if self.response_times.len() >= 1000 {
            self.response_times.remove(0);
        }
        self.response_times.push(duration);
    }

    pub fn increment_error(&mut self, error_type: &str) {
        *self.errors_by_type.entry(error_type.to_string()).or_insert(0) += 1;
    }

    pub fn get_average_response_time(&self) -> Option<Duration> {
        if self.response_times.is_empty() {
            return None;
        }
        
        let total: Duration = self.response_times.iter().sum();
        Some(total / self.response_times.len() as u32)
    }

    pub fn get_uptime(&self) -> Option<Duration> {
        self.uptime_start.map(|start| start.elapsed())
    }

    pub fn get_success_rate(&self) -> f64 {
        let total = self.emails_sent_total + self.emails_failed_total;
        if total == 0 {
            return 1.0;
        }
        self.emails_sent_total as f64 / total as f64
    }

    // Convert to a serializable version
    pub fn to_serializable(&self) -> SerializableMetrics {
        SerializableMetrics {
            connections_total: self.connections_total,
            connections_active: self.connections_active,
            emails_sent_total: self.emails_sent_total,
            emails_failed_total: self.emails_failed_total,
            bytes_processed_total: self.bytes_processed_total,
            response_times_count: self.response_times.len(),
            errors_by_type: self.errors_by_type.clone(),
            uptime_seconds: self.get_uptime().map(|d| d.as_secs()),
            average_response_time_ms: self.get_average_response_time().map(|d| d.as_millis() as u64),
            success_rate_percent: self.get_success_rate() * 100.0,
        }
    }
}

// Thread-safe metrics collector
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    inner: Arc<RwLock<Metrics>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Metrics::new())),
        }
    }

    pub async fn increment_connections(&self) {
        let mut metrics = self.inner.write().await;
        metrics.increment_connections();
    }

    pub async fn decrement_active_connections(&self) {
        let mut metrics = self.inner.write().await;
        metrics.decrement_active_connections();
    }

    pub async fn increment_emails_sent(&self) {
        let mut metrics = self.inner.write().await;
        metrics.increment_emails_sent();
    }

    pub async fn increment_emails_failed(&self) {
        let mut metrics = self.inner.write().await;
        metrics.increment_emails_failed();
    }

    pub async fn add_bytes_processed(&self, bytes: u64) {
        let mut metrics = self.inner.write().await;
        metrics.add_bytes_processed(bytes);
    }

    pub async fn record_response_time(&self, duration: Duration) {
        let mut metrics = self.inner.write().await;
        metrics.record_response_time(duration);
    }

    pub async fn increment_error(&self, error_type: &str) {
        let mut metrics = self.inner.write().await;
        metrics.increment_error(error_type);
    }

    pub async fn get_snapshot(&self) -> Metrics {
        let metrics = self.inner.read().await;
        Metrics {
            connections_total: metrics.connections_total,
            connections_active: metrics.connections_active,
            emails_sent_total: metrics.emails_sent_total,
            emails_failed_total: metrics.emails_failed_total,
            bytes_processed_total: metrics.bytes_processed_total,
            response_times: metrics.response_times.clone(),
            errors_by_type: metrics.errors_by_type.clone(),
            uptime_start: metrics.uptime_start,
        }
    }

    // Log current metrics at INFO level
    pub async fn log_metrics(&self) {
        let metrics = self.get_snapshot().await;
        
        info!(
            connections_total = metrics.connections_total,
            connections_active = metrics.connections_active,
            emails_sent = metrics.emails_sent_total,
            emails_failed = metrics.emails_failed_total,
            bytes_processed = metrics.bytes_processed_total,
            success_rate = format!("{:.2}%", metrics.get_success_rate() * 100.0),
            avg_response_time = ?metrics.get_average_response_time(),
            uptime = ?metrics.get_uptime(),
            "Current metrics"
        );

        if !metrics.errors_by_type.is_empty() {
            warn!(errors = ?metrics.errors_by_type, "Error breakdown");
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

// Start a background task to periodically log metrics
pub fn start_metrics_logger(collector: MetricsCollector, interval: Duration) {
    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(interval);
        loop {
            interval_timer.tick().await;
            collector.log_metrics().await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_metrics_collection() {
        let collector = MetricsCollector::new();

        collector.increment_connections().await;
        collector.increment_emails_sent().await;
        collector.add_bytes_processed(1024).await;
        collector.record_response_time(Duration::from_millis(100)).await;

        let metrics = collector.get_snapshot().await;
        assert_eq!(metrics.connections_total, 1);
        assert_eq!(metrics.connections_active, 1);
        assert_eq!(metrics.emails_sent_total, 1);
        assert_eq!(metrics.bytes_processed_total, 1024);
        assert_eq!(metrics.response_times.len(), 1);
    }

    #[tokio::test]
    async fn test_success_rate_calculation() {
        let collector = MetricsCollector::new();

        // Initially 100% success rate (no emails)
        let metrics = collector.get_snapshot().await;
        assert_eq!(metrics.get_success_rate(), 1.0);

        // Send 3 successful, 1 failed
        for _ in 0..3 {
            collector.increment_emails_sent().await;
        }
        collector.increment_emails_failed().await;

        let metrics = collector.get_snapshot().await;
        assert_eq!(metrics.get_success_rate(), 0.75);
    }
}
