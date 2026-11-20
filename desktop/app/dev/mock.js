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
    fps: 5,
    focus: true,
    subtitleBelow: true,
    window: params.has("pc") ? "Hollow.exe" : "",
    regions: params.has("pc") ? { "hollow.exe": { x: 0.1, y: 0.72, width: 0.8, height: 0.22 } } : {},
    protectedTerms: ["It Takes Two", "May", "Cody"],
    profile: "elden-ring",
  },
  profiles: [{ id: "elden-ring", name: "Elden Ring" }, { id: "hades", name: "Hades" }],
  profileGenre: "aksiyon RPG",
  profileGlossary: "Site of Grace = Lütuf Noktası\nRune = Rün\nMelina",
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
      if (cmd === "save_profile") Object.assign(status, { profileGenre: args.genre, profileGlossary: args.glossary });
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
        // ?teshis: Teşhis paneli açık, örnek ölçümlerle (?teshis=donmus: donmuş görüntü).
        const frozen = new URLSearchParams(location.search).get("teshis") === "donmus";
        emit({
          type: "health",
          diagnostics: {
            stage: frozen ? "frameFrozen" : "ok",
            summary: frozen ? "Görüntü 14 sn'dir değişmiyor: oyun duraklatılmış ya da Remote Play donmuş olabilir." : "4 çeviri gösteriliyor.",
            uptimeS: 2712, capture: frozen ? "warn" : "ok", waitingForWindow: false, readsPerS: 4.9, framesPerS: frozen ? 0 : 30,
            lastReadAgoS: 0.1, frameAgeMs: frozen ? 14000 : 21, frozenS: frozen ? 14 : 0, captureMs: 18, streamRestarts: 1, readErrors: 3,
            ocrFailing: false, lastReadError: null, ocr: "ok", ocrMs: 142, lastOcrAgoS: 0.1, lines: 7, blocks: 5, stable: 5, filtered: 1,
            cacheHits: 1840, translation: "ok", translatingEnabled: true, queue: 0, inFlight: 0, worker: "boşta", lastTranslationAgoS: 3.2,
            translationMs: 96, translationErrors: 2, timeouts: 1, rateLimited: 0, retries: 2, rejected: 1, backoffS: 0, lastTranslationError: null,
            output: "ok", tvOk: true, shown: 4, lastSendAgoS: 0.1, sendErrors: 0, roundMs: 171, latencyMs: 192, recoveries: 2, memoryMb: 212,
            systemState: "running", recovery: { state: "running", restarts: 1, recoveries: 1, lastRecoveredAgo: 1400, reason: "Ekran okuma 21 sn'dir çalışmıyor", nextAttemptIn: null },
            wakes: 1, lastWakeAgoS: 1800, candidateStage: "gösteriliyor",
            activeCandidate: { id: "b12", state: "ready", ageS: 2.4, frames: 11, stability: 1, requiredS: 0.3, confidence: 0.93, growing: false, similarity: 0.4 },
            subtitle: { candidates: 412, accepted: 377, dropped: 35, duplicates: 920, growths: 64, continued: 12, fastAccepts: 140 },
            provider: "DeepL", profile: "Elden Ring", glossaryTerms: 14, glossaryHits: 37, fixedHits: 210, normalizedHits: 44,
            memoryEntries: 1840, sessionEntries: 23, contextLines: 3, breaker: "kapalı", breakerOpens: 0, probes: 0, requests: 512, errorClass: "zaman aşımı", needsUser: false,
          },
        }, 700);
        emit({ type: "log", message: "Kare akışı takıldı; yeniden başlatıldı." }, 750);
        emit({ type: "log", message: "Çeviri 21 sn'dir yanıt vermiyor; yeni çeviri işçisi açıldı." }, 760);
        if (new URLSearchParams(location.search).has("teshis")) setTimeout(() => (document.getElementById("diag-panel").open = true), 800);
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
