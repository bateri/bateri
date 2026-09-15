# Ayarlar, tema ve font

## Hedef

Kullanıcı bateri'nin temasını, fontunu ve pano davranışını **uygulama
açıkken** değiştirebilsin: ayar dosyasını kaydettiği ya da menüden tema
seçtiği an ekran değişsin, shell'ler yaşamaya devam etsin, hata pencerede
görünsün. Açık ve koyu tema gömülü gelsin, varsayılan olarak sistemin
görünümünü izlesin.

## Gereksinimler

- **R1** — Ayar modeli `bt-core`'da saf fonksiyonlar: metin → `Settings` +
  tanılar; metin + (anahtar, değer) → biçimi korunmuş yeni metin. Tek
  bağımlılık `toml_edit`, yalnız `bt-core`'da.
  - **R1.1** — Ayrıştırılamayan dosya: açılışta varsayılanlar, canlı
    yenilemede hiçbir şey uygulanmaz. Ayrıştırılan dosyada her anahtar ya
    geçerli değerini ya varsayılanını alır; yanlış tip tanı üretir;
    bilinmeyen anahtar sessizce yoksayılır.
  - **R1.2** — Süreli koşu (`BT_RUN_SECONDS`) dört girişi **tek dalda**
    kapatır: dosya okuma, izleme, sistem görünümü, Tema menüsünün dolması.
    Sınamalar gerçek `HOME`'a dokunmaz.
  - **R1.3** — `[terminal] scrollback` ilk tüketici; tavanı kaynağıyla.
- **R2** — Hata pencere alt başlığında görünür: kaynak başına yuva (ayar
  dosyası, tema, font, yazma), yuva yalnız kendi kaynağı düzelince boşalır,
  tek sahip fonksiyon; stderr'e de basılır.
- **R3** — Tema: `background`, `foreground`, `dim`, `accent` + `[ansi]` 16
  renk, her anahtar opsiyonel, eksik anahtar gömülü `bateri`'den.
  - **R3.1** — Tek kaynak: zemin atlaması, clear rengi, imleç rengi ve renk
    sorgusu aynı temadan. `DEFAULT_BG`/`DEFAULT_CURSOR` kalkar,
    `Theme::BATERI` `const`. Tema `Adapter`'da yaprak kilit altında,
    takas edilebilir.
  - **R3.2** — Taşımadan önce bugünkü 19 palet değeri sabit listeye bağlı.
  - **R3.3** — Varsayılan ön plan + SGR 2 `dim` rolünü alır (ters video dahil).
  - **R3.4** — Tema adı çözümü: `themes/{ad}.toml` → gömülü; kullanılamazsa
    görünür hata ve yedek. Tema **bir görünüm için** seçilirken (açılış,
    görünüm değişimi) yedek görünüme uyan gömülü tema; görünüm aynıyken
    etkin tema dosyası bozulunca (canlı yenileme) ekrandaki tema kalır.
    (phase-3'te düzeldi: "önceki tema" görünüm değişiminde öteki görünümün
    temasını bırakıyordu.)
- **R4** — Açık tema ve sistem: gömülü `bateri-light`; `theme = "system"`
  (varsayılan) → `light_theme` / `dark_theme`; görünüm değişimi canlı.
  - **R4.1** — SGR 2'li renk zemine doğru karıştırılır; sabit değerli çizim
    yolu bekçisi kuraldan önce var.
- **R5** — Canlı izleme: dizin + `settings.toml` + etkin kullanıcı teması
  dosya kaynakları, her olayda yeniden kurulur; fark → yalnız değişen
  uygulanır; `Term::set_options`'a `Config` `Settings`'in tamamından gider;
  uygulamadan sonra kare istenir; dizin yoksa kaynak yok.
- **R6** — Font: `[font] family / size` canlı; aile atlas anahtarında; aile
  bulunamazsa zincir + görünür bildirim (`bt-atlas` tipi sızmadan);
  eşaralıklı olmayan aile uyarısı; punto kırpması sessiz. Ölçeğin iki kapısı
  borcu gerekçesiyle kapanır.
- **R7** — Ana menü: About, "Ayarlar…" (Cmd ,; yoksa şablonla oluşturur),
  Çıkış (Cmd Q); Düzen'de Kopyala/Yapıştır menü seçicilerine taşınır, geçici
  köprü silinir, Command'lı tuşu yutan dal kalır.
- **R8** — Görünüm menüsü: Tema ▸ (açılırken listelenir, seçili işaretli)
  yalnız dosyaya yazar — biçim korunur, yerinde yazılır, ayrıştırılamayan
  dosyaya yazılmaz; Cmd +/−/0 geçici punto, dosyadaki `size` değişince
  sıfırlanır.
- **R9** — OSC 52: `[clipboard] osc52 = "off" | "copy"`, varsayılan `copy`,
  tanınmayan değer ve açılışta ayrıştırılamayan dosya → `off`; `Wake`'e yeni
  çağrı, kilitsiz tek yuva (son yazma kazanır), `p`/`s` yoksayılır.
- **R10** — `docs/AYARLAR.md` ilk anahtarla doğar ve her phase'de o phase'in
  anahtarları, davranışı ve sınırlarıyla büyür.

## Yaklaşım

1. **Model ve görünür hata** (phase-1): `bt-core::settings`, `bt-shell`'de
   kök dizini parametre alan yükleyici, hermetik dal, alt başlık yuvaları,
   `scrollback`.
2. **Tema altyapısı** (phase-2): bekçiler önce; `Theme`, tek kaynak, yaprak
   kilit, tema dosyası ve ad çözümü. Ekran bit bit aynı.
3. **Açık tema ve sistem** (phase-3): sönük kural değişimi, `bateri-light`,
   `system` + görünüm takibi.
4. **Canlı izleme** (phase-4): vnode kaynakları, fark ve uygulama zinciri.
5. **Canlı font** (phase-5): aile ve punto, font bildirimi.
6. **Ana menü** (phase-6): uygulama ve Düzen menüleri, Cmd-C/V taşıması.
7. **Görünüm menüsü** (phase-7): Tema ▸ ve yazma, punto kısayolları.
8. **OSC 52** (phase-8): anahtar ve köprü.

Her phase `make hepsi`'yi yeşil bırakır; hangisinde durulursa durulsun kalan
hâl tutarlıdır (öncekiler çalışır, sonrakiler yoktur).

## Kapsam Dışı

Ayar penceresi; `line_height`; gömülü font; dört durum rolü (013); geniş tema
kataloğu (009); tema değişiminin çalışan uygulamaya bildirilmesi (DEC 2031);
kapatma onayı; LS_COLORS/prompt renkleri (012); OSC 52 okuma yönü; komut
paleti; "önce ata, sonra sor" renk sorgusu sınırı (`session.rs:473-478`,
yorumu yönlenir). Gerekçeler `discussion.md`'de.

## Göç

- Kullanıcının makinesinde ayar dosyası yok; dosya yokken davranış bugünküyle
  aynı, tek fark varsayılan temanın sistem görünümünü izlemesi (açık modda
  `bateri-light`).
- **Klavye:** Cmd-Q artık uygulamayı kapatır (bugün yutuluyor) ve açık
  programı sormadan kapatır. Cmd-C/V aynı davranır, yolu menüdür.
- `make duman` jetonları değişmez: süreli koşu ayarları ve sistemi görmez,
  `smoke_shell`'de SGR 2 yok.
- Koyu temada SGR 2'li renklerin değerleri birkaç basamak kayar (phase-3,
  bilinçli).

## Akış

```
settings.toml / themes/*.toml ──vnode──▶ bt-shell yükleyici (ana kuyruk)
        ▲                                  │ oku → bt-core::settings::parse
        │ yerinde yaz                      │ fark(önceki, yeni)
Görünüm ▸ Tema ▸ ── bt-core biçim koruyan  ├─ tema    → Session::set_theme → kare iste
                    yazma                  ├─ görünüm → (system ise) light/dark seç ─┘
viewDidChangeEffectiveAppearance ──────────┤
                                           ├─ font    → Renderer font → refresh_geometry
                                           ├─ scrollback/osc52 → Config(tamamı) → set_options → kare iste
                                           └─ tanılar → alt başlık yuvaları → setSubtitle

okuyucu thread (Term kilidi): ColorRequest → tema (yaprak kilit)
                              ClipboardStore → Wake → atomik yuva → ana kuyruk → clipboard::copy
BT_RUN_SECONDS: yükleyici, izleme, görünüm, menü dolumu KAPALI → Theme::BATERI
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| phase-5 | ✅ |
| phase-6 | ✅ |
| phase-7 | |
| phase-8 | |
| kapı | |
