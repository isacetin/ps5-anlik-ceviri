//! TV olmadan geliştirme için sahte TV: gelen mesajları yazar ve gerçek TV'deki Ktor gibi düzenli
//! ping atıp pong bekler (pong gelmezse istemci TV'de ~30 sn'de bir bağlantıyı koparırdı).
//!
//! cargo run -p ceviri-engine --example fake_tv -- 18765
//! cargo run -p ceviri-cli -- run --dry-run --tv 127.0.0.1:18765

use std::io::ErrorKind;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use ceviri_core::OverlayMessage;
use tungstenite::Message;

const PING_EVERY: Duration = Duration::from_secs(2);

fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "8765".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).expect("port açılamadı");
    println!("Sahte TV ws://127.0.0.1:{port} adresinde dinliyor");

    for stream in listener.incoming().flatten() {
        let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
        stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let Ok(mut socket) = tungstenite::accept(stream) else { continue };
        println!("[bağlandı] {peer}");
        let (mut pings, mut pongs) = (0u32, 0u32);
        let mut last_ping = Instant::now();
        loop {
            if last_ping.elapsed() >= PING_EVERY {
                pings += 1;
                if socket.send(Message::Ping(Vec::new().into())).is_err() {
                    break;
                }
                last_ping = Instant::now();
            }
            match socket.read() {
                Ok(Message::Text(text)) => match serde_json::from_str::<OverlayMessage>(&text) {
                    Ok(message) => {
                        let texts: Vec<&str> = message.blocks.iter().map(|b| b.text.as_str()).collect();
                        println!("seq {:>3}  {} blok  {}", message.seq, texts.len(), texts.join("  |  "));
                    }
                    Err(error) => println!("[bozuk JSON] {error}"),
                },
                Ok(Message::Pong(_)) => pongs += 1,
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
        }
        println!("[koptu] {peer}  ping {pings}, pong {pongs}");
    }
}
