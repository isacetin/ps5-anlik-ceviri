//! `ceviri`: masaüstü iş hattını platform katmanıyla birlikte deneyen komut satırı aracı.
//! Ana thread olay döngüsünü çalıştırır, iş worker thread'de yapılır (GUI uygulamasıyla aynı düzen).

mod platform;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ceviri_core::{block_merger, ReadError, Stabilizer};

const USAGE: &str = "\
Kullanım:
  ceviri windows                 Ekrandaki pencereleri listeler
  ceviri read [seçenekler]       Pencereyi okuyup OCR satırlarını, blokları ve sabit blokları yazar
    --bundle ID                  Okunacak uygulama (varsayılan: PS Remote Play)
    --count N                    Tur sayısı (varsayılan 5)
    --fps N                      Saniyedeki tur (varsayılan 2)
    --titlebar PT                Başlık çubuğu yüksekliği (varsayılan: tam ekranda 0, pencerede 28)
    --dump KLASÖR                Her turda blok kutuları çizilmiş kareyi kaydeder";

fn option(args: &[String], name: &str) -> Option<String> {
    let index = args.iter().position(|a| a == name)?;
    args.get(index + 1).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    platform::run(move || match args.first().map(String::as_str) {
        Some("windows") => list_windows(),
        Some("read") => read(&args),
        _ => {
            println!("{USAGE}");
            Err(String::new())
        }
    });
}

fn list_windows() -> Result<(), String> {
    for window in platform::list_windows()? {
        println!("{}  {}x{}  \"{}\"", window.app, window.width, window.height, window.title);
    }
    Ok(())
}

fn read(args: &[String]) -> Result<(), String> {
    let count: u32 = option(args, "--count").and_then(|v| v.parse().ok()).unwrap_or(5);
    let fps: f64 = option(args, "--fps").and_then(|v| v.parse().ok()).unwrap_or(2.0f64).max(0.2);
    let title_bar = option(args, "--titlebar").and_then(|v| v.parse().ok());
    let dump_dir = option(args, "--dump").map(PathBuf::from);
    if let Some(dir) = &dump_dir {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    let mut reader = platform::screen_reader(option(args, "--bundle").as_deref(), fps, title_bar)?;
    let mut stabilizer = Stabilizer::new();
    let started = Instant::now();
    println!("OCR modeli ilk turda yükleniyor; ilk okuma ~30 sn sürebilir...");

    for round in 1..=count {
        let round_start = Instant::now();
        match reader.read() {
            Ok(reading) => {
                let merged = block_merger::merge(&reading.lines);
                let stable = stabilizer.update(&merged, started.elapsed().as_secs_f64());
                println!(
                    "#{round} video {}x{}{}  yakalama {} ms  OCR {} ms  {} satır, {} blok, {} sabit",
                    reading.video_width,
                    reading.video_height,
                    if reading.fullscreen { " (tam ekran)" } else { "" },
                    reading.capture_ms,
                    reading.ocr_ms,
                    reading.lines.len(),
                    merged.len(),
                    stable.len()
                );
                for block in &stable {
                    let b = block.bbox;
                    println!("    {} [x={:.3} y={:.3} w={:.3} h={:.3}] \"{}\"", block.id, b.x, b.y, b.width, b.height, block.text);
                }
                if let Some(dir) = &dump_dir {
                    let boxes: Vec<_> = merged.iter().map(|m| m.bbox).collect();
                    reader.dump(&dir.join(format!("ocr_{round:04}.png")), &boxes)?;
                }
            }
            Err(ReadError::NoFrameYet) => println!("#{round} akış açılıyor, henüz kare yok"),
            Err(ReadError::WindowNotFound(message)) => println!("#{round} {message}"),
            Err(error) => return Err(error.to_string()),
        }
        if round < count {
            let remaining = Duration::from_secs_f64(1.0 / fps).saturating_sub(round_start.elapsed());
            std::thread::sleep(remaining);
        }
    }
    Ok(())
}
