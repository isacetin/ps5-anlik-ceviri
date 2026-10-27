//! `ceviri`: masaüstü iş hattını platform katmanıyla birlikte çalıştıran komut satırı aracı.
//! Ana thread olay döngüsünü çalıştırır, iş worker thread'de yapılır (GUI uygulamasıyla aynı düzen).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ceviri_core::{block_merger, storage, ReadError, Stabilizer};
use ceviri_engine::{discovery, tv_client, Config, DeepLClient, Event, Pipeline, TvClient};
use ceviri_platform as platform;

const USAGE: &str = "\
Kullanım:
  ceviri run [seçenekler]        Remote Play'i okuyup Türkçe çeviriyi TV'ye gönderir (Ctrl+C ile çık)
    --tv HOST[:PORT]             TV adresi (verilmezse yerel ağda aranır; port varsayılan 8765)
    --dry-run                    DeepL'e gitmeden İngilizce metni gönderir (hizalama testi, kota harcamaz)
    --fps N                      Saniyedeki tur (varsayılan 2, en az 1)
    --verbose                    Her turun süresini yazar
    --once                       Tek tur çalışıp çıkar
  ceviri read [seçenekler]       TV'siz: OCR satırlarını ve sabit blokları yazar
    --count N                    Tur sayısı (varsayılan 5)
  ceviri translate \"METİN\"       DeepL anahtarını tek bir metinle dener (önbelleği kullanmaz)
  ceviri windows                 Ekrandaki pencereleri listeler

Ortak seçenekler (run, read):
    --bundle ID                  Okunacak uygulama (varsayılan: PS Remote Play). Mac: bundle ID,
                                 Windows: pencere başlığında ya da exe adında geçen metin
    --titlebar N                 Başlık çubuğu yüksekliği; Mac'te pt, Windows'ta piksel
                                 (varsayılan: tam ekranda 0, pencerede otomatik)
    --dump KLASÖR                Her turda blok kutuları çizilmiş kareyi kaydeder";

fn option(args: &[String], name: &str) -> Option<String> {
    let index = args.iter().position(|a| a == name)?;
    args.get(index + 1).cloned()
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    platform::run(move || match args.first().map(String::as_str) {
        Some("run") => run(&args),
        Some("read") => read(&args),
        Some("translate") => translate(&args),
        Some("windows") => list_windows(),
        _ => {
            println!("{USAGE}");
            Err(String::new())
        }
    });
}

fn dump_dir(args: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(dir) = option(args, "--dump").map(PathBuf::from) else { return Ok(None) };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Some(dir))
}

fn run(args: &[String]) -> Result<(), String> {
    let dry_run = flag(args, "--dry-run");
    let verbose = flag(args, "--verbose");
    let fps: f64 = option(args, "--fps").and_then(|v| v.parse().ok()).unwrap_or(2.0f64).max(1.0);
    let title_bar = option(args, "--titlebar").and_then(|v| v.parse().ok());

    let mut config = Config { fps, dump_dir: dump_dir(args)?, once: flag(args, "--once"), ..Config::default() };
    if !dry_run {
        let key = storage::deepl_key().ok_or_else(|| {
            let path = storage::deepl_key_path().map(|p| p.display().to_string()).unwrap_or_default();
            format!("DeepL anahtarı bulunamadı. DEEPL_AUTH_KEY ortam değişkenini ayarla ya da anahtarı {path} dosyasına koy. Çevirisiz denemek için --dry-run.")
        })?;
        let deepl = DeepLClient::new(key);
        config.cache_path = storage::cache_path();
        config.cache = config.cache_path.as_deref().map(storage::load_cache).unwrap_or_default();
        println!("DeepL: {}  önbellekte {} çeviri", deepl.host(), config.cache.len());
        config.backend = Some(Arc::new(move |texts: &[String]| deepl.translate(texts)));
    }

    let address = match option(args, "--tv") {
        Some(tv) => tv_client::with_default_port(&tv),
        None => {
            println!("TV yerel ağda aranıyor ({})...", discovery::SERVICE_TYPE);
            discovery::find_tv(Duration::from_secs(5))
                .ok_or("TV bulunamadı. TV'de overlay servisi açık mı? Adresi elle ver: --tv 192.168.1.5")?
        }
    };
    let tv = TvClient::new(address);
    println!("TV: {}  fps: {fps}  mod: {}  (Ctrl+C ile çık)", tv.url(), if dry_run { "çevirisiz (dry-run)" } else { "DeepL EN→TR" });
    println!("OCR modeli yükleniyor; ilk okuma ~30 sn sürebilir...");

    let reader = platform::screen_reader(option(args, "--bundle").as_deref(), fps, title_bar)?;
    let mut pipeline = Pipeline::new(reader, tv, config);
    pipeline.run(|event| match event {
        Event::Round(stats) if verbose => println!(
            "#{} video {}x{}  yakalama {} ms  OCR {} ms  {} blok, {} sabit, {} gönderilen",
            stats.round, stats.video_width, stats.video_height, stats.capture_ms, stats.ocr_ms, stats.merged, stats.stable, stats.sent
        ),
        Event::Round(_) => {}
        Event::WaitingForWindow(message) => println!("{message} Açılması bekleniyor..."),
        Event::WindowFound => println!("Pencere bulundu."),
        Event::Shown(texts) if texts.is_empty() => println!("TV: (boş)"),
        Event::Shown(texts) => println!("TV: {}", texts.join("  |  ")),
        Event::TvConnected => println!("TV'ye bağlandı."),
        Event::TvUnreachable(error) => eprintln!("{error}"),
        Event::TranslationFailed(error) => eprintln!("DeepL hatası: {error}"),
        Event::RoundSkipped(error) => eprintln!("Tur atlandı: {error}"),
    });
    Ok(())
}

fn read(args: &[String]) -> Result<(), String> {
    let count: u32 = option(args, "--count").and_then(|v| v.parse().ok()).unwrap_or(5);
    let fps: f64 = option(args, "--fps").and_then(|v| v.parse().ok()).unwrap_or(2.0f64).max(0.2);
    let title_bar = option(args, "--titlebar").and_then(|v| v.parse().ok());
    let dump_dir = dump_dir(args)?;

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
            std::thread::sleep(Duration::from_secs_f64(1.0 / fps).saturating_sub(round_start.elapsed()));
        }
    }
    Ok(())
}

fn translate(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Kullanım: ceviri translate \"Door is locked.\"".into());
    }
    let key = storage::deepl_key().ok_or("DeepL anahtarı bulunamadı")?;
    let text = args[1..].join(" ");
    let translated = DeepLClient::new(key).translate(std::slice::from_ref(&text)).map_err(|e| format!("DeepL hatası: {e}"))?;
    println!("{text}  →  {}", translated.first().map(String::as_str).unwrap_or(""));
    Ok(())
}

fn list_windows() -> Result<(), String> {
    for window in platform::list_windows()? {
        println!("{}  {}x{}  \"{}\"", window.app, window.width, window.height, window.title);
    }
    Ok(())
}
