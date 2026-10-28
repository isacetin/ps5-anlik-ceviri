//! `ceviri`: masaüstü iş hattını platform katmanıyla birlikte çalıştıran komut satırı aracı.
//! Ana thread olay döngüsünü çalıştırır, iş worker thread'de yapılır (GUI uygulamasıyla aynı düzen).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ceviri_core::{block_merger, storage, terms, ReadError, Rect, SharedRegion, Stabilizer, Terms};
use ceviri_engine::pipeline::Backend;
use ceviri_engine::{discovery, tv_client, Config, DeepLClient, Event, Pipeline, TvClient};
use ceviri_opus::{model, OpusTranslator};
use ceviri_platform as platform;

const USAGE: &str = "\
Kullanım:
  ceviri run [seçenekler]        Remote Play'i okuyup Türkçe çeviriyi TV'ye gönderir (Ctrl+C ile çık)
    --tv HOST[:PORT]             TV adresi (verilmezse yerel ağda aranır; port varsayılan 8765)
    --dry-run                    Çevirmeden İngilizce metni gönderir (hizalama testi, kota harcamaz)
    --yerel                      DeepL yerine bilgisayardaki çeviri modelini kullanır (önce: ceviri model-indir)
    --fps N                      Saniyedeki tur (varsayılan 2, en az 1)
    --keep \"AD1,AD2\"              Çevrilmeyecek adlar (oyun, karakter); PlayStation, PS5 vb. hep korunur
    --verbose                    Her turun süresini yazar
    --once                       Tek tur çalışıp çıkar
  ceviri read [seçenekler]       TV'siz: OCR satırlarını ve sabit blokları yazar
    --count N                    Tur sayısı (varsayılan 5)
    --oyun PENCERE               Remote Play yerine bu bilgisayardaki oyunu okur (yalnızca Windows);
                                 PENCERE başlıkta ya da exe adında geçen metin (bkz. ceviri windows)
    --bolge X,Y,GEN,YUK          --oyun ile: yalnızca bu bölgeyi okur (0-1, ör. 0,0.7,1,0.3)
  ceviri translate [--yerel] \"METİN\"  Tek bir metni çevirir (önbelleği kullanmaz; --yerel: yerel model)
  ceviri model-indir             Yerel çeviri modelini indirir (~241 MB)
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
        Some("model-indir") => download_model(),
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

    let keep: Vec<String> = option(args, "--keep").map(|v| v.split(',').map(str::to_string).collect()).unwrap_or_default();
    let mut config = Config { fps, dump_dir: dump_dir(args)?, once: flag(args, "--once"), terms: Terms::new(&keep), ..Config::default() };
    let local = flag(args, "--yerel");
    if !dry_run && local {
        let opus = load_local()?;
        config.cache_path = storage::local_cache_path();
        config.cache = config.cache_path.as_deref().map(|path| storage::load_local_cache(path, storage::cache_path().as_deref())).unwrap_or_default();
        println!("Yerel model yüklendi  önbellekte {} çeviri", config.cache.len());
        config.backend = Some(Arc::new(move |texts: &[String]| opus.translate(texts)));
    } else if !dry_run {
        let deepl = DeepLClient::new(deepl_key()?);
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
    let mode = match (dry_run, local) {
        (true, _) => "çevirisiz (dry-run)",
        (false, true) => "yerel model EN→TR",
        (false, false) => "DeepL EN→TR",
    };
    println!("TV: {}  fps: {fps}  mod: {mode}  (Ctrl+C ile çık)", tv.url());
    println!("OCR modeli yükleniyor; ilk okuma ~30 sn sürebilir...");

    let reader = platform::screen_reader(option(args, "--bundle").as_deref(), fps, title_bar)?;
    let mut pipeline = Pipeline::new(reader, Some(tv), config);
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
        Event::TranslationFailed(error) => eprintln!("Çeviri hatası: {error}"),
        Event::RoundSkipped(error) => eprintln!("Tur atlandı: {error}"),
        Event::Frame { .. } => {}
    });
    Ok(())
}

fn read(args: &[String]) -> Result<(), String> {
    let count: u32 = option(args, "--count").and_then(|v| v.parse().ok()).unwrap_or(5);
    let fps: f64 = option(args, "--fps").and_then(|v| v.parse().ok()).unwrap_or(2.0f64).max(0.2);
    let title_bar = option(args, "--titlebar").and_then(|v| v.parse().ok());
    let dump_dir = dump_dir(args)?;

    let mut reader = match option(args, "--oyun") {
        Some(window) => platform::game_reader(&window, SharedRegion::new(region_option(args)?.into()))?,
        None => platform::screen_reader(option(args, "--bundle").as_deref(), fps, title_bar)?,
    };
    let mut stabilizer = Stabilizer::new();
    let started = Instant::now();
    println!("OCR modeli ilk turda yükleniyor; ilk okuma ~30 sn sürebilir...");

    for round in 1..=count {
        let round_start = Instant::now();
        match reader.read() {
            Ok(reading) => {
                let aspect = reading.video_width as f64 / reading.video_height.max(1) as f64;
                let merged = block_merger::merge_with(&reading.lines, &block_merger::Options { aspect, ..Default::default() });
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

/// `--bolge 0,0.7,1,0.3` → normalize dikdörtgen.
fn region_option(args: &[String]) -> Result<Option<Rect>, String> {
    let Some(value) = option(args, "--bolge") else { return Ok(None) };
    let parts: Vec<f64> = value.split(',').map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("--bolge: sayı bekleniyordu: {value}"))?;
    let [x, y, width, height] = parts[..] else { return Err("--bolge: dört sayı gerekir: X,Y,GENİŞLİK,YÜKSEKLİK".into()) };
    ceviri_core::geometry::clamp_region(Rect::new(x, y, width, height)).map(Some).ok_or_else(|| format!("--bolge: bölge çok küçük: {value}"))
}

fn deepl_key() -> Result<String, String> {
    storage::deepl_key().ok_or_else(|| {
        let path = storage::deepl_key_path().map(|p| p.display().to_string()).unwrap_or_default();
        format!("DeepL anahtarı bulunamadı. DEEPL_AUTH_KEY ortam değişkenini ayarla ya da anahtarı {path} dosyasına koy. Çevirisiz denemek için --dry-run, yerel model için --yerel.")
    })
}

fn load_local() -> Result<OpusTranslator, String> {
    let dir = storage::local_model_dir().ok_or("uygulama klasörü bulunamadı")?;
    if !model::is_installed(&dir) {
        return Err("Yerel çeviri modeli yok. Önce: ceviri model-indir".into());
    }
    OpusTranslator::load(&dir)
}

fn translate(args: &[String]) -> Result<(), String> {
    let local = flag(args, "--yerel");
    let words: Vec<&str> = args[1..].iter().map(String::as_str).filter(|a| *a != "--yerel").collect();
    if words.is_empty() {
        return Err("Kullanım: ceviri translate [--yerel] \"Door is locked.\"".into());
    }
    let text = words.join(" ");
    let protected = Terms::new(&[]).protect(&text);
    let backend: Backend = if local {
        let opus = load_local()?;
        Arc::new(move |texts: &[String]| opus.translate(texts))
    } else {
        let deepl = DeepLClient::new(deepl_key()?);
        Arc::new(move |texts: &[String]| deepl.translate(texts))
    };
    let started = Instant::now();
    let translated = backend(&[protected]).map_err(|e| format!("Çeviri hatası: {e}"))?;
    println!("{text}  →  {}  ({} ms)", terms::unprotect(translated.first().map(String::as_str).unwrap_or("")), started.elapsed().as_millis());
    Ok(())
}

fn download_model() -> Result<(), String> {
    let dir = storage::local_model_dir().ok_or("uygulama klasörü bulunamadı")?;
    println!("Yerel çeviri modeli indiriliyor: {}", dir.display());
    let mut last = u64::MAX;
    model::download(&dir, |done, total| {
        let percent = done * 100 / total.max(1);
        if percent / 10 != last / 10 || done == total {
            println!("  %{percent}  ({} / {} MB)", done / 1_000_000, total / 1_000_000);
        }
        last = percent;
    })?;
    println!("Hazır. Deneme: ceviri translate --yerel \"The door is locked.\"");
    Ok(())
}

fn list_windows() -> Result<(), String> {
    for window in platform::list_windows()? {
        println!("{}  {}x{}  \"{}\"", window.app, window.width, window.height, window.title);
    }
    Ok(())
}
