# Phase 3 — Odak: içi boş imleç ve duran blink

## Özet

Pencere odakta değilken caret'in **içi boşalsın** ve blink **dursun** —
görünür kalarak. Odak `bt-core`'a hiç girmesin.

_Requirements: R7, R7.1, R7.2, R7.3, R7.4, R5, R10_

> **phase-2'ye bağlı.** Kenar (`stroke`) yolu orada **sıfır kalınlıkla**
> iniyor; bu phase onu `rule_px`'e açıyor ve dolgu alfasını sıfırlıyor. Tek
> başına okunmaz.

## Değişiklikler

- **`crates/bt-shell/src/app.rs`** — `NSWindowDelegate`'e iki metot:
  `windowDidBecomeKey:` / `windowDidResignKey:`, ikisi de
  `DisplayLink::set_focused`'a iniyor. Emsal komşusu `windowDidChangeOcclusion`.
  - **R7.1'in yeri çağrı, varsayılan değil.** `focused: true` varsayılanı
    yetmez: `make duman` koşarken açılan bir Spotlight `windowDidResignKey:`
    doğurur, o da bir kare ister ve kapı bir makinede yeşil bir makinede
    kırmızı düşer. Hermetik koşuda çağrı **hiç yapılmıyor**; kapının biçimi
    `resolve_reduce_motion`'ın `Inputs`'a bakması.
- **`crates/bt-gpu/src/link.rs`** — `focused: Cell<bool>` (varsayılan `true`)
  ve `set_focused`.
  - Blink'in kapısı üçüncü terimini alıyor:
    `cursor.blink && !motion.reduce() && focused`. **Yeni mekanizma yok** —
    "kapalı blink görünür kalır" değişmezi (`content_frame`, `enabled=false` →
    `lit=true`, `next_flip=None`) bugün gizli imleci koruyor; bu onun **üçüncü
    tüketicisi**.
  - **Değişimin kendisi kare istiyor**, yalnız yarıda kalan animasyon değil —
    emsal ve gerekçe `set_reduce_motion`'ın `changed` terimi ve yorumu. Aynı
    değerde **no-op** (R7.2): açılıştaki `windowDidBecomeKey:` tam bu yola
    düşüyor ve bedava bir içerik karesi yazardı.
- **`crates/bt-gpu/src/frame.rs`** — içi boşluk `caret_rect`'in **ikinci**
  dikdörtgeninden geçiyor: odaksızda **opak iç boş dönüyor**.
  - `CursorBlock`'u besleyen **tek** şey o ikinci dikdörtgen olmalı. O zaman
    `cell.metal` hiç açılmıyor, ters çevirme kendiliğinden kapanıyor ve
    Karar 2'nin geçerlilik koşulu (`Cursor::text == tema zemini`) bu yolda hiç
    devreye girmiyor — içi boş imlecin altındaki harf **ön plan renginde**
    kalmalı, çünkü altında boyanmış bir zemin yok.
  - Bu R5'in tarifinin ta kendisi ("boyanan var, opak iç yok"); phase-2 imzayı
    hazırladı, bu phase ikinci dalı kullanıyor.
- **`crates/bt-gpu/shaders/cell_bg.metal`** — `caret_fragment`'in kenar yolu
  açılıyor: dolgu alfası 0, `strokeW` = `rule_px`. Yeni alan **yok**, phase-2
  hepsini tanımlamıştı.
- **İçi boşalma yalnız `Block`'a.** Underline ve beam zaten birer ince şerit;
  onların "içi boş" hâli bir pikselin çerçevesi, yani hiçbir şey. Odaksızlığın
  sinyali o şekillerde **blink'in durması**. Bilinen sınırlara yazılıyor —
  gözle kontrolde "beam odaksızda değişmedi" kusur sanılmasın. Referans bu
  noktada sessiz (`docs/ARASTIRMA.md` odağı izlediğini söylüyor, şekil başına
  davranışı değil).
- **Bayat iki cümle** (R10): `session.rs`'in "odak bugün sınırdan geçmiyor"u ve
  `Cursor::blink`'in doc'undaki "buradan geçen şey yalnız 'sönsün mü'" —
  ikincisi ikinci sahip doğunca yalan oluyor.
- **`CLAUDE.md`** — caret cümlelerine odak; **boşta sıfır kare** bölümüne
  kazanç: odaksız boş pencere saat **kurmuyor**.

## Kabul

- Odak gidince caret **görünür** kalıyor ve içi boşalıyor; blink duruyor.
  Odak dönünce blink kaldığı yerden değil **yanık** başlıyor.
- İçi boş imlecin altındaki harf **ön plan renginde** — ters çevirme kapalı.
- Odaksız pencerede `cursor_blink = "on"` olsa bile **saat kurulmuyor**
  (`next_flip()` `None`), yani pencere gerçekten boşta.
- Aynı odakta ikinci bir `set_focused` **kare istemiyor**.
- Hermetik koşu (`BT_RUN_SECONDS`) odağı hiç okumuyor: jetonlar değişmiyor ve
  koşu sırasında başka pencereye geçmek kapıyı kırmızıya düşürmüyor.
- Dock'ta da aynı: caret tek, yani dock'un caret'i de boşalıyor.

## Uygulama Notları

- **`caret_painted_rect` ayrıldı.** Boyanan dikdörtgen odağa **bağlı değil**
  (içi boş caret aynı yeri kaplıyor, yalnız içini boyamıyor), oysa tek
  fonksiyon kalsaydı `Caret::instance` ona sahte bir `hollow: false`
  geçirirdi. Şimdi odağa bağlı olan tek şey opak iç ve bu imzadan okunuyor.
- **`move_caret` odağı `caret_hollow`'dan geri türetiyor.** Hareket karesi
  `bt-shell`'e hiç gitmiyor; alan olmasaydı odaksız pencerede caret ilk
  kaymada dolardı — şeklin (`caret_shape`) birebir aynı gerekçesi.
- **`set_focused` ile blink'in `AND`'i birim sınamayla çivilenemedi:**
  ikisi de `DisplayLink` örneği istiyor ve sınamalarda hiç kurulmuyor
  (`DisplayLink::new` gerçek bir view/`CAMetalDisplayLink` istiyor).
  **Emsal ve sınır aynı:** `set_reduce_motion`'ın da sınaması yok. Çivilenen
  yarı `Frame` ve piksel düzeyinde — içi boşluğun kendisi, ters çevirmenin
  kalkması, ince şekillerin muafiyeti ve hareket karesinin koruması.
  Blink'in dayandığı değişmez (`enabled=false` → `lit=true`,
  `next_flip=None`) zaten `blink.rs`'te iki sınamayla çivili; odak onun
  **üçüncü tüketicisi**, yeni bir mekanizma değil.
- **Kenar kolu phase-2'de sınanmıştı** (`a_hollow_caret_paints_only_its_edge`,
  `/code-review` bulgusu). Bu phase onu üretim yolundan da bağladı
  (`an_unfocused_caret_paints_a_ring_through_the_production_path`): ikisi bir
  arada olmasa "kol çalışıyor ama odak onu hiç açmıyor" hâli sessiz kalırdı.
- **`.metal` değişmedi**, yani bu phase riskli değil: kenar kolu phase-2'de
  yazılmıştı, burada yalnız `stroke` sıfırdan `rule_px`'e çıkıyor.

## Yayın Etkisi

- **shader** — `.metal` değişti (kenar yolu açıldı): `make shader` koşmalı.
  Yeni uniform alanı yok, düzen sözleşmesi phase-2'den.
- **belge** — `CLAUDE.md` (caret + boşta sıfır kare), `session.rs` ve
  `Cursor::blink`'in bayat doc'ları.
- **bilinen sınır, üç madde:** underline ve beam odaksızda **şekil
  değiştirmiyor** (sinyalleri blink'in durması); odak `bt-core`'a girmediği
  için `frame()` sınırından okunamıyor, yani bir sınama odağı yalnız
  `bt-gpu` düzeyinde görebiliyor; ve **hücrenin kenarına mürekkep koyan bir
  glyph içi boş caret'in halkasının üstüne çiziliyor** — çizim sırasının
  gerekçesi (blok opak, altındaki harf ters çevrilmiş renkle) içi boş caret'te
  iki yarısıyla birden düşüyor. Bugün seyrek (kutu çizim henüz yok), 018'de
  görünür olacak; çaresi caret'i içi boşken glyph'lerden **sonra** encode
  etmek ve o yuva seçimini yeniden açıyor (`/code-review`).
- **ölçüm iddiası yok.** "Pil kazancı" ölçülmedi ve bu set kanca doğurmuyor;
  yön koddan kanıtlı (saat kurulmuyor), büyüklüğü değil.
- ayar şeması / tema / terminfo / app bundle / yeni bağımlılık: yok.
- **geri alma:** phase commit'ini revert; `focused` varsayılanı `true` olduğu
  için kalan kod bugünkü davranışa döner.

## Checklist

- [x] `app.rs`: `windowDidBecomeKey:` / `windowDidResignKey:` → `set_focused`
- [x] `app.rs`: hermetik koşuda **çağrı yapılmıyor** (R7.1, kapı çağrı yerinde)
- [x] `link.rs`: `focused` biti, blink kapısının üçüncü terimi
- [x] `link.rs`: `set_focused` değişimde kare istiyor, aynı değerde **no-op**
- [x] `frame.rs`: odaksızda `caret_rect`'in **opak içi boş**; `CursorBlock`'u
      besleyen tek şey o
- [x] `cell_bg.metal`: kenar yolu `rule_px` ile açık (shader phase-2'de yazıldı)
- [x] Test: odaksız glyph **ön plan renginde**
      (`a_hollow_caret_leaves_the_glyph_its_own_color`)
- [x] Test: halka örneği — üretim yolundan **ve** shader kolundan (ikisi ayrı)
- [~] Test: `set_focused` no-op — **`DisplayLink` sınamalarda kurulamıyor**;
      emsal `set_reduce_motion`, onun da sınaması yok (Uygulama Notları)
- [~] Test: odaksız blink — aynı sınır; dayandığı değişmez `blink.rs`'te
      iki sınamayla çivili ve odak onun üçüncü tüketicisi
- [x] `CLAUDE.md`, iki bayat doc
- [x] Doğrulama geçti (`make hepsi` exit 0 + `make shader` exit 0)
- [ ] `make duman` (kullanıcıda) — koşu sırasında başka pencereye geçilerek
- [ ] **Gözle kontrol:** odak gidince içi boşalma ve blink'in durması; dock'ta
      aynısı; beam/underline'da yalnız blink'in durması
- [x] Yayın etkisi yazıldı
