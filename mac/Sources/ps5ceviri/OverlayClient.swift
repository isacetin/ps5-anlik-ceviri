import Foundation
import PS5CeviriCore

/// TV'deki WebSocket server'a blok listesi gönderir. Bağlantı koparsa hemen yeniden bağlanıp bir kez
/// daha dener; yeni bağlantıda seq sıfırdan başlar (TV de sayacı bağlantıyla sıfırlar).
@MainActor
final class OverlayClient {
    private let url: URL
    private var task: URLSessionWebSocketTask?
    private var seq: Int64 = 0

    init(url: URL) {
        self.url = url
    }

    /// Gönderim başarılıysa true. Liste değişmese de çağrılmalı: TV 3 sn mesaj almazsa temizler.
    @discardableResult
    func send(_ blocks: [OverlayBlock]) async -> Bool {
        for attempt in 1...2 {
            if task == nil { connect() }
            guard let task else { return false }
            seq += 1
            do {
                let json = try OverlayMessage(seq: seq, blocks: blocks).json()
                try await task.send(.string(json))
                return true
            } catch {
                task.cancel(with: .goingAway, reason: nil)
                self.task = nil
                if attempt == 2 {
                    FileHandle.standardError.write(Data("WebSocket gönderilemedi (\(url.absoluteString)): \(error.localizedDescription)\n".utf8))
                }
            }
        }
        return false
    }

    func close() async {
        await send([])
        task?.cancel(with: .normalClosure, reason: nil)
        task = nil
    }

    private func connect() {
        let newTask = URLSession.shared.webSocketTask(with: url)
        newTask.resume()
        task = newTask
        seq = 0
        listen(newTask)
    }

    /// TV mesaj göndermez, ama bekleyen bir receive olmadan URLSession server'ın ping'lerine
    /// cevap vermez ve TV (Ktor, 30 sn) bağlantıyı keser. Bağlantı kapanınca receive hata döner;
    /// bir sonraki gönderim yeniden bağlanır.
    private nonisolated func listen(_ task: URLSessionWebSocketTask) {
        task.receive { [weak self] result in
            guard case .success = result else { return }
            self?.listen(task)
        }
    }
}
