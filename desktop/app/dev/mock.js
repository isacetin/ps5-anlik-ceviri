// Tarayıcıda arayüz önizlemesi: Tauri köprüsü yerine geçer ve "Başlat"a basılınca gerçek
// formatta örnek olaylar yayınlar. Uygulamaya dahil edilmez (ui/ dışında).
const handlers = [];
const status = { running: false, hasDeeplKey: true, settings: { output: "both", tvAddress: "", translate: true, fps: 2, window: "", protectedTerms: ["It Takes Two", "May", "Cody"] } };
window.__TAURI__ = {
  core: {
    invoke: async (cmd) => {
      if (cmd === "status") return status;
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

// preview.html?ayarlar: Ayarlar ve DeepL panelleri açık gösterilir.
if (new URLSearchParams(location.search).has("ayarlar")) {
  addEventListener("load", () => document.querySelectorAll("details").forEach((d) => (d.open = true)));
}
