//! Sürecin bellek kullanımı (teşhis paneli; uzun oturumda sızıntı olup olmadığını görmek için).
//! Ek bağımlılık kullanılmaz: Linux'ta /proc, Mac'te `ps` (10 sn'de bir çağrılır). Windows'ta yok.

/// Yerleşik bellek (RSS), MB.
pub fn memory_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        // statm: sayfa cinsinden; ikinci alan yerleşik bellek.
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096.0 / 1_048_576.0)
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/bin/ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().ok()?;
        let kilobytes: f64 = String::from_utf8_lossy(&output.stdout).trim().parse().ok()?;
        Some(kilobytes / 1024.0)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn bellek_olculur() {
        let mb = super::memory_mb().expect("bellek ölçülebilmeli");
        assert!(mb > 1.0 && mb < 100_000.0, "{mb}");
    }
}
