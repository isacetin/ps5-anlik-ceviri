// Tarayıcıda arayüz önizlemesi: Tauri köprüsü yerine geçer ve "Başlat"a basılınca gerçek
// formatta örnek olaylar yayınlar. Uygulamaya dahil edilmez (ui/ dışında).
const handlers = [];
const status = { running: false, hasDeeplKey: true, settings: { output: "both", tvAddress: "", translate: true, fps: 2, window: "", protectedTerms: ["It Takes Two", "May", "Cody"] } };
window.__TAURI__ = {
  core: {
    invoke: async (cmd) => {
      if (cmd === "status") return status;
      if (cmd === "setup_status") return { platform: "macos", screenRecording: true, remotePlay: true, tvApkAvailable: true, hasDeeplKey: true, setupDone: status.settings.setupDone };
      if (cmd === "tv_scan") return new Promise((r) => setTimeout(() => r([{ ip: "192.168.1.5", appRunning: false }]), 200));
      if (cmd === "tv_install") {
        const labels = [
          "TV'ye bağlanılıyor… TV'de izin sorusu çıkarsa \"Her zaman izin ver\"i işaretleyip Tamam'a basın.",
          "TV'deki sürüm kontrol ediliyor…",
          "TV uygulaması yükleniyor (yaklaşık 20 saniye)…",
        ];
        labels.forEach((label, i) => setTimeout(() => handlers.forEach((h) => h({ payload: { label } })), 100 * (i + 1)));
        return new Promise((r) => setTimeout(r, new URLSearchParams(location.search).has("kuruluyor") ? 1e9 : 500));
      }
      if (cmd === "tv_test") return new Promise((r) => setTimeout(r, 300));
      if (cmd === "start") {
        status.running = true;
        const emit = (payload, ms) => setTimeout(() => handlers.forEach((h) => h({ payload })), ms);
        emit({ type: "state", state: "running", detail: "Türkçe çeviri" }, 300);
        emit({ type: "tv", connected: true, address: "ws://192.168.1.5:8765", error: null }, 400);
        emit({ type: "round", video: "1920x1080", captureMs: 18, ocrMs: 142, blocks: 5, sent: 4 }, 500);
        emit({ type: "shown", texts: ["Yerel Oyna", "Çevrimiçi Oyna", "Seçenekler", "Erişilebilirlik Seçenekleri"] }, 600);
      }
      return null;
    },
  },
  event: { listen: async (_name, handler) => handlers.push(handler) },
};

// preview.html?calisiyor: sayfa açılınca "Başlat"a basılmış hali gösterilir.
if (new URLSearchParams(location.search).has("calisiyor")) {
  addEventListener("load", () => setTimeout(() => document.getElementById("toggle").click(), 50));
}

// preview.html?sihirbaz&tikla=N: kurulum sihirbazı, "ileri" düğmesine N kez basılmış hali.
const query = new URLSearchParams(location.search);
status.settings.setupDone = !query.has("sihirbaz");
if (query.has("tikla")) {
  addEventListener("load", async () => {
    for (let i = 0; i < Number(query.get("tikla")); i++) {
      await new Promise((r) => setTimeout(r, 900));
      document.querySelector("#wizard-body .wizard-actions .primary")?.click();
    }
  });
}

// preview.html?ayarlar: Ayarlar ve DeepL panelleri açık gösterilir.
if (new URLSearchParams(location.search).has("ayarlar")) {
  addEventListener("load", () => document.querySelectorAll("details").forEach((d) => (d.open = true)));
}
