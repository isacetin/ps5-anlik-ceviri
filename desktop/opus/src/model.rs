//! Yerel çeviri modelinin indirilmesi. Model (Opus-MT en→tr, CTranslate2 int8'e çevrilmiş) bu
//! deponun GitHub Releases sayfasında durur; dosyalar tek tek iner ve SHA-256 ile doğrulanır.
//! Yarım kalan dosya `.part` adıyla kalır ve bir sonraki denemede baştan indirilir.
//!
//! Model: Helsinki-NLP/opus-mt-tc-big-en-tr, CC-BY 4.0 (Tiedemann & Thottingal, OPUS-MT).

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

pub const BASE_URL: &str = "https://github.com/isacetin/ps5-anlik-ceviri/releases/download/model-opus-mt-en-tr-int8-v1";

pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub const FILES: [ModelFile; 5] = [
    ModelFile { name: "config.json", size: 223, sha256: "8f6496adfc930cbfecbe8281112197705c488fab47d34b4829b06d7f478909af" },
    ModelFile { name: "model.bin", size: 237_977_893, sha256: "a5a148aae553f2573fd1f9ba735a03385f050437f98e51f8b5c4e4f9c4de3719" },
    ModelFile { name: "shared_vocabulary.json", size: 1_112_966, sha256: "19bb107d0d72fde62e5c28378da7eb1bb379a2203b9957890618d3df18ba2bb3" },
    ModelFile { name: "source.spm", size: 796_953, sha256: "180d3f94ddfee5585d78b702290ded0f4f9bd40f299349c8383b32a579fefad4" },
    ModelFile { name: "target.spm", size: 833_391, sha256: "cd0c475692178b6c626aadaef01bd8f98d8053946938e4e67005fc1e4811d797" },
];

pub fn total_size() -> u64 {
    FILES.iter().map(|f| f.size).sum()
}

/// Bütün dosyalar yerinde ve boyutları doğru mu (açılışta hızlı kontrol; özet indirirken doğrulanır).
pub fn is_installed(dir: &Path) -> bool {
    files_present(dir, &FILES)
}

/// Eksik dosyaları indirir; `progress(inen, toplam)` bayt cinsinden.
pub fn download(dir: &Path, progress: impl FnMut(u64, u64)) -> Result<(), String> {
    download_files(BASE_URL, dir, &FILES, progress)
}

fn files_present(dir: &Path, files: &[ModelFile]) -> bool {
    files.iter().all(|f| fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.size))
}

fn download_files(base_url: &str, dir: &Path, files: &[ModelFile], mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let total: u64 = files.iter().map(|f| f.size).sum();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        // Bağlantı kopmadan takılırsa: 30 sn hiç veri gelmezse vazgeç.
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut done = 0;
    progress(done, total);
    for file in files {
        let path = dir.join(file.name);
        if fs::metadata(&path).is_ok_and(|m| m.len() == file.size) && sha256_file(&path).as_deref() == Some(file.sha256) {
            done += file.size;
            progress(done, total);
            continue;
        }
        let part = dir.join(format!("{}.part", file.name));
        let url = format!("{base_url}/{}", file.name);
        let result = fetch(&agent, &url, &part, |n| {
            done += n;
            progress(done.min(total), total);
        });
        let digest = match result {
            Ok(digest) => digest,
            Err(error) => {
                let _ = fs::remove_file(&part);
                return Err(error);
            }
        };
        if digest != file.sha256 {
            let _ = fs::remove_file(&part);
            return Err(format!("{} bozuk indi (özet uyuşmuyor); tekrar deneyin.", file.name));
        }
        fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Dosyayı indirip yazar; SHA-256 özetini (hex) döner.
fn fetch(agent: &ureq::Agent, url: &str, path: &Path, mut on_bytes: impl FnMut(u64)) -> Result<String, String> {
    let response = agent.get(url).call().map_err(|e| format!("Model indirilemedi ({url}): {e}"))?;
    let mut reader = response.into_body().into_reader();
    let mut out = fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| format!("Model indirilirken bağlantı koptu: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        out.write_all(&buffer[..n]).map_err(|e| format!("{}: {e}", path.display()))?;
        on_bytes(n as u64);
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Dosya adına göre içerik veren sahte sunucu; istenen yolları kaydeder.
    fn fake_server(files: &'static [(&'static str, &'static [u8])]) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/model", listener.local_addr().unwrap());
        let requested = Arc::new(Mutex::new(Vec::new()));
        let log = requested.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line.trim().is_empty() {
                        break;
                    }
                }
                let path = request_line.split_whitespace().nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(path.clone());
                let mut stream = stream;
                match files.iter().find(|(name, _)| path.ends_with(&format!("/{name}"))) {
                    Some((_, body)) => {
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                        stream.write_all(body).unwrap();
                    }
                    None => write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap(),
                }
            }
        });
        (base, requested)
    }

    fn model_file(name: &'static str, body: &[u8]) -> ModelFile {
        let digest: &'static str = Box::leak(hex(&Sha256::digest(body)).into_boxed_str());
        ModelFile { name, size: body.len() as u64, sha256: digest }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ceviri-opus-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn dosyalar_iner_dogrulanir_ve_ikinci_kez_indirilmez() {
        static BODIES: [(&str, &[u8]); 2] = [("a.bin", b"birinci dosya"), ("b.json", b"{\"ikinci\": true}")];
        let (base, requested) = fake_server(&BODIES);
        let files = [model_file("a.bin", BODIES[0].1), model_file("b.json", BODIES[1].1)];
        let dir = temp_dir("ok");
        let mut seen = Vec::new();

        download_files(&base, &dir, &files, |done, total| seen.push((done, total))).unwrap();

        assert!(files_present(&dir, &files));
        assert_eq!(fs::read(dir.join("a.bin")).unwrap(), BODIES[0].1);
        assert_eq!(seen.first(), Some(&(0, 29)));
        assert_eq!(seen.last(), Some(&(29, 29)));
        assert_eq!(requested.lock().unwrap().len(), 2);

        // Yerinde ve özeti doğru olan dosya tekrar istenmez.
        download_files(&base, &dir, &files, |_, _| {}).unwrap();
        assert_eq!(requested.lock().unwrap().len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ozeti_tutmayan_dosya_reddedilir_ve_kalmaz() {
        static BODIES: [(&str, &[u8]); 1] = [("a.bin", b"degistirilmis")];
        let (base, _) = fake_server(&BODIES);
        let files = [ModelFile { name: "a.bin", size: 13, sha256: "00" }];
        let dir = temp_dir("bad");

        let error = download_files(&base, &dir, &files, |_, _| {}).unwrap_err();

        assert!(error.contains("bozuk"), "{error}");
        assert!(!dir.join("a.bin").exists());
        assert!(!dir.join("a.bin.part").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sunucuda_olmayan_dosya_hata_verir() {
        static BODIES: [(&str, &[u8]); 0] = [];
        let (base, _) = fake_server(&BODIES);
        let dir = temp_dir("missing");

        let error = download_files(&base, &dir, &[ModelFile { name: "yok.bin", size: 1, sha256: "00" }], |_, _| {}).unwrap_err();

        assert!(error.contains("indirilemedi"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gercek_model_listesi_tutarli() {
        assert_eq!(total_size(), 240_721_426);
        assert!(FILES.iter().all(|f| f.sha256.len() == 64));
        assert!(BASE_URL.starts_with("https://"));
    }
}
