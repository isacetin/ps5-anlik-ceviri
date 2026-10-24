import PS5CeviriCore
import Testing

struct GeometryTests {
    @Test func varsayilanPencerede28ptBaslikCubuguCikarilir() {
        // Gerçek Remote Play penceresi: 960x568, altındaki 960x540 alan video.
        let rect = Geometry.videoRect(imageWidth: 960, imageHeight: 568, topInset: 28)

        #expect(rect == Rect(x: 0, y: 28, width: 960, height: 540))
    }

    @Test func ultraGenisTamEkrandaYanBantlarAtilir() {
        // 3440x1440 monitörde tam ekran: 2560x1440 video ortada.
        let rect = Geometry.videoRect(imageWidth: 3440, imageHeight: 1440, topInset: 0)

        #expect(rect == Rect(x: 440, y: 0, width: 2560, height: 1440))
    }

    @Test func uzunPenceredeUstVeAltBantlarAtilir() {
        let rect = Geometry.videoRect(imageWidth: 1600, imageHeight: 1228, topInset: 28)

        #expect(rect == Rect(x: 0, y: 178, width: 1600, height: 900))
    }

    @Test func visionKutusuSolUstOrijineCevrilir() {
        // Vision: sol alt orijin. Altta duran bir altyazı (y=0.1) üstten 0.82'de başlar.
        let box = Geometry.topLeftBox(fromVision: Rect(x: 0.2, y: 0.1, width: 0.6, height: 0.08))

        #expect(abs(box.x - 0.2) < 1e-9)
        #expect(abs(box.y - 0.82) < 1e-9)
        #expect(abs(box.width - 0.6) < 1e-9)
        #expect(abs(box.height - 0.08) < 1e-9)
    }
}
