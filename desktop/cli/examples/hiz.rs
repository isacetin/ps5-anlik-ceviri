//! Yerel model ile DeepL'in cümle başına çeviri süresini karşılaştırır (ısınmış halde, iş
//! hattındaki gibi tek tek cümle). DeepL ~500 karakter kota harcar.
//! cargo run --release -p ceviri-cli --example hiz [-- --yalniz-yerel]
use std::time::Instant;

use ceviri_core::storage;
use ceviri_engine::DeepLClient;
use ceviri_opus::OpusTranslator;

const LINES: &[&str] = &[
    "Continue",
    "New Game",
    "The door is locked.",
    "Press and hold to interact.",
    "You shouldn't be here.",
    "I think we should split up and look for the key.",
    "Find a way to restore power to the control room.",
    "Do you really think they are going to let us walk out of here alive?",
    "Listen to me. Whatever happens tonight, you have to promise me you won't go back for him.",
    "The old lighthouse keeper told me the storm would pass by morning, but I've never seen the sea this angry before.",
];

fn measure(name: &str, mut translate: impl FnMut(&str) -> Result<String, String>, repeats: usize) -> Result<(), String> {
    let cold = Instant::now();
    translate("Hello there.")?;
    println!("\n== {name} ==\nilk çağrı (soğuk): {} ms", cold.elapsed().as_millis());
    let mut all = Vec::new();
    for line in LINES {
        let mut times = Vec::new();
        let mut output = String::new();
        for _ in 0..repeats {
            let started = Instant::now();
            output = translate(line)?;
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.total_cmp(b));
        let median = times[times.len() / 2];
        all.push(median);
        println!("{median:6.0} ms  {:3} kr  {line}\n                   → {output}", line.chars().count());
    }
    all.sort_by(|a, b| a.total_cmp(b));
    println!("ortanca {:.0} ms, en kısa {:.0} ms, en uzun {:.0} ms", all[all.len() / 2], all[0], all[all.len() - 1]);
    Ok(())
}

fn main() -> Result<(), String> {
    let only_local = std::env::args().any(|a| a == "--yalniz-yerel");

    let loading = Instant::now();
    let opus = OpusTranslator::load(&storage::local_model_dir().ok_or("uygulama klasörü yok")?)?;
    println!("yerel model yüklendi: {} ms", loading.elapsed().as_millis());
    measure("Yerel model (Opus-MT)", |text| opus.translate(&[text.to_string()]).map(|mut t| t.remove(0)).map_err(|e| e.to_string()), 5)?;

    if !only_local {
        let deepl = DeepLClient::new(storage::deepl_key().ok_or("DeepL anahtarı yok")?);
        measure("DeepL", |text| deepl.translate(&[text.to_string()]).map(|mut t| t.remove(0)).map_err(|e| e.to_string()), 1)?;
    }
    Ok(())
}
