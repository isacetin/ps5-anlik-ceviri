// Arayüz: komutlar Rust tarafındaki Controller'a gider, iş hattı olayları "ceviri" adıyla gelir.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
let running = false;
let busy = false;
let currentOutput = "tv";
// Sunucudaki ayarların tamamı; form yalnızca bir kısmını düzenler (ör. setupDone formda yok).
let savedSettings = null;
// Bu bilgisayarda oynanan oyun okunabilir mi (yalnızca Windows).
let gameSource = false;
// Formda seçili kaynak: "remotePlay" | "pcGame".
let currentSource = "remotePlay";

function setLevel(dot, level) {
  $(dot).dataset.level = level;
}

function showError(message) {
  const box = $("error");
  box.textContent = message;
  box.hidden = !message;
}

function setState(state, detail) {
  running = state !== "stopped";
  const pill = $("state-pill");
  pill.dataset.state = state;
  pill.textContent = { stopped: "Durduruldu", starting: "Başlıyor", running: "Çalışıyor" }[state] ?? state;
  const button = $("toggle");
  button.textContent = running ? "Durdur" : "Başlat";
  button.classList.toggle("stop", running);
  button.disabled = busy;
  if (detail) $("state-detail").textContent = detail;
  for (const element of $("settings").elements) element.disabled = running;
  // Bölge çalışırken de değişir: okuyucu bir sonraki turda yeni bölgeyi okur.
  $("region-select").disabled = false;
  $("region-clear").disabled = false;
  if (!running) {
    setLevel("tv-dot", "idle");
    $("tv-value").textContent = usesTv(currentOutput) ? "Bağlı değil" : "Kullanılmıyor";
    setLevel("window-dot", "idle");
    $("window-value").textContent = "—";
    $("stats").hidden = true;
    renderShown([]);
    $("state-detail").textContent = idleDetail();
  }
}

const isPcGame = () => currentSource === "pcGame";
const idleDetail = () => (isPcGame() ? "Oyunu açtıktan sonra başlatın." : "PS Remote Play'i açıp PS5'e bağlandıktan sonra başlatın.");

// Kaynağa göre formun görünen kısmı: PC oyununda TV ve Remote Play pencere alanı yoktur.
function renderSource(source) {
  currentSource = source;
  const pc = isPcGame();
  $("source-field").hidden = !gameSource;
  $("game-fields").hidden = !pc;
  $("output-field").hidden = pc;
  $("window-field").hidden = pc;
  $("tv-address-field").hidden = pc || !usesTv(document.querySelector('input[name="output"]:checked')?.value ?? currentOutput);
  $("window-label").textContent = pc ? "Oyun" : "Remote Play";
  if (!running) {
    $("state-detail").textContent = idleDetail();
    $("tv-value").textContent = !pc && usesTv(currentOutput) ? "Bağlı değil" : "Kullanılmıyor";
  }
  if (pc && !gameWindows) loadGameWindows();
}

// Oyun penceresi listesi; kayıtlı oyun açık değilse de seçili kalır.
let gameWindows = null;

async function loadGameWindows() {
  const note = $("game-note");
  try {
    gameWindows = await invoke("list_windows");
    note.textContent = gameWindows.length ? "" : "Ekranda oyun penceresi bulunamadı. Oyunu açıp Yenile'ye basın.";
    note.className = "note";
  } catch (error) {
    gameWindows = [];
    note.className = "note bad";
    note.textContent = String(error);
  }
  renderGameWindows($("game-window").value || savedSettings?.window || "");
}

function renderGameWindows(selected) {
  const select = $("game-window");
  const option = (value, text) => Object.assign(document.createElement("option"), { value, textContent: text });
  const options = [option("", "— Oyunu seçin —")];
  for (const w of gameWindows ?? []) options.push(option(w.exe, `${w.title} — ${w.exe}`));
  if (selected && !(gameWindows ?? []).some((w) => w.exe === selected)) options.push(option(selected, `${selected} (şu an açık değil)`));
  select.replaceChildren(...options);
  select.value = selected;
}

function renderRegion(region) {
  $("region-value").textContent = region
    ? `seçili bölge (genişlik %${Math.round(region.width * 100)}, yükseklik %${Math.round(region.height * 100)})`
    : "tüm ekran";
  $("region-clear").hidden = !region;
}

const regionKey = (settings) => settings.window.trim().toLowerCase();

function renderShown(texts) {
  const list = $("shown");
  list.replaceChildren();
  if (texts.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = running ? "Ekranda şu an yazı yok." : "Henüz bir şey gösterilmedi.";
    list.append(empty);
    return;
  }
  for (const text of texts) {
    const item = document.createElement("li");
    item.textContent = text;
    list.append(item);
  }
}

const usesTv = (output) => output === "tv" || output === "both";
// Formdaki "mod": çevirisiz test ya da çeviriyi yapan (yerel model / DeepL).
const modeOf = (settings) => (settings.translate ? settings.engine : "test");

function renderSettings(settings) {
  savedSettings = settings;
  currentOutput = settings.output;
  for (const radio of document.querySelectorAll('input[name="source"]')) {
    radio.checked = radio.value === settings.source;
  }
  for (const radio of document.querySelectorAll('input[name="output"]')) {
    radio.checked = radio.value === settings.output;
  }
  $("tv-address-field").hidden = !usesTv(settings.output);
  $("tv-address").value = settings.tvAddress;
  for (const radio of document.querySelectorAll('input[name="mode"]')) {
    radio.checked = radio.value === modeOf(settings);
  }
  $("fps").value = String(Math.round(settings.fps));
  $("window").value = settings.window;
  $("protected-terms").value = settings.protectedTerms.join("\n");
  $("mode-value").textContent = { local: "Türkçe (yerel model)", deepl: "Türkçe (DeepL)", test: "Çevirisiz test" }[modeOf(settings)];
  setLevel("mode-dot", settings.translate ? "ok" : "warn");
  renderGameWindows(settings.source === "pcGame" ? settings.window : "");
  renderRegion(settings.source === "pcGame" ? settings.regions?.[regionKey(settings)] ?? null : null);
  renderSource(settings.source);
}

function readSettings() {
  const source = gameSource ? (document.querySelector('input[name="source"]:checked')?.value ?? "remotePlay") : "remotePlay";
  return {
    ...savedSettings,
    source,
    output: document.querySelector('input[name="output"]:checked')?.value ?? "tv",
    tvAddress: $("tv-address").value.trim(),
    ...modeSettings(document.querySelector('input[name="mode"]:checked')?.value ?? "local"),
    fps: Number($("fps").value),
    window: source === "pcGame" ? $("game-window").value : $("window").value.trim(),
    protectedTerms: $("protected-terms").value.split("\n").map((t) => t.trim()).filter(Boolean),
  };
}

function modeSettings(mode) {
  return mode === "test" ? { translate: false } : { translate: true, engine: mode };
}

function renderModel(installed, sizeMb) {
  const badge = $("model-badge");
  badge.textContent = installed ? "İndirildi" : "Yok";
  badge.classList.toggle("ok", installed);
  $("model-size").textContent = String(sizeMb);
  $("model-download").textContent = installed ? "Yeniden doğrula" : "İndir";
}

function renderKey(hasKey) {
  const badge = $("key-badge");
  badge.textContent = hasKey ? "Kayıtlı" : "Yok";
  badge.classList.toggle("ok", hasKey);
}

async function refresh() {
  const status = await invoke("status");
  gameSource = status.gameSource;
  renderSettings(status.settings);
  renderKey(status.hasDeeplKey);
  renderModel(status.hasLocalModel, status.localModelMb);
  setState(status.running ? "running" : "stopped");
  return status;
}

async function start() {
  const status = await refresh();
  // İlk açılış: kurulum sihirbazı.
  if (!status.settings.setupDone && !status.running) window.Wizard.open(status.settings, refresh);
}

$("open-wizard").addEventListener("click", () => window.Wizard.open(savedSettings, refresh));

// Kaldırma iki adımlı: ilk tıklama onay ister.
let uninstallArmed = false;
$("tv-uninstall").addEventListener("click", async () => {
  const note = $("maintenance-note");
  const ip = savedSettings.tvAddress;
  if (!ip) {
    note.className = "note bad";
    note.textContent = "Kayıtlı bir TV adresi yok. TV uygulamasını TV'nin Ayarlar → Uygulamalar menüsünden kaldırabilirsiniz.";
    return;
  }
  if (!uninstallArmed) {
    uninstallArmed = true;
    $("tv-uninstall").textContent = `Emin misiniz? ${ip} üzerinden kaldır`;
    setTimeout(() => {
      uninstallArmed = false;
      $("tv-uninstall").textContent = "TV uygulamasını kaldır";
    }, 5000);
    return;
  }
  uninstallArmed = false;
  $("tv-uninstall").textContent = "TV uygulamasını kaldır";
  note.className = "note";
  note.textContent = "Kaldırılıyor…";
  try {
    await invoke("tv_uninstall", { ip });
    note.className = "note ok";
    note.textContent = "TV uygulaması kaldırıldı.";
  } catch (error) {
    note.className = "note bad";
    note.textContent = String(error);
  }
});

function onEvent({ payload: event }) {
  switch (event.type) {
    case "state":
      if (event.state === "running" || event.state === "starting") showError("");
      setState(event.state, event.detail);
      break;
    case "tv":
      setLevel("tv-dot", event.connected ? "ok" : "bad");
      $("tv-value").textContent = event.connected ? event.address.replace("ws://", "") : "Ulaşılamıyor";
      $("tv-value").title = event.error ?? "";
      break;
    case "window":
      setLevel("window-dot", event.found ? "ok" : "warn");
      $("window-value").textContent = event.found ? "Okunuyor" : "Pencere bekleniyor";
      $("window-value").title = event.message;
      break;
    case "round":
      setLevel("window-dot", "ok");
      $("window-value").textContent = `Okunuyor (${event.video})`;
      $("stats").hidden = false;
      $("stats").textContent = `Yakalama ${event.captureMs} ms · OCR ${event.ocrMs} ms · ${event.blocks} blok · ${event.sent} gösterilen`;
      break;
    case "shown":
      renderShown(event.texts);
      break;
    case "error":
      showError(event.message);
      break;
  }
}

$("toggle").addEventListener("click", async () => {
  busy = true;
  $("toggle").disabled = true;
  try {
    if (running) {
      await invoke("stop");
    } else {
      showError("");
      await invoke("start");
      setState("starting", "Başlıyor…");
    }
  } catch (error) {
    showError(String(error));
  } finally {
    busy = false;
    $("toggle").disabled = false;
  }
});

for (const radio of document.querySelectorAll('input[name="output"]')) {
  radio.addEventListener("change", () => ($("tv-address-field").hidden = !usesTv(radio.value)));
}

for (const radio of document.querySelectorAll('input[name="source"]')) {
  radio.addEventListener("change", () => renderSource(radio.value));
}

$("game-refresh").addEventListener("click", loadGameWindows);

$("game-window").addEventListener("change", () => {
  const key = $("game-window").value.toLowerCase();
  renderRegion(savedSettings.regions?.[key] ?? null);
});

// Bölge seçimi kayıtlı oyun için yapılır: durmuşken önce formdaki seçim kaydedilir.
async function regionAction(action) {
  const note = $("game-note");
  note.className = "note";
  note.textContent = "";
  try {
    if (!running) {
      const settings = readSettings();
      await invoke("save_settings", { settings });
      savedSettings = settings;
    }
    await action();
  } catch (error) {
    note.className = "note bad";
    note.textContent = String(error);
  }
}

$("region-select").addEventListener("click", () => regionAction(() => invoke("open_region")));
$("region-clear").addEventListener("click", () => regionAction(() => invoke("set_region", { region: null })));

listen("region", ({ payload }) => {
  savedSettings = { ...savedSettings, regions: payload.regions };
  renderRegion(payload.region);
});

listen("overlay-hidden", ({ payload: hidden }) => {
  if (running) $("state-detail").textContent = hidden ? "Çeviriler gizli: Ctrl+Shift+T ile geri getirin." : "Çeviriler gösteriliyor.";
});

$("settings").addEventListener("submit", async (event) => {
  event.preventDefault();
  const note = $("settings-note");
  const settings = readSettings();
  try {
    await invoke("save_settings", { settings });
    renderSettings(settings);
    note.className = "note ok";
    note.textContent = "Kaydedildi";
  } catch (error) {
    note.className = "note bad";
    note.textContent = String(error);
  }
});

$("deepl").addEventListener("submit", async (event) => {
  event.preventDefault();
  const note = $("deepl-note");
  const input = $("deepl-key");
  note.className = "note";
  note.textContent = "Deneniyor…";
  try {
    const sample = await invoke("save_deepl_key", { key: input.value });
    input.value = "";
    note.className = "note ok";
    note.textContent = sample;
  } catch (error) {
    note.className = "note bad";
    note.textContent = String(error);
  }
  renderKey((await invoke("status")).hasDeeplKey);
});

$("model-download").addEventListener("click", async () => {
  const note = $("model-note");
  const button = $("model-download");
  button.disabled = true;
  note.className = "note";
  note.textContent = "İndiriliyor…";
  $("model-progress").hidden = false;
  try {
    note.textContent = await invoke("download_local_model");
    note.className = "note ok";
  } catch (error) {
    note.className = "note bad";
    note.textContent = String(error);
  }
  $("model-progress").hidden = true;
  button.disabled = false;
  const status = await invoke("status");
  renderModel(status.hasLocalModel, status.localModelMb);
});

listen("model", ({ payload }) => {
  const percent = Math.floor((payload.done * 100) / Math.max(payload.total, 1));
  $("model-progress").value = percent;
  $("model-note").textContent = `İndiriliyor… %${percent} (${Math.round(payload.done / 1e6)} / ${Math.round(payload.total / 1e6)} MB)`;
});

listen("ceviri", onEvent);
start().catch((error) => showError(String(error)));
