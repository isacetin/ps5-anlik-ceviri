// Tarayıcıda arayüz önizlemesi: Tauri köprüsü yerine geçer ve "Başlat"a basılınca gerçek
// formatta örnek olaylar yayınlar. Uygulamaya dahil edilmez (ui/ dışında).
const handlers = {};
const emitTo = (name, payload) => (handlers[name] ?? []).forEach((h) => h({ payload }));
// preview.html?windows: Windows'taki gibi "bu bilgisayardaki oyun" seçeneği; ?pc: o seçili gelir.
const params = new URLSearchParams(location.search);
const windows = params.has("windows") || params.has("pc");
const status = {
  running: false,
  hasDeeplKey: true,
  hasLocalModel: true,
  localModelMb: 241,
  gameSource: windows,
  settings: {
    source: params.has("pc") ? "pcGame" : "remotePlay",
    output: params.has("pc") ? "screen" : "both",
    tvAddress: "",
    translate: true,
    engine: "local",
    fps: 2,
    window: params.has("pc") ? "Hollow.exe" : "",
    regions: params.has("pc") ? { "hollow.exe": { x: 0.1, y: 0.72, width: 0.8, height: 0.22 } } : {},
    protectedTerms: ["It Takes Two", "May", "Cody"],
  },
};
const mockWindows = [
  { exe: "Hollow.exe", title: "Hollow Knight", width: 1920, height: 1080 },
  { exe: "witcher3.exe", title: "The Witcher 3", width: 2560, height: 1440 },
  { exe: "msedge.exe", title: "Yeni sekme - Microsoft Edge", width: 1280, height: 900 },
];
window.__TAURI__ = {
  core: {
    invoke: async (cmd, args = {}) => {
      if (cmd === "status") return status;
      if (cmd === "setup_status")
        return { platform: windows ? "windows" : "macos", screenRecording: windows ? null : true, remotePlay: true, tvApkAvailable: true, hasDeeplKey: true, hasLocalModel: true, localModelMb: 241, setupDone: status.settings.setupDone, gameSource: windows };
      if (cmd === "list_windows") return new Promise((r) => setTimeout(() => r(mockWindows), 200));
      if (cmd === "save_settings" || cmd === "finish_setup") Object.assign(status.settings, args.settings);
      if (cmd === "current_region") return status.settings.regions[status.settings.window.toLowerCase()] ?? null;
      if (cmd === "set_region") {
        const key = status.settings.window.toLowerCase();
        if (args.region) status.settings.regions[key] = args.region;
        else delete status.settings.regions[key];
        emitTo("region", { region: args.region, regions: status.settings.regions });
      }
      if (cmd === "tv_scan") return new Promise((r) => setTimeout(() => r([{ ip: "192.168.1.5", appRunning: false }]), 200));
      if (cmd === "tv_install") {
        const labels = [
          "TV'ye bağlanılıyor… TV'de izin sorusu çıkarsa \"Her zaman izin ver\"i işaretleyip Tamam'a basın.",
          "TV'deki sürüm kontrol ediliyor…",
          "TV uygulaması yükleniyor (yaklaşık 20 saniye)…",
        ];
        labels.forEach((label, i) => setTimeout(() => emitTo("kurulum", { label }), 100 * (i + 1)));
        return new Promise((r) => setTimeout(r, new URLSearchParams(location.search).has("kuruluyor") ? 1e9 : 500));
      }
      if (cmd === "tv_test") return new Promise((r) => setTimeout(r, 300));
      if (cmd === "start") {
        status.running = true;
        const emit = (payload, ms) => setTimeout(() => emitTo("ceviri", payload), ms);
        emit({ type: "state", state: "running", detail: "Türkçe çeviri" }, 300);
        if (status.settings.source !== "pcGame") emit({ type: "tv", connected: true, address: "ws://192.168.1.5:8765", error: null }, 400);
        emit({ type: "round", video: "1920x1080", captureMs: 18, ocrMs: 142, blocks: 5, sent: 4 }, 500);
        emit({ type: "shown", texts: ["Yerel Oyna", "Çevrimiçi Oyna", "Seçenekler", "Erişilebilirlik Seçenekleri"] }, 600);
      }
      return null;
    },
  },
  event: { listen: async (name, handler) => (handlers[name] ??= []).push(handler) },
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
