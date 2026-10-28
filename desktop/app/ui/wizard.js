// İlk kurulum sihirbazı. Her adım durumu mümkün olduğunca kendisi kontrol eder; kullanıcıdan yalnızca
// uygulamanın yapamayacağı şeyler (TV'de tuşa basmak, izin vermek) istenir.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  let status = null; // setup_status
  let settings = null; // mevcut ayarlar (sihirbaz bunları doldurur)
  let onFinish = () => {};
  let index = 0;
  const tv = { phase: "prepare", found: [], selected: "", log: [], error: "", busy: false };

  const pcGame = () => settings.source === "pcGame";
  const usesTv = () => !pcGame() && (settings.output === "tv" || settings.output === "both");

  // Adımlar: `when` yanlışsa atlanır.
  const steps = [
    { id: "welcome", render: renderWelcome },
    { id: "screen", when: () => status.platform === "macos", render: renderScreenPermission },
    { id: "remote", when: () => !pcGame(), render: renderRemotePlay },
    { id: "game", when: pcGame, render: renderGame },
    { id: "tv", when: usesTv, render: renderTv },
    { id: "controller", when: () => !pcGame(), render: renderController },
    { id: "translate", render: renderTranslate },
    { id: "done", render: renderDone },
  ];
  const active = () => steps.filter((s) => !s.when || s.when());

  function el(tag, props = {}, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (key === "class") node.className = value;
      else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
      else if (value !== undefined && value !== null && value !== false) node.setAttribute(key, value === true ? "" : value);
    }
    for (const child of children.flat()) {
      if (child !== null && child !== undefined && child !== false) node.append(child);
    }
    return node;
  }

  const button = (label, onclick, kind = "secondary", disabled = false) => el("button", { type: "button", class: kind, onclick, disabled }, label);
  const statusLine = (ok, text, pending = false) =>
    el("p", { class: "status-line" }, el("span", { class: "dot", "data-level": pending ? "warn" : ok ? "ok" : "bad" }), text);

  function render() {
    const list = active();
    index = Math.min(index, list.length - 1);
    const step = list[index];
    const progress = document.getElementById("wizard-progress");
    progress.replaceChildren(
      el("span", {}, `Adım ${index + 1} / ${list.length}`),
      el("div", { class: "dots" }, list.map((_, i) => el("i", { class: i < index ? "done" : i === index ? "current" : "" }))),
    );
    const body = document.getElementById("wizard-body");
    // Adımlar koşullu parçaları null olarak bırakır; replaceChildren null'ı "null" metni yapar.
    body.replaceChildren(...step.render().filter(Boolean));
  }

  function nav({ next = "Devam", canNext = true, back = true, onNext } = {}) {
    return el(
      "div",
      { class: "wizard-actions" },
      back && index > 0 ? button("Geri", () => { index -= 1; render(); }, "ghost") : el("span"),
      button(next, async () => {
        if (onNext && (await onNext()) === false) return;
        index += 1;
        render();
      }, "primary small", !canNext),
    );
  }

  // 1 — Hoş geldiniz
  function renderWelcome() {
    const choice = (value, title, detail) =>
      el(
        "label",
        { class: "option" },
        el("input", {
          type: "radio",
          name: "w-output",
          value,
          checked: value === "pc" ? pcGame() : !pcGame() && settings.output === value,
          onchange: () => {
            Object.assign(settings, value === "pc" ? { source: "pcGame", output: "screen" } : { source: "remotePlay", output: value });
            render();
          },
        }),
        el("span", {}, el("strong", {}, title), el("small", {}, detail)),
      );
    const pcChoice = status.gameSource
      ? [
          el("p", { class: "question" }, "Ya da bu bilgisayarda mı oynuyorsunuz?"),
          choice("pc", "Bu bilgisayarda yüklü bir oyunda", "Steam, Epic vb. ile bu bilgisayarda oynadığınız oyun. PS5 gerekmez; çeviriler oyunun üstünde gösterilir."),
        ]
      : [];
    return [
      el("h2", { class: "title" }, "Hoş geldiniz"),
      el("p", {}, "Anlık Çeviri, oynadığınız oyundaki İngilizce yazıları okuyup Türkçesini yazının tam üstünde gösterir. Birkaç adımda kuralım."),
      el("p", { class: "question" }, status.gameSource ? "PS5'te oynuyorsanız çevirileri nerede görmek istersiniz?" : "Çevirileri nerede görmek istersiniz?"),
      choice("tv", "TV'de", "Oyunu TV'den izliyorsanız. TV'nin Android TV ya da Google TV olması gerekir."),
      choice("screen", "Bu bilgisayarın ekranında", "Oyunu bilgisayardan (Remote Play penceresinden) oynuyorsanız. Her TV ile çalışır."),
      choice("both", "İkisinde de", ""),
      ...pcChoice,
      nav({ back: false }),
    ];
  }

  // 2 — Ekran Kaydı izni (Mac)
  function renderScreenPermission() {
    const granted = status.screenRecording === true;
    const recheck = async () => { status = await invoke("setup_status"); render(); };
    return [
      el("h2", { class: "title" }, "Ekranı okuma izni"),
      el("p", {}, "Uygulamanın Remote Play penceresindeki yazıları okuyabilmesi için macOS'ten \"Ekran ve Sistem Sesi Kaydı\" izni gerekir. Yalnızca Remote Play penceresi okunur; görüntü hiçbir yere gönderilmez."),
      statusLine(granted, granted ? "İzin verilmiş" : "İzin henüz verilmemiş"),
      granted
        ? null
        : el(
            "div",
            { class: "row" },
            button("İzin iste", async () => { await invoke("request_screen_recording"); await recheck(); }),
            button("Sistem Ayarları'nı aç", () => invoke("open_privacy_settings", { page: "screen" })),
            button("Tekrar kontrol et", recheck, "ghost"),
          ),
      granted ? null : el("p", { class: "help" }, "Sistem Ayarları'nda Anlık Çeviri'yi açın. macOS uygulamayı yeniden başlatmanızı isteyebilir; yeniden açtığınızda sihirbaz kaldığı yerden devam eder."),
      nav({ next: granted ? "Devam" : "Şimdilik atla" }),
    ];
  }

  // 3 — PS Remote Play
  function renderRemotePlay() {
    const open = status.remotePlay;
    return [
      el("h2", { class: "title" }, "PS Remote Play"),
      el("p", {}, "Yazılar, PS5 görüntüsünü bilgisayara getiren PlayStation'ın ücretsiz PS Remote Play uygulamasından okunur."),
      el(
        "ol",
        { class: "numbered" },
        el("li", {}, "PS Remote Play'i indirip kurun."),
        el("li", {}, "Açın, PSN hesabınızla giriş yapıp PS5'inize bağlanın."),
        el("li", {}, "Pencereyi büyük tutun ya da tam ekran yapın: ne kadar büyük olursa yazılar o kadar iyi okunur."),
      ),
      statusLine(open, open ? "Remote Play penceresi açık" : "Remote Play penceresi bulunamadı", !open),
      el(
        "div",
        { class: "row" },
        button("Remote Play'i indir", () => invoke("open_link", { which: "remote-play" })),
        button("Tekrar kontrol et", async () => { status = await invoke("setup_status"); render(); }, "ghost"),
      ),
      nav({ next: open ? "Devam" : "Sonra yaparım" }),
    ];
  }

  // 3 — PC oyunu: oyun penceresini seç
  const game = { windows: null, busy: false, error: "" };

  async function loadGames() {
    game.busy = true;
    game.error = "";
    render();
    try {
      game.windows = await invoke("list_windows");
    } catch (error) {
      game.windows = [];
      game.error = String(error);
    }
    game.busy = false;
    render();
  }

  function renderGame() {
    if (game.windows === null && !game.busy) setTimeout(loadGames);
    const windows = game.windows ?? [];
    const options = windows.map((w) =>
      el(
        "label",
        { class: "option" },
        el("input", { type: "radio", name: "w-game", value: w.exe, checked: settings.window === w.exe, onchange: () => { settings.window = w.exe; render(); } }),
        el("span", {}, el("strong", {}, w.title), el("small", {}, `${w.exe} · ${w.width}×${w.height}`)),
      ),
    );
    const savedClosed = settings.window && !windows.some((w) => w.exe === settings.window);
    return [
      el("h2", { class: "title" }, "Oyunu seçin"),
      el("p", {}, "Oyunu açın (ana menüye gelmesi yeter), sonra aşağıdaki listeden seçin."),
      game.busy ? el("p", { class: "help" }, "Pencereler aranıyor…") : null,
      !game.busy && windows.length === 0 ? statusLine(false, "Ekranda oyun penceresi bulunamadı.", true) : null,
      el("div", { class: "options" }, options),
      savedClosed ? statusLine(true, `Seçili: ${settings.window} (şu an açık değil)`) : null,
      game.error ? el("p", { class: "error-inline" }, game.error) : null,
      el("div", { class: "row" }, button("Listeyi yenile", loadGames, "ghost", game.busy)),
      el(
        "p",
        { class: "help" },
        "Oyunun grafik ayarlarında ekran modunu ",
        el("strong", {}, "Kenarlıksız pencere"),
        " (Borderless) ya da ",
        el("strong", {}, "Pencere"),
        " yapın. \"Tam ekran\" (özel tam ekran) modunda çeviriler oyunun üstünde görünmeyebilir.",
      ),
      nav({ next: settings.window ? "Devam" : "Sonra seçerim" }),
    ];
  }

  // 4 — TV kurulumu
  function renderTv() {
    if (!status.tvApkAvailable) {
      return [
        el("h2", { class: "title" }, "TV uygulaması"),
        statusLine(false, "Bu derlemede TV uygulaması bulunmuyor."),
        el("p", { class: "help" }, "Geliştirici derlemesi: önce `./gradlew assembleRelease` ile TV uygulamasını derleyin."),
        nav({ next: "Atla" }),
      ];
    }
    if (tv.phase === "prepare") return renderTvPrepare();
    if (tv.phase === "pick") return renderTvPick();
    if (tv.phase === "install") return renderTvInstall();
    return renderTvTest();
  }

  function renderTvPrepare() {
    return [
      el("h2", { class: "title" }, "TV'yi hazırlayın"),
      el("p", {}, "TV uygulamasını bu bilgisayar kuracak. Bunun için TV'de bir kez şu ayarları açın (kumandayla):"),
      el(
        "ol",
        { class: "numbered" },
        el("li", {}, "Ayarlar → Cihaz Tercihleri → Hakkında ekranında ", el("strong", {}, "Yapı"), " satırına 7 kez basın. \"Artık bir geliştiricisiniz\" mesajı çıkar."),
        el("li", {}, "Ayarlar → Cihaz Tercihleri → ", el("strong", {}, "Geliştirici seçenekleri"), " altında ", el("strong", {}, "Ağ üzerinden hata ayıklama"), "yı (bazı TV'lerde \"USB hata ayıklama\" ile birlikte) açın."),
        el("li", {}, "TV ile bu bilgisayar aynı modeme bağlı olsun."),
      ),
      el("p", { class: "help" }, "Bu ayarlar TV'ye zarar vermez. Kurulumdan sonra \"Ağ üzerinden hata ayıklama\"yı yeniden kapatabilirsiniz."),
      tv.error ? el("p", { class: "error-inline" }, tv.error) : null,
      el(
        "div",
        { class: "wizard-actions" },
        button("Geri", () => { index -= 1; render(); }, "ghost"),
        button(tv.busy ? "Aranıyor…" : "Hazırım, TV'yi bul", scanTv, "primary small", tv.busy),
      ),
    ];
  }

  async function scanTv() {
    tv.busy = true;
    tv.error = "";
    render();
    try {
      tv.found = await invoke("tv_scan");
      tv.selected = tv.found[0]?.ip ?? settings.tvAddress ?? "";
      tv.phase = "pick";
    } catch (error) {
      tv.error = String(error);
    }
    tv.busy = false;
    render();
  }

  function renderTvPick() {
    const manual = el("input", { type: "text", placeholder: "ör. 192.168.1.5", value: tv.found.some((c) => c.ip === tv.selected) ? "" : tv.selected, spellcheck: "false", oninput: (e) => (tv.selected = e.target.value.trim()) });
    return [
      el("h2", { class: "title" }, "TV'yi seçin"),
      tv.found.length
        ? el(
            "div",
            {},
            el("p", {}, tv.found.length === 1 ? "Ağda bir TV bulundu:" : "Ağda birden fazla cihaz bulundu; TV'nizi seçin:"),
            tv.found.map((c) =>
              el(
                "label",
                { class: "option" },
                el("input", { type: "radio", name: "w-tv", value: c.ip, checked: tv.selected === c.ip, onchange: () => { tv.selected = c.ip; manual.value = ""; } }),
                el("span", {}, el("strong", {}, c.ip), el("small", {}, c.appRunning ? "Anlık Çeviri zaten kurulu" : "Hata ayıklaması açık")),
              ),
            ),
          )
        : statusLine(false, "Ağda hata ayıklaması açık bir TV bulunamadı."),
      el("label", { class: "field" }, el("span", {}, tv.found.length ? "Ya da adresi elle girin" : "TV'nin adresini elle girin (TV'de Ayarlar → Ağ ve İnternet ekranında yazar)"), manual),
      el(
        "div",
        { class: "wizard-actions" },
        button("Tekrar ara", scanTv, "ghost"),
        button("Bu TV'ye kur", installTv, "primary small"),
      ),
    ];
  }

  async function installTv() {
    if (!tv.selected) return;
    tv.phase = "install";
    tv.log = [];
    tv.error = "";
    tv.busy = true;
    render();
    try {
      await invoke("tv_install", { ip: tv.selected });
      settings.tvAddress = tv.selected;
      tv.phase = "test";
      tv.busy = false;
      render();
      showTest();
      return;
    } catch (error) {
      tv.error = String(error);
    }
    tv.busy = false;
    render();
  }

  function renderTvInstall() {
    return [
      el("h2", { class: "title" }, "TV'ye kuruluyor"),
      el("ul", { class: "log" }, tv.log.map((line, i) => el("li", { class: i === tv.log.length - 1 && tv.busy ? "current" : "" }, line))),
      tv.error ? el("p", { class: "error-inline" }, tv.error) : null,
      tv.busy
        ? el("p", { class: "help" }, "TV ekranına bakın: ilk kurulumda \"Hata ayıklamaya izin verilsin mi?\" sorusu çıkar.")
        : el("div", { class: "wizard-actions" }, button("Geri", () => { tv.phase = "pick"; render(); }, "ghost"), button("Tekrar dene", installTv, "primary small")),
    ];
  }

  async function showTest() {
    tv.testing = true;
    render();
    try {
      await invoke("tv_test", { ip: tv.selected });
    } catch (error) {
      tv.error = String(error);
    }
    tv.testing = false;
    render();
  }

  function renderTvTest() {
    return [
      el("h2", { class: "title" }, "TV kuruldu"),
      statusLine(true, `TV uygulaması ${tv.selected} adresinde çalışıyor.`),
      el("p", { class: "question" }, "TV'de, PS5'in HDMI girişindeyken ekranın ortasında \"Anlık Çeviri hazır ✓\" yazısını görüyor musunuz?"),
      el("p", { class: "help" }, "TV kurulum sırasında uygulamanın ekranına geçmiş olabilir; kumandayla PS5'in HDMI girişine dönün."),
      tv.error ? el("p", { class: "error-inline" }, tv.error) : null,
      el(
        "div",
        { class: "row" },
        button(tv.testing ? "Gösteriliyor…" : "Tekrar göster", showTest, "ghost", tv.testing),
      ),
      el(
        "details",
        { class: "tips" },
        el("summary", {}, "Görmüyorum"),
        el(
          "ul",
          {},
          el("li", {}, "TV'de “PS5 Çeviri” uygulamasını açıp \"Servis: Çalışıyor\" ve \"Overlay penceresi: Ekranda\" yazdığını kontrol edin."),
          el("li", {}, "Yazı ekranda ama yeri kaymışsa TV'nin resim formatını \"Tam Piksel\" ya da \"Just Scan\" yapın."),
        ),
      ),
      el(
        "p",
        { class: "help" },
        "İsterseniz şimdi TV'de \"Ağ üzerinden hata ayıklama\"yı kapatabilirsiniz; TV uygulaması çalışmaya devam eder (güncellemek için yeniden açmanız gerekir). TV uygulamasını istediğiniz zaman TV'nin Ayarlar → Uygulamalar menüsünden ya da bu uygulamanın ayarlarından kaldırabilirsiniz.",
      ),
      nav({ next: "Görüyorum, devam" }),
    ];
  }

  // 5 — Kontrolcü
  function renderController() {
    return [
      el("h2", { class: "title" }, "Kontrolcü"),
      el("p", {}, "DualSense'i PS5'e değil ", el("strong", {}, "bu bilgisayara"), " bağlayın. PS5'e bağlı kontrolcüyle oynamaya başlayınca PS5 Remote Play bağlantısını keser ve çeviri durur."),
      el(
        "ul",
        { class: "numbered" },
        el("li", {}, el("strong", {}, "Kablo: "), "DualSense'i USB-C kabloyla bilgisayara takın (veri aktaran bir kablo olmalı)."),
        el("li", {}, el("strong", {}, "Bluetooth: "), "PS5'i kapatın, DualSense'te Create ve PS tuşlarına ışık hızlı yanıp sönene kadar basılı tutun, bilgisayarın Bluetooth ayarlarından bağlanın."),
      ),
      el("p", { class: "help" }, usesTv() ? "Görüntüyü TV'den izlersiniz (gecikmesiz); tuşlar Remote Play üzerinden PS5'e gider." : "Tuşlar Remote Play üzerinden PS5'e gider."),
      nav(),
    ];
  }

  // 6 — Çeviri
  const model = { busy: false, percent: 0, note: "", ok: false };

  async function downloadModel() {
    model.busy = true;
    model.note = "İndiriliyor…";
    model.ok = false;
    render();
    try {
      model.note = await invoke("download_local_model");
      model.ok = true;
      status.hasLocalModel = true;
    } catch (error) {
      model.note = String(error);
    }
    model.busy = false;
    render();
  }

  function renderTranslate() {
    const mode = settings.translate ? settings.engine : "test";
    const choose = (value) => {
      Object.assign(settings, value === "test" ? { translate: false } : { translate: true, engine: value });
      render();
    };
    const choice = (value, title, detail) =>
      el(
        "label",
        { class: "option" },
        el("input", { type: "radio", name: "w-mode", value, checked: mode === value, onchange: () => choose(value) }),
        el("span", {}, el("strong", {}, title), el("small", {}, detail)),
      );

    const local = el(
      "div",
      { class: "choice-detail" },
      status.hasLocalModel
        ? statusLine(true, "Model indirildi.")
        : el("p", { class: "help" }, `Model bir kez indirilir (${status.localModelMb} MB).`),
      model.busy ? el("progress", { max: "100", value: String(model.percent) }) : null,
      status.hasLocalModel ? null : el("div", { class: "row" }, button(model.busy ? "İndiriliyor…" : "Modeli indir", downloadModel, "secondary", model.busy)),
      model.note ? el("p", { class: model.ok ? "note ok" : model.busy ? "note" : "note bad" }, model.note) : null,
    );

    const input = el("input", { type: "password", placeholder: "xxxxxxxx-xxxx-…:fx", autocomplete: "off", spellcheck: "false" });
    const note = el("span", { class: "note" });
    const save = async () => {
      note.className = "note";
      note.textContent = "Deneniyor…";
      try {
        note.textContent = await invoke("save_deepl_key", { key: input.value });
        note.className = "note ok";
        input.value = "";
        status.hasDeeplKey = true;
      } catch (error) {
        note.className = "note bad";
        note.textContent = String(error);
      }
    };
    const deepl = el(
      "div",
      { class: "choice-detail" },
      el("p", { class: "help" }, el("strong", {}, "DeepL API Free"), " hesabıyla ayda 500.000 karakter ücretsizdir (kayıtta kart istenir, ücret çekilmez)."),
      status.hasDeeplKey ? statusLine(true, "Anahtar kayıtlı; değiştirmek isterseniz yenisini girin.") : null,
      el("div", { class: "row" }, button("DeepL hesabı aç", () => invoke("open_link", { which: "deepl" }))),
      el("label", { class: "field" }, el("span", {}, "API anahtarı (\":fx\" ile biter; yalnızca bu bilgisayarda saklanır)"), input),
      el("div", { class: "actions" }, button("Kaydet ve dene", save), note),
    );

    const ready = mode === "test" || (mode === "local" ? status.hasLocalModel : status.hasDeeplKey);
    return [
      el("h2", { class: "title" }, "Çeviri"),
      el("p", {}, "Çeviriyi kim yapsın?"),
      choice("local", "Yerel model (önerilen)", "Bu bilgisayarda çalışır: altyazı başına ~0,1 sn, ücretsiz, internetsiz. Konuşmalarda DeepL'e yakın, deyimlerde daha zayıf."),
      mode === "local" ? local : null,
      choice("deepl", "DeepL", "En iyi çeviri kalitesi. Ücretsiz hesap ve API anahtarı gerekir, internet ister."),
      mode === "deepl" ? deepl : null,
      choice("test", "Şimdilik çevirisiz dene", "İngilizce yazıları olduğu yerde gösterir; kutuların doğru yere oturduğunu görürsünüz."),
      nav({ next: ready || model.busy ? "Devam" : "Sonra yaparım", canNext: !model.busy }),
    ];
  }

  // 7 — Bitti
  function renderDone() {
    const where = pcGame() ? "oyunun üstünde" : { tv: "TV'de", screen: "bu ekranda", both: "TV'de ve bu ekranda" }[settings.output];
    return [
      el("h2", { class: "title" }, "Hazır"),
      el("p", {}, `Çeviriler ${where} gösterilecek (${{ local: "yerel model", deepl: "DeepL", test: "çevirisiz test modu" }[settings.translate ? settings.engine : "test"]}).`),
      pcGame()
        ? el("p", {}, "Oyun açıkken ana ekrandaki ", el("strong", {}, "Başlat"), "a basın.")
        : el("p", {}, "Remote Play açıkken ana ekrandaki ", el("strong", {}, "Başlat"), "a basın. İlk açılışta yazı tanıma modeli ~30 saniyede yüklenir."),
      pcGame()
        ? el(
            "p",
            { class: "help" },
            "Oyun içindeyken ",
            el("kbd", {}, "Ctrl+Shift+T"),
            " çevirileri gizler/gösterir. Ekranda çok yazı varsa ",
            el("kbd", {}, "Ctrl+Shift+R"),
            " ile yalnızca bir bölgeyi (ör. altyazı bandı) okutun.",
          )
        : null,
      el("p", { class: "help" }, "Bu sihirbazı Ayarlar'dan istediğiniz zaman yeniden açabilirsiniz."),
      finishError ? el("p", { class: "error-inline" }, finishError) : null,
      el(
        "div",
        { class: "wizard-actions" },
        button("Geri", () => { index -= 1; render(); }, "ghost"),
        button("Kurulumu bitir", finish, "primary small"),
      ),
    ];
  }

  let finishError = "";

  async function finish() {
    try {
      await invoke("finish_setup", { settings });
      finishError = "";
      close();
      onFinish();
    } catch (error) {
      finishError = String(error);
      render();
    }
  }

  function close() {
    document.getElementById("wizard").hidden = true;
    document.getElementById("app-main").hidden = false;
  }

  listen("model", ({ payload }) => {
    model.percent = Math.floor((payload.done * 100) / Math.max(payload.total, 1));
    model.note = `İndiriliyor… %${model.percent} (${Math.round(payload.done / 1e6)} / ${Math.round(payload.total / 1e6)} MB)`;
    if (model.busy && active()[index]?.id === "translate") render();
  });

  listen("kurulum", ({ payload }) => {
    tv.log.push(payload.label);
    if (tv.phase === "install") render();
  });

  window.Wizard = {
    async open(currentSettings, done) {
      settings = { ...currentSettings };
      onFinish = done;
      status = await invoke("setup_status");
      index = 0;
      Object.assign(tv, { phase: "prepare", found: [], selected: settings.tvAddress ?? "", log: [], error: "", busy: false });
      Object.assign(model, { busy: false, percent: 0, note: "", ok: false });
      Object.assign(game, { windows: null, busy: false, error: "" });
      document.getElementById("app-main").hidden = true;
      document.getElementById("wizard").hidden = false;
      render();
    },
  };
})();
