//! One bounded machine sampler shared by all browser clients. No process arguments or secrets.
use crate::extract::Json;
use crate::state::AppState;
use axum::Router;
use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use ts_rs::TS;
#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct MachineSample {
    pub at_ms: f64,
    pub cpu_percent: Option<f32>,
    pub memory_used: f64,
    pub memory_total: f64,
    pub disk_available: f64,
    pub disk_total: f64,
    pub received_bytes: f64,
    pub transmitted_bytes: f64,
    pub interval_ms: f64,
}
static SAMPLES: OnceLock<Mutex<Vec<MachineSample>>> = OnceLock::new();
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/system/metrics", axum::routing::get(metrics))
}
async fn metrics() -> Json<Vec<MachineSample>> {
    let samples = SAMPLES.get_or_init(|| {
        std::thread::spawn(|| {
            let mut system = sysinfo::System::new_all();
            let mut networks = sysinfo::Networks::new_with_refreshed_list();
            let mut disks = sysinfo::Disks::new_with_refreshed_list();
            let mut last = std::time::Instant::now();
            let mut first = true;
            loop {
                system.refresh_cpu_all();
                system.refresh_memory();
                networks.refresh(true);
                disks.refresh(true);
                let interval = last.elapsed().as_millis() as f64;
                last = std::time::Instant::now();
                let (disk_available, disk_total) = disk_totals(disks.list().iter().map(|d| {
                    (
                        d.name().to_string_lossy().into_owned(),
                        d.available_space(),
                        d.total_space(),
                    )
                }));
                let sample = MachineSample {
                    at_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as f64,
                    cpu_percent: if first {
                        None
                    } else {
                        Some(system.global_cpu_usage())
                    },
                    memory_used: system.used_memory() as f64,
                    memory_total: system.total_memory() as f64,
                    disk_available,
                    disk_total,
                    received_bytes: networks.values().map(|n| n.received() as f64).sum(),
                    transmitted_bytes: networks.values().map(|n| n.transmitted() as f64).sum(),
                    interval_ms: interval,
                };
                first = false;
                if let Some(s) = SAMPLES.get() {
                    let mut s = s.lock().expect("s mutex must not be poisoned");
                    s.push(sample);
                    if s.len() > 120 {
                        s.remove(0);
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        });
        Mutex::new(vec![])
    });
    Json(
        samples
            .lock()
            .expect("samples mutex must not be poisoned")
            .clone(),
    )
}

// Bind mounts repeat a device; counting each mount invents storage capacity.
fn disk_totals(disks: impl IntoIterator<Item = (String, u64, u64)>) -> (f64, f64) {
    let mut devices = std::collections::HashMap::new();
    for (name, available, total) in disks {
        if name.is_empty() || matches!(name.as_str(), "tmpfs" | "devtmpfs" | "overlay" | "none") {
            continue;
        }
        devices
            .entry(name)
            .and_modify(|value: &mut (u64, u64)| {
                value.0 = value.0.min(available);
                value.1 = value.1.max(total);
            })
            .or_insert((available, total));
    }
    devices.values().fold((0.0, 0.0), |(available, total), d| {
        (available + d.0 as f64, total + d.1 as f64)
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn bind_mounts_do_not_multiply_capacity() {
        assert_eq!(
            super::disk_totals(vec![
                ("/dev/a".into(), 40, 100),
                ("/dev/a".into(), 39, 100),
                ("/dev/b".into(), 20, 50),
                ("tmpfs".into(), 200, 500)
            ]),
            (59.0, 150.0)
        );
    }
}
