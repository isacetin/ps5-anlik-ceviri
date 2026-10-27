//! winprobe: Faz 0 test aracı. Windows'ta PS Remote Play penceresi yakalanabiliyor mu ve
//! Windows'un yerleşik OCR'ı oyun yazılarını ne kadar iyi okuyor, bunu ölçer.
//! Çıktılar (kareler, OCR kutuları çizilmiş görüntüler, rapor.txt) bir klasöre yazılır.

#[cfg(not(windows))]
fn main() {
    eprintln!("winprobe yalnızca Windows'ta çalışır.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    app::main();
}

#[cfg(windows)]
mod app {
    use std::fmt::Write as _;
    use std::io::{BufRead, Write as _};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use ceviri_core::geometry::video_rect;
    use ceviri_core::{FrameStats, TextLine};
    use ceviri_windows::capture::{self, WindowCapture};
    use ceviri_windows::imaging::{crop, draw_boxes, prepare_for_ocr};
    use ceviri_windows::ocr::{self, OcrSetup};
    use ceviri_windows::win::{self, WindowInfo};
    use image::RgbaImage;
    use windows::Media::Ocr::OcrEngine;

    const USAGE: &str = "\
Kullanım:
  winprobe                      Tam test (çift tıklayınca bu çalışır)
    --kare N                    Kare sayısı (varsayılan 8)
    --aralik SN                 Kareler arası saniye (varsayılan 1)
    --pencere METIN             Remote Play yerine başlığında/exe adında METIN geçen pencere
    --olcek X                   OCR'dan önce video alanını X kat büyüt (ör. 2; varsayılan 1)
    --baslik PX                 Başlık çubuğu yüksekliğini elle ver (varsayılan: otomatik)
  winprobe windows              Ekrandaki pencereleri listeler
  winprobe ocr DOSYA.png...     Görüntü dosyalarında OCR çalıştırır";

    /// Ekrana yazılan her satır rapor.txt'ye de girer.
    static REPORT: Mutex<String> = Mutex::new(String::new());

    macro_rules! say {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            println!("{line}");
            let mut report = REPORT.lock().unwrap();
            report.push_str(&line);
            report.push('\n');
        }};
    }

    struct Options {
        frames: u32,
        interval: f64,
        window_filter: Option<String>,
        scale: f64,
        title_bar: Option<i32>,
    }

    fn option(args: &[String], name: &str) -> Option<String> {
        let index = args.iter().position(|a| a == name)?;
        args.get(index + 1).cloned()
    }

    pub fn main() {
        win::enable_dpi_awareness();
        ceviri_windows::ensure_mta();
        let args: Vec<String> = std::env::args().skip(1).collect();
        let result = match args.first().map(String::as_str) {
            None => full_test(&args, true),
            Some(a) if a.starts_with("--") => full_test(&args, false),
            Some("windows") => {
                list_windows();
                Ok(())
            }
            Some("ocr") => ocr_files(&args[1..]),
            Some(_) => {
                println!("{USAGE}");
                std::process::exit(1);
            }
        };
        if let Err(error) = result {
            say!("HATA: {error}");
            if args.is_empty() {
                wait_enter("Kapatmak için Enter'a bas.");
            }
            std::process::exit(1);
        }
    }

    fn list_windows() {
        for w in win::visible_windows() {
            println!(
                "{}{}  {}x{}  başlık {} px  \"{}\"{}",
                if w.looks_like_remote_play() { "* " } else { "  " },
                w.exe,
                w.width(),
                w.height(),
                w.top_inset,
                w.title,
                if w.minimized { "  (simge durumunda)" } else { "" }
            );
        }
    }

    fn wait_enter(prompt: &str) -> String {
        print!("{prompt} ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        line.trim().to_lowercase()
    }

    fn full_test(args: &[String], interactive: bool) -> Result<(), String> {
        let options = Options {
            frames: option(args, "--kare").and_then(|v| v.parse().ok()).unwrap_or(8).max(1),
            interval: option(args, "--aralik").and_then(|v| v.parse().ok()).unwrap_or(1.0),
            window_filter: option(args, "--pencere").map(|v| v.to_lowercase()),
            scale: option(args, "--olcek").and_then(|v| v.parse().ok()).unwrap_or(1.0f64).clamp(0.25, 4.0),
            title_bar: option(args, "--baslik").and_then(|v| v.parse().ok()),
        };

        say!("winprobe {} — Faz 0: Remote Play yakalama + Windows OCR testi", env!("CARGO_PKG_VERSION"));
        say!("Sistem: {}", win::os_version());
        let supported = capture::is_supported();
        say!("Windows.Graphics.Capture destekleniyor: {}", if supported { "evet" } else { "HAYIR" });
        if !supported {
            return Err("Bu Windows sürümü pencere yakalamayı desteklemiyor (Windows 10 1903+ gerekir).".into());
        }

        let ocr_setup = ocr::setup().map_err(|e| format!("OCR başlatılamadı: {e}"))?;
        report_ocr_setup(&ocr_setup);

        let out_dir = new_output_dir()?;
        say!("Çıktı klasörü: {}", out_dir.display());
        say!("");

        let mut round = 1;
        loop {
            let window = find_window(&options, interactive)?;
            run_round(&window, &options, &ocr_setup, &out_dir, round)?;
            save_report(&out_dir);
            if !interactive || round >= 3 {
                break;
            }
            say!("");
            let answer = wait_enter(
                "Başka bir modda (ör. Remote Play TAM EKRAN) tekrar denemek için modu değiştirip Enter'a bas, bitirmek için q:",
            );
            if answer == "q" {
                break;
            }
            round += 1;
        }

        say!("");
        say!("Bitti. Lütfen şu klasörü olduğu gibi gönder: {}", out_dir.display());
        save_report(&out_dir);
        if interactive {
            wait_enter("Kapatmak için Enter'a bas.");
        }
        Ok(())
    }

    fn report_ocr_setup(setup: &OcrSetup) {
        say!("Kurulu OCR dilleri: {}", if setup.installed.is_empty() { "(yok)".into() } else { setup.installed.join(", ") });
        say!("OCR azami görüntü boyutu: {} px", setup.max_dimension);
        match &setup.engine_language {
            Some(tag) => say!("İngilizce OCR motoru: {tag}"),
            None => {
                say!("UYARI: İngilizce OCR dili kurulu değil; OCR adımı atlanacak, sadece yakalama test edilecek.");
                say!("  Kurmak için: Ayarlar > Zaman ve dil > Dil ve bölge > Dil ekle > English (United States)");
                say!("  (ya da Yönetici PowerShell: Add-WindowsCapability -Online -Name \"Language.OCR~~~en-US~0.0.1.0\")");
            }
        }
    }

    fn new_output_dir() -> Result<PathBuf, String> {
        for n in 1..1000 {
            let dir = PathBuf::from(format!("winprobe-sonuc-{n}"));
            if !dir.exists() {
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                return Ok(std::fs::canonicalize(&dir).unwrap_or(dir));
            }
        }
        Err("Çıktı klasörü oluşturulamadı".into())
    }

    fn save_report(out_dir: &Path) {
        let report = REPORT.lock().unwrap().clone();
        // Not Defteri'nin Türkçe karakterleri doğru açması için BOM'lu UTF-8.
        let _ = std::fs::write(out_dir.join("rapor.txt"), format!("\u{feff}{}", report.replace('\n', "\r\n")));
    }

    fn find_window(options: &Options, interactive: bool) -> Result<WindowInfo, String> {
        loop {
            let windows = win::visible_windows();
            let found = windows.iter().find(|w| match &options.window_filter {
                Some(filter) => w.title.to_lowercase().contains(filter) || w.exe.to_lowercase().contains(filter),
                None => w.looks_like_remote_play(),
            });
            if let Some(window) = found {
                if !window.minimized {
                    return Ok(window.clone());
                }
                say!("Remote Play penceresi simge durumunda; simge durumundaki pencereler yakalanamaz.");
            } else {
                say!("Remote Play penceresi bulunamadı. Ekrandaki pencereler:");
                for w in &windows {
                    say!("  {}  {}x{}  \"{}\"", w.exe, w.width(), w.height(), w.title);
                }
            }
            if !interactive {
                return Err("Pencere bulunamadı (--pencere ile başka bir pencere seçilebilir).".into());
            }
            if wait_enter("PS Remote Play'i aç, PS5'e bağlan ve Enter'a bas (çıkmak için q):") == "q" {
                return Err("Kullanıcı çıktı.".into());
            }
        }
    }

    fn run_round(window: &WindowInfo, options: &Options, ocr_setup: &OcrSetup, out_dir: &Path, round: u32) -> Result<(), String> {
        say!("=== Tur {round} ===");
        say!(
            "Pencere: \"{}\" ({})  {}x{} px  başlık çubuğu {} px",
            window.title,
            window.exe,
            window.width(),
            window.height(),
            window.top_inset
        );

        let started = Instant::now();
        let mut capture = WindowCapture::start(window.hwnd).map_err(|e| format!("Yakalama başlatılamadı: {e}"))?;
        say!(
            "Yakalama oturumu {} ms'de başladı ({}).",
            started.elapsed().as_millis(),
            if capture.uses_software_renderer() { "donanım yok, yazılımsal D3D11/WARP" } else { "donanım D3D11" }
        );

        let mut blank = 0;
        let mut stale = 0;
        let mut ocr_times = Vec::new();
        let mut read_times = Vec::new();
        let mut line_counts = Vec::new();

        for index in 1..=options.frames {
            let requested = Instant::now();
            let Some((frame, fresh)) = capture
                .latest_frame(Duration::from_secs(3))
                .map_err(|e| format!("Kare okunamadı: {e}"))?
            else {
                say!("#{index}: 3 sn içinde hiç kare gelmedi.");
                blank += 1;
                continue;
            };
            let read_ms = requested.elapsed().as_secs_f64() * 1000.0;
            read_times.push(read_ms);
            if !fresh {
                stale += 1;
            }

            let stats = FrameStats::analyze_rgba(&frame.rgba, frame.width, frame.height);
            if stats.looks_blank() {
                blank += 1;
            }
            let image = RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).ok_or("Kare tampon boyutu hatalı")?;
            save_png(&image, &out_dir.join(format!("t{round}_pencere_{index:02}.png")))?;

            // Pencere kenarlığı yakalamaya dahil değil; başlık çubuğu ise dahil.
            let scale_y = frame.height as f64 / window.height().max(1) as f64;
            let top_inset = options.title_bar.unwrap_or(window.top_inset) as f64 * scale_y;
            let video = video_rect(frame.width as f64, frame.height as f64, top_inset);
            let video_image = crop(&image, video);
            save_png(&video_image, &out_dir.join(format!("t{round}_video_{index:02}.png")))?;

            let mut line = format!(
                "#{index}: kare {}x{}{}  okuma {:.0} ms  ort. parlaklık {:.0}  siyah olmayan %{:.1}{}  video {}x{}",
                frame.width,
                frame.height,
                if fresh { "" } else { " (yeni kare yok, öncekisi)" },
                read_ms,
                stats.mean_luma,
                stats.non_dark_fraction * 100.0,
                if stats.looks_blank() { "  SİYAH!" } else { "" },
                video_image.width(),
                video_image.height()
            );

            if let Some(engine) = &ocr_setup.engine {
                let run = run_ocr(engine, &video_image, options.scale, ocr_setup.max_dimension)?;
                ocr_times.push(run.ms);
                line_counts.push(run.lines.len());
                let _ = write!(line, "  OCR {:.0} ms ({}x{}), {} satır", run.ms, run.size.0, run.size.1, run.lines.len());
                say!("{line}");
                print_lines(&run.lines);
                let boxes: Vec<_> = run.lines.iter().map(|l| l.bbox).collect();
                save_png(&draw_boxes(&video_image, &boxes), &out_dir.join(format!("t{round}_ocr_{index:02}.png")))?;
            } else {
                say!("{line}");
            }

            if index < options.frames {
                std::thread::sleep(Duration::from_secs_f64(options.interval));
            }
        }
        drop(capture);

        say!("--- Tur {round} özeti ---");
        let total = options.frames as usize;
        if blank == total {
            say!("YAKALAMA: BAŞARISIZ — tüm kareler siyah/boş. Remote Play içeriği korumalı olarak yakalanıyor olabilir.");
        } else {
            say!("YAKALAMA: TAMAM — {}/{} karede görüntü var.", total - blank, total);
        }
        if stale > 0 {
            say!("  {stale} turda yeni kare gelmedi (görüntü değişmiyorsa normal).");
        }
        if !read_times.is_empty() {
            say!("  Ortalama kare okuma: {:.0} ms", average(&read_times));
        }
        if !ocr_times.is_empty() {
            say!(
                "OCR: ortalama {:.0} ms, kare başına ortalama {:.1} satır (ilk çağrı: {:.0} ms)",
                average(&ocr_times),
                line_counts.iter().sum::<usize>() as f64 / line_counts.len() as f64,
                ocr_times[0]
            );
        }
        Ok(())
    }

    struct OcrRun {
        lines: Vec<TextLine>,
        ms: f64,
        /// OCR'a verilen (ölçeklenmiş) görüntünün boyutu.
        size: (u32, u32),
    }

    /// Video alanını OCR'a hazırlar (ölçek + azami boyut) ve tanır. Kutular video alanına göre normalize.
    fn run_ocr(engine: &OcrEngine, video: &RgbaImage, scale: f64, max_dimension: u32) -> Result<OcrRun, String> {
        let prepared = prepare_for_ocr(video, scale, max_dimension);
        let started = Instant::now();
        let lines = ocr::recognize(engine, &prepared).map_err(|e| format!("OCR hatası: {e}"))?;
        Ok(OcrRun { lines, ms: started.elapsed().as_secs_f64() * 1000.0, size: prepared.dimensions() })
    }

    fn print_lines(lines: &[TextLine]) {
        for line in lines {
            say!(
                "    [x={:.3} y={:.3} w={:.3} h={:.3}] \"{}\"",
                line.bbox.x,
                line.bbox.y,
                line.bbox.width,
                line.bbox.height,
                line.text
            );
        }
    }

    fn ocr_files(paths: &[String]) -> Result<(), String> {
        if paths.is_empty() {
            return Err("Kullanım: winprobe ocr DOSYA.png ...".into());
        }
        let setup = ocr::setup().map_err(|e| format!("OCR başlatılamadı: {e}"))?;
        report_ocr_setup(&setup);
        let engine = setup.engine.as_ref().ok_or("İngilizce OCR dili kurulu değil.")?;
        for path in paths {
            let image = image::open(path).map_err(|e| format!("{path}: {e}"))?.to_rgba8();
            let run = run_ocr(engine, &image, 1.0, setup.max_dimension)?;
            say!("{path}: {}x{}  OCR {:.0} ms, {} satır", run.size.0, run.size.1, run.ms, run.lines.len());
            print_lines(&run.lines);
        }
        Ok(())
    }

    fn save_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
        image.save(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn average(values: &[f64]) -> f64 {
        values.iter().sum::<f64>() / values.len() as f64
    }
}
