//! TV'deki "PS5 Ceviri" servisini (_ps5ceviri._tcp, Android NSD) yerel ağda mDNS ile bulur.

use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

pub const SERVICE_TYPE: &str = "_ps5ceviri._tcp.local.";

/// "ip:port" ya da `timeout` içinde bulunamazsa `None`. IPv4 seçilir: IPv6 link-local adres
/// (fe80::…%en0) zone bilgisi olmadan URL'de kullanılamıyor.
pub fn find_tv(timeout: Duration) -> Option<String> {
    let daemon = ServiceDaemon::new().ok()?;
    let receiver = daemon.browse(SERVICE_TYPE).ok()?;
    let deadline = Instant::now() + timeout;
    let found = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break None;
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                if let Some(ip) = info.get_addresses_v4().into_iter().next() {
                    break Some(format!("{ip}:{}", info.get_port()));
                }
            }
            Ok(_) => {}
            Err(_) => break None,
        }
    };
    let _ = daemon.shutdown();
    found
}
