// TV uygulamasındaki çizimin karşılığı: her blok orijinal yazının üstüne, font satır
// yüksekliğinden başlar; sığmazsa küçülür (alt sınıra kadar), yine sığmazsa kutu merkezi
// korunarak yatayda genişler (en fazla genişliğin %90'ı).
const { listen } = window.__TAURI__.event;
let last = "";

function place(block, W, H) {
  const el = document.createElement("div");
  el.className = "block";
  el.textContent = block.text;
  const boxW = block.w * W;
  const boxH = block.h * H;
  el.style.left = `${block.x * W}px`;
  el.style.top = `${block.y * H}px`;
  el.style.width = `${boxW}px`;
  el.style.minHeight = `${boxH}px`;
  document.body.append(el);

  // TV'de alt sınır yüksekliğin %2,8'i (uzaktan okunur); bilgisayar ekranına yakından bakıldığı
  // için burada %2.
  const minSize = Math.max(12, 0.02 * H);
  let size = Math.max(minSize, block.lineH * H * 1.15);
  el.style.fontSize = `${size}px`;
  while (el.scrollHeight > boxH * 1.05 && size > minSize) {
    size = Math.max(minSize, size * 0.92);
    el.style.fontSize = `${size}px`;
  }
  if (el.scrollHeight > boxH * 1.05) {
    const center = block.x * W + boxW / 2;
    const width = Math.min(W * 0.9, boxW * Math.min(2.5, el.scrollHeight / boxH));
    const left = Math.min(Math.max(center - width / 2, W * 0.05), W * 0.95 - width);
    el.style.left = `${left}px`;
    el.style.width = `${width}px`;
  }
}

function render(blocks) {
  const key = JSON.stringify(blocks);
  if (key === last) return;
  last = key;
  document.body.replaceChildren();
  for (const block of blocks) place(block, innerWidth, innerHeight);
}

listen("overlay", ({ payload }) => render(payload));
addEventListener("resize", () => {
  const blocks = last ? JSON.parse(last) : [];
  last = "";
  render(blocks);
});
