use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init_tracing() {
    let env_filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .parse("melnode=debug,metrics=info")
        .unwrap();

    let subscriber_builder = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .with(env_filter);

    #[cfg(feature = "metrics")]
    let subscriber_builder = {
        let metrics_file = File::create("metrics.csv").expect("Failed to create metrics file");
        let metrics_layer = MetricsLayer::new(metrics_file);
        subscriber_builder.with(metrics_layer)
    };

    let _ = subscriber_builder.try_init();
}

/// Emits a custom metrics that gets written to `metrics.csv`.
/// Enabled using the `metrics` feature flag (e.g. `cargo run --features metrics --bin melnode`).
pub fn emit_metric(metric_name: &str, value: f64) {
    tracing::info!(target: "metrics", metric_name = metric_name, value = value, "Captured metric: {} with value {}", metric_name, value);
}

struct CsvVisitor {
    values: Vec<String>,
}

impl CsvVisitor {
    fn new() -> Self {
        CsvVisitor { values: Vec::new() }
    }
}

impl Visit for CsvVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "metric_name" || field.name() == "value" {
            self.values.push(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "metric_name" || field.name() == "value" {
            self.values.push(format!("{:?}", value));
        }
    }
}

pub struct MetricsLayer<W: Write> {
    writer: Mutex<csv::Writer<W>>,
}

impl<W: Write> MetricsLayer<W> {
    pub fn new(writer: W) -> Self {
        MetricsLayer {
            writer: Mutex::new(csv::Writer::from_writer(writer)),
        }
    }
}

impl<S, W> Layer<S> for MetricsLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: Write + 'static,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        if metadata.target() == "metrics" && metadata.level() == &tracing::Level::INFO {
            let mut visitor = CsvVisitor::new();
            event.record(&mut visitor);

            // Lock the mutex before writing
            let mut writer = self.writer.lock().expect("Writer mutex was poisoned");

            if let Err(e) = writer.write_record(&visitor.values) {
                eprintln!("Failed to write metrics to CSV: {}", e);
            }
            if let Err(e) = writer.flush() {
                eprintln!("Failed to flush metrics CSV: {}", e);
            }
        }
    }
}
