// Arayüz: komutlar Rust tarafındaki Controller'a gider, iş hattı olayları "ceviri" adıyla gelir.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
let running = false;
let busy = false;
let currentOutput = "tv";

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
  if (!running) {
    setLevel("tv-dot", "idle");
    $("tv-value").textContent = usesTv(currentOutput) ? "Bağlı değil" : "Kullanılmıyor";
    setLevel("window-dot", "idle");
    $("window-value").textContent = "—";
    $("stats").hidden = true;
    renderShown([]);
    $("state-detail").textContent = "PS Remote Play'i açıp PS5'e bağlandıktan sonra başlatın.";
  }
}

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

function renderSettings(settings) {
  currentOutput = settings.output;
  for (const radio of document.querySelectorAll('input[name="output"]')) {
    radio.checked = radio.value === settings.output;
  }
  $("tv-address-field").hidden = !usesTv(settings.output);
  $("tv-address").value = settings.tvAddress;
  for (const radio of document.querySelectorAll('input[name="mode"]')) {
    radio.checked = radio.value === (settings.translate ? "translate" : "test");
  }
  $("fps").value = String(Math.round(settings.fps));
  $("window").value = settings.window;
  $("protected-terms").value = settings.protectedTerms.join("\n");
  $("mode-value").textContent = settings.translate ? "Türkçe (DeepL)" : "Çevirisiz test";
  setLevel("mode-dot", settings.translate ? "ok" : "warn");
  if (!running) $("tv-value").textContent = usesTv(settings.output) ? "Bağlı değil" : "Kullanılmıyor";
}

function readSettings() {
  return {
    output: document.querySelector('input[name="output"]:checked')?.value ?? "tv",
    tvAddress: $("tv-address").value.trim(),
    translate: document.querySelector('input[name="mode"]:checked')?.value !== "test",
    fps: Number($("fps").value),
    window: $("window").value.trim(),
    protectedTerms: $("protected-terms").value.split("\n").map((t) => t.trim()).filter(Boolean),
  };
}

function renderKey(hasKey) {
  const badge = $("key-badge");
  badge.textContent = hasKey ? "Kayıtlı" : "Yok";
  badge.classList.toggle("ok", hasKey);
}

async function refresh() {
  const status = await invoke("status");
  renderSettings(status.settings);
  renderKey(status.hasDeeplKey);
  setState(status.running ? "running" : "stopped");
}

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
      $("stats").textContent = `Yakalama ${event.captureMs} ms · OCR ${event.ocrMs} ms · ${event.blocks} blok · ${event.sent} TV'de`;
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

listen("ceviri", onEvent);
refresh().catch((error) => showError(String(error)));
