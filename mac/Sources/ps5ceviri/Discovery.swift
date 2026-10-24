import Foundation
import Network
import Synchronization

/// TV'deki "PS5 Ceviri" servisini (_ps5ceviri._tcp) yerel ağda bulur ve ws:// adresini döner.
enum Discovery {
    private static let queue = DispatchQueue(label: "ps5ceviri.discovery")

    static func findTV(timeout: Duration = .seconds(5)) async -> URL? {
        let browser = NWBrowser(for: .bonjour(type: "_ps5ceviri._tcp", domain: nil), using: .tcp)
        let endpoint: NWEndpoint? = await withCheckedContinuation { continuation in
            let once = ResumeOnce(continuation) { browser.cancel() }
            browser.browseResultsChangedHandler = { results, _ in
                if let first = results.first { once.resume(first.endpoint) }
            }
            browser.stateUpdateHandler = { state in
                if case .failed = state { once.resume(nil) }
            }
            browser.start(queue: queue)
            queue.asyncAfter(deadline: .now() + Double(timeout.components.seconds)) { once.resume(nil) }
        }
        guard let endpoint else { return nil }
        return await resolve(endpoint)
    }

    /// Servis adını IP:port'a çevirmek için kısa bir TCP bağlantısı açılır. IPv4'e zorlanır:
    /// IPv6 link-local adres (fe80::…%en0) zone olmadan URL'de kullanılamaz.
    private static func resolve(_ endpoint: NWEndpoint) async -> URL? {
        let parameters = NWParameters.tcp
        (parameters.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options)?.version = .v4
        let connection = NWConnection(to: endpoint, using: parameters)
        return await withCheckedContinuation { continuation in
            let once = ResumeOnce(continuation) { connection.cancel() }
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    guard case .hostPort(let host, let port) = connection.currentPath?.remoteEndpoint else {
                        once.resume(nil)
                        return
                    }
                    var hostText = "\(host)"
                    if let percent = hostText.firstIndex(of: "%") { hostText = String(hostText[..<percent]) }
                    if hostText.contains(":") { hostText = "[\(hostText)]" }
                    once.resume(URL(string: "ws://\(hostText):\(port.rawValue)"))
                case .failed, .cancelled:
                    once.resume(nil)
                default:
                    break
                }
            }
            connection.start(queue: queue)
            queue.asyncAfter(deadline: .now() + 5) { once.resume(nil) }
        }
    }
}

/// Continuation'ı yalnızca bir kez sürdürür ve ardından kaynakları kapatır.
private final class ResumeOnce<Value: Sendable>: Sendable {
    private let continuation: Mutex<CheckedContinuation<Value, Never>?>
    private let cleanup: @Sendable () -> Void

    init(_ continuation: CheckedContinuation<Value, Never>, cleanup: @escaping @Sendable () -> Void) {
        self.continuation = Mutex(continuation)
        self.cleanup = cleanup
    }

    func resume(_ value: Value) {
        let pending = continuation.withLock { current in
            defer { current = nil }
            return current
        }
        guard let pending else { return }
        cleanup()
        pending.resume(returning: value)
    }
}
