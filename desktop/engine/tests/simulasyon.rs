//! Gerçek ekran olmadan oyun oturumu simülasyonu: daktilo/duraklamalı altyazı, büyük harfle
//! yeniden görünen replik, geri sayım, iki satırlı altyazı, menü, hızlı diyalog. Çeviri arka ucuna
//! giden metin sayısını ölçer (önce/sonra karşılaştırması için `cargo test -p ceviri-engine
//! --test simulasyon -- --nocapture`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ceviri_core::{FrameInfo, ReadError, Reading, Rect, ScreenReader, TextLine};
use ceviri_engine::pipeline::Backend;
use ceviri_engine::{Config, Event, Pipeline};

const FPS: f64 = 20.0;

fn line(text: &str, x: f64, y: f64, w: f64) -> TextLine {
    TextLine { text: text.into(), bbox: Rect::new(x, y, w, 0.04), confidence: 0.92 }
}

fn subtitle(text: &str) -> Vec<TextLine> {
    vec![line(text, 0.5 - text.len() as f64 * 0.004, 0.85, text.len() as f64 * 0.008)]
}

/// Tur numarasından ekrandaki satırlar (20 tur = 1 sn).
fn scene(round: u64) -> Vec<TextLine> {
    let typed = [
        "The", "The ancient", "The ancient kingdom,", "The ancient kingdom,", "The ancient kingdom,", "The ancient kingdom,", "The ancient kingdom,",
        "The ancient kingdom, they", "The ancient kingdom, they say,", "The ancient kingdom, they say,", "The ancient kingdom, they say,",
        "The ancient kingdom, they say,", "The ancient kingdom, they say,", "The ancient kingdom, they say, was destroyed",
    ];
    match round {
        // 0–2 sn: duraklamalı daktilo (her 2 turda bir adım), sonra tam cümle.
        0..=27 => subtitle(typed[(round / 2) as usize]),
        28..=60 => subtitle("The ancient kingdom, they say, was destroyed by war."),
        // 3–5 sn: menü.
        61..=110 => vec![line("Resume", 0.1, 0.4, 0.08), line("Options", 0.1, 0.46, 0.08), line("Inventory", 0.1, 0.52, 0.1), line("Quit Game", 0.1, 0.58, 0.1)],
        // 5,5–7 sn: iki satırlı altyazı.
        111..=140 => vec![line("I thought you would", 0.4, 0.85, 0.2), line("never come back.", 0.41, 0.895, 0.18)],
        // 7–9 sn: geri sayım.
        141..=180 => subtitle(&format!("Respawning in {} seconds", 9 - (round - 141) / 10)),
        // 9–10 sn: aynı replik, büyük harf ve noktasız.
        181..=200 => subtitle("THE ANCIENT KINGDOM, THEY SAY, WAS DESTROYED BY WAR"),
        // 10–12 sn: hızlı diyalog (her 0,25 sn yeni replik).
        _ => subtitle(["Run!", "Get down!", "Behind you, watch out!", "Over there, near the gate.", "Move, move!", "We can make it.", "Hold on.", "Almost there."][((round - 201) / 5).min(7) as usize]),
    }
}

struct Script {
    round: u64,
}

impl ScreenReader for Script {
    fn read(&mut self) -> Result<Reading, ReadError> {
        let lines = scene(self.round);
        self.round += 1;
        Ok(Reading { lines, video_width: 1920, video_height: 1080, fullscreen: true, capture_ms: 1, ocr_ms: 1, screen: None, frame: FrameInfo { seq: Some(self.round), age_ms: Some(16), restarts: 0 } })
    }
}

#[test]
fn oyun_oturumu_api_cagri_sayisi() {
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = asked.clone();
    let backend: Backend = Arc::new(move |texts: &[String], _context: &str| {
        log.lock().unwrap().extend(texts.iter().cloned());
        thread::sleep(Duration::from_millis(80));
        Ok(texts.iter().map(|t| format!("TR({t})")).collect())
    });
    let config = Config { backend: Some(backend), fps: FPS, ..Config::default() };
    let mut pipeline = Pipeline::new(Box::new(Script { round: 0 }), None, config);
    let stop = pipeline.stop_handle();
    let shown = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = shown.clone();
    // Senaryo tur sayısıyla ilerler (makine hızına bağlı); son replik gösterilince ya da en çok
    // 25 sn sonra durulur.
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    let worker = thread::spawn(move || {
        pipeline.run(|event| {
            if let Event::Shown(texts) = event {
                if texts.iter().any(|t| t.contains("Almost there")) {
                    finished.store(true, Ordering::Relaxed);
                }
                sink.lock().unwrap().extend(texts);
            }
        })
    });
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(25) && !done.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
    }
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();

    let asked = asked.lock().unwrap();
    let shown = shown.lock().unwrap();
    eprintln!("SIM: modele giden metin sayısı = {}", asked.len());
    for text in asked.iter() {
        eprintln!("SIM:   {text}");
    }
    let partial = asked.iter().filter(|t| t.contains("ancient") && !t.to_lowercase().contains("war")).count();
    eprintln!("SIM: yarım altyazı çevirisi = {partial}");
    assert!(shown.iter().any(|t| t.contains("destroyed by war")), "tam cümle gösterilmeli");
    assert!(shown.iter().any(|t| t.contains("Almost there")), "son hızlı replik gösterilmeli");
}
