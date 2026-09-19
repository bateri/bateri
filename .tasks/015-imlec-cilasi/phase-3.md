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

<!-- Kodlanırken doldurulacak. -->

## Yayın Etkisi

- **shader** — `.metal` değişti (kenar yolu açıldı): `make shader` koşmalı.
  Yeni uniform alanı yok, düzen sözleşmesi phase-2'den.
- **belge** — `CLAUDE.md` (caret + boşta sıfır kare), `session.rs` ve
  `Cursor::blink`'in bayat doc'ları.
- **bilinen sınır, iki madde:** underline ve beam odaksızda **şekil
  değiştirmiyor** (sinyalleri blink'in durması); odak `bt-core`'a girmediği
  için `frame()` sınırından okunamıyor, yani bir sınama odağı yalnız
  `bt-gpu` düzeyinde görebiliyor.
- **ölçüm iddiası yok.** "Pil kazancı" ölçülmedi ve bu set kanca doğurmuyor;
  yön koddan kanıtlı (saat kurulmuyor), büyüklüğü değil.
- ayar şeması / tema / terminfo / app bundle / yeni bağımlılık: yok.
- **geri alma:** phase commit'ini revert; `focused` varsayılanı `true` olduğu
  için kalan kod bugünkü davranışa döner.

## Checklist

- [ ] `app.rs`: `windowDidBecomeKey:` / `windowDidResignKey:` → `set_focused`
- [ ] `app.rs`: hermetik koşuda **çağrı yapılmıyor** (R7.1, kapı çağrı yerinde)
- [ ] `link.rs`: `focused` biti, blink kapısının üçüncü terimi
- [ ] `link.rs`: `set_focused` değişimde kare istiyor, aynı değerde **no-op**
- [ ] `frame.rs`: odaksızda `caret_rect`'in **opak içi boş**; `CursorBlock`'u
      besleyen tek şey o
- [ ] `cell_bg.metal`: kenar yolu `rule_px` ile açık, dolgu alfası 0
- [ ] Test: odaksız glyph **ön plan renginde**
      (`glyph_under_the_cursor_takes_the_cursor_text_color`'ın kardeşi)
- [ ] Test: halka örneği — kenarda imleç rengi, ortada zemin
- [ ] Test: `set_focused` aynı değerde `requests` sayacını kımıldatmıyor
- [ ] Test: odaksız + `blink = on` → `next_flip()` `None`, saat kurulmuyor
- [ ] `CLAUDE.md`, iki bayat doc
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
- [ ] `make duman` (kullanıcıda) — koşu sırasında başka pencereye geçilerek
- [ ] **Gözle kontrol:** odak gidince içi boşalma ve blink'in durması; dock'ta
      aynısı; beam/underline'da yalnız blink'in durması
- [ ] Yayın etkisi yazıldı
