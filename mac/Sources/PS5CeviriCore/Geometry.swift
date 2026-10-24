/// Basit dikdörtgen. Bağlama göre piksel ya da 0...1 normalize değer tutar.
public struct Rect: Equatable, Sendable {
    public var x: Double
    public var y: Double
    public var width: Double
    public var height: Double

    public init(x: Double, y: Double, width: Double, height: Double) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }
}

public enum Geometry {
    /// PS5 görüntüsünün en-boy oranı.
    public static let videoAspect = 16.0 / 9.0

    /// Remote Play penceresinin yakalanan görüntüsünde oyun videosunun kapladığı alan (piksel).
    /// Üstteki başlık çubuğu (`topInset`) çıkarılır, kalan alana 16:9 video ortalanarak sığdırılır
    /// (tam ekranda 21:9 monitördeki yan siyah bantlar böylece dışarıda kalır).
    public static func videoRect(imageWidth: Double, imageHeight: Double, topInset: Double) -> Rect {
        let availableHeight = max(imageHeight - topInset, 0)
        var width = imageWidth
        var height = width / videoAspect
        if height > availableHeight {
            height = availableHeight
            width = height * videoAspect
        }
        return Rect(
            x: (imageWidth - width) / 2,
            y: topInset + (availableHeight - height) / 2,
            width: width,
            height: height
        )
    }

    /// Vision kutusu (0...1, origin SOL ALT) → protokol kutusu (0...1, origin SOL ÜST).
    public static func topLeftBox(fromVision box: Rect) -> Rect {
        Rect(x: box.x, y: 1 - box.y - box.height, width: box.width, height: box.height)
    }
}
