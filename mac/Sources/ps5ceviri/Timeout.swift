import Foundation
import Synchronization

struct TimeoutError: Error, CustomStringConvertible {
    let seconds: Double
    var description: String { "\(seconds) sn içinde yanıt gelmedi" }
}

/// İşlemi süre sınırıyla çalıştırır. Task group kullanılmaz: ScreenCaptureKit, Remote Play bağlantısı
/// koptuğunda continuation'ı hiç sürdürmeyebiliyor ve group o çocuğu sonsuza kadar bekler. Burada
/// takılan iş geride bırakılır, çağıran zaman aşımı hatasıyla devam eder.
func withTimeout<T: Sendable>(seconds: Double, _ operation: @escaping @Sendable () async throws -> T) async throws -> T {
    try await withCheckedThrowingContinuation { continuation in
        let once = ResumeOnceThrowing(continuation)
        Task {
            do { once.resume(.success(try await operation())) } catch { once.resume(.failure(error)) }
        }
        Task {
            try? await Task.sleep(for: .seconds(seconds))
            once.resume(.failure(TimeoutError(seconds: seconds)))
        }
    }
}

private final class ResumeOnceThrowing<Value: Sendable>: Sendable {
    private let continuation: Mutex<CheckedContinuation<Value, Error>?>

    init(_ continuation: CheckedContinuation<Value, Error>) {
        self.continuation = Mutex(continuation)
    }

    func resume(_ result: Result<Value, Error>) {
        let pending = continuation.withLock { current in
            defer { current = nil }
            return current
        }
        pending?.resume(with: result)
    }
}
