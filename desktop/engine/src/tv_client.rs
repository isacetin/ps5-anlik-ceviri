//! TV'deki WebSocket server'a blok listesi gönderir (Swift `OverlayClient` karşılığı).

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use ceviri_core::{OverlayBlock, OverlayMessage};
use tungstenite::{Message, WebSocket};

/// TV uygulamasının dinlediği port.
pub const DEFAULT_PORT: u16 = 8765;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// "192.168.1.5" → "192.168.1.5:8765"; port verilmişse olduğu gibi.
pub fn with_default_port(host: &str) -> String {
    let has_port = match host.rsplit_once(':') {
        Some((name, port)) => !name.is_empty() && !name.ends_with(':') && port.parse::<u16>().is_ok(),
        None => false,
    };
    if has_port {
        host.to_string()
    } else {
        format!("{host}:{DEFAULT_PORT}")
    }
}

/// Bağlantı koparsa hemen yeniden bağlanıp bir kez daha dener; yeni bağlantıda `seq` sıfırdan
/// başlar (TV de sayacı bağlantıyla sıfırlar).
pub struct TvClient {
    address: String,
    socket: Option<WebSocket<TcpStream>>,
    seq: i64,
}

impl TvClient {
    /// `address`: "host:port".
    pub fn new(address: impl Into<String>) -> TvClient {
        TvClient { address: address.into(), socket: None, seq: 0 }
    }

    pub fn url(&self) -> String {
        format!("ws://{}", self.address)
    }

    /// Liste değişmese de her turda çağrılmalı: TV 3 sn mesaj almazsa yazıları temizler.
    pub fn send(&mut self, blocks: &[OverlayBlock]) -> Result<(), String> {
        let mut last_error = String::new();
        for _attempt in 0..2 {
            if self.socket.is_none() {
                self.socket = Some(self.connect()?);
                self.seq = 0;
            }
            self.seq += 1;
            let json = OverlayMessage::new(self.seq, blocks.to_vec()).to_json();
            let socket = self.socket.as_mut().expect("bağlantı az önce kuruldu");
            match socket.send(Message::text(json)) {
                Ok(()) => {
                    self.drain_incoming();
                    return Ok(());
                }
                Err(error) => {
                    last_error = error.to_string();
                    self.socket = None;
                }
            }
        }
        Err(format!("WebSocket gönderilemedi ({}): {last_error}", self.url()))
    }

    /// Ekranı temizler ve bağlantıyı kapatır.
    pub fn close(&mut self) {
        let _ = self.send(&[]);
        if let Some(mut socket) = self.socket.take() {
            let _ = socket.close(None);
            let _ = socket.flush();
        }
    }

    fn connect(&self) -> Result<WebSocket<TcpStream>, String> {
        let fail = |error: &dyn std::fmt::Display| format!("TV'ye bağlanılamadı ({}): {error}", self.url());
        let address = self.address.to_socket_addrs().map_err(|e| fail(&e))?.next().ok_or_else(|| fail(&"adres çözülemedi"))?;
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|e| fail(&e))?;
        let _ = stream.set_nodelay(true);
        // El sıkışma takılırsa sonsuza kadar beklenmesin.
        stream.set_read_timeout(Some(CONNECT_TIMEOUT)).map_err(|e| fail(&e))?;
        stream.set_write_timeout(Some(CONNECT_TIMEOUT)).map_err(|e| fail(&e))?;
        let (socket, _) = tungstenite::client(format!("ws://{}/", self.address), stream).map_err(|e| fail(&e))?;
        // Sonrasında okuma yalnızca bekleyen ping'leri boşaltmak için yapılır; beklememeli.
        socket.get_ref().set_read_timeout(Some(Duration::from_millis(1))).map_err(|e| fail(&e))?;
        Ok(socket)
    }

    /// TV mesaj göndermez, ama ping'leri okunmazsa pong gitmez ve TV (Ktor, 30 sn) bağlantıyı
    /// keser. tungstenite pong'u okuma sırasında sıraya koyar; `flush` ile hemen gönderilir.
    fn drain_incoming(&mut self) {
        let Some(socket) = self.socket.as_mut() else { return };
        loop {
            match socket.read() {
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
                Err(_) => {
                    // Kapanmış bağlantı: bir sonraki gönderim yeniden bağlanır.
                    self.socket = None;
                    return;
                }
            }
        }
        let _ = socket.flush();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    /// Her bağlantıda en fazla `per_connection` mesaj okuyup kapatan sahte TV.
    pub(crate) fn fake_tv(per_connection: usize) -> (String, mpsc::Receiver<OverlayMessage>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let Ok(mut socket) = tungstenite::accept(stream) else { continue };
                for _ in 0..per_connection {
                    match socket.read() {
                        Ok(Message::Text(text)) => {
                            let message: OverlayMessage = serde_json::from_str(&text).unwrap();
                            if sender.send(message).is_err() {
                                return;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                let _ = socket.close(None);
                let _ = socket.flush();
            }
        });
        (address, receiver)
    }

    fn block(text: &str) -> OverlayBlock {
        OverlayBlock { id: "b1".into(), x: 0.1, y: 0.2, w: 0.3, h: 0.04, line_h: 0.03, text: text.into() }
    }

    #[test]
    fn varsayilan_port_eklenir() {
        assert_eq!(with_default_port("192.168.1.5"), "192.168.1.5:8765");
        assert_eq!(with_default_port("192.168.1.5:9000"), "192.168.1.5:9000");
        assert_eq!(with_default_port("tv.local"), "tv.local:8765");
    }

    #[test]
    fn mesajlar_artan_seq_ile_gider() {
        let (address, messages) = fake_tv(usize::MAX);
        let mut client = TvClient::new(address);

        client.send(&[block("Kapı kilitli.")]).unwrap();
        client.send(&[]).unwrap();

        let first = messages.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = messages.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!((first.seq, first.v), (1, 1));
        assert_eq!(first.blocks[0].text, "Kapı kilitli.");
        assert_eq!(second.seq, 2);
        assert!(second.blocks.is_empty());
    }

    #[test]
    fn baglanti_koparsa_yeniden_baglanir_ve_seq_sifirlanir() {
        // TV her bağlantıda tek mesaj okuyup kapatıyor.
        let (address, messages) = fake_tv(1);
        let mut client = TvClient::new(address);

        client.send(&[block("bir")]).unwrap();
        assert_eq!(messages.recv_timeout(Duration::from_secs(2)).unwrap().seq, 1);
        thread::sleep(Duration::from_millis(100));

        // Kapanan bağlantı ya bu gönderimde ya bir sonrakinde fark edilir; ikisinde de mesaj ulaşmalı.
        for _ in 0..3 {
            let _ = client.send(&[block("iki")]);
            if let Ok(message) = messages.recv_timeout(Duration::from_millis(500)) {
                assert_eq!(message.seq, 1);
                assert_eq!(message.blocks[0].text, "iki");
                return;
            }
        }
        panic!("yeniden bağlanıldıktan sonra mesaj gelmedi");
    }

    #[test]
    fn kapali_porta_baglanma_hata_doner() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut client = TvClient::new(format!("127.0.0.1:{port}"));

        let error = client.send(&[]).unwrap_err();

        assert!(error.contains("TV'ye bağlanılamadı"), "{error}");
    }
}
