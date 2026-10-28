// Bölge seçimi: pencere oyunun istemci alanını tam kaplar; çizilen dikdörtgen pencereye göre
// 0...1 aralığına çevrilip `set_region` ile gönderilir. Rust tarafı kaydedip pencereyi kapatır.
const { invoke } = window.__TAURI__.core;

const help = document.getElementById("help");
let start = null;
let box = null;

function place(element, rect) {
  Object.assign(element.style, { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.width}px`, height: `${rect.height}px` });
}

function rectBetween(a, b) {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) };
}

const close = () => invoke("close_region");
const wholeScreen = () => invoke("set_region", { region: null });

// Kayıtlı bölge kesikli çizgiyle gösterilir.
invoke("current_region").then((region) => {
  if (!region) return;
  const current = document.createElement("div");
  current.className = "box current";
  place(current, { x: region.x * innerWidth, y: region.y * innerHeight, width: region.width * innerWidth, height: region.height * innerHeight });
  document.body.append(current);
});

addEventListener("mousedown", (event) => {
  if (event.button !== 0 || help.contains(event.target)) return;
  start = { x: event.clientX, y: event.clientY };
  box?.remove();
  box = document.createElement("div");
  box.className = "box";
  document.body.append(box);
  place(box, rectBetween(start, start));
  help.style.visibility = "hidden";
});

addEventListener("mousemove", (event) => {
  if (start) place(box, rectBetween(start, { x: event.clientX, y: event.clientY }));
});

addEventListener("mouseup", (event) => {
  if (!start || event.button !== 0) return;
  const rect = rectBetween(start, { x: event.clientX, y: event.clientY });
  start = null;
  // Yanlışlıkla tıklama: bölge sayılmaz.
  if (rect.width < 12 || rect.height < 12) {
    box.remove();
    box = null;
    help.style.visibility = "";
    return;
  }
  invoke("set_region", {
    region: { x: rect.x / innerWidth, y: rect.y / innerHeight, width: rect.width / innerWidth, height: rect.height / innerHeight },
  });
});

addEventListener("contextmenu", (event) => {
  event.preventDefault();
  wholeScreen();
});

addEventListener("keydown", (event) => {
  if (event.key === "Escape") close();
});

document.getElementById("whole").addEventListener("click", wholeScreen);
document.getElementById("cancel").addEventListener("click", close);
