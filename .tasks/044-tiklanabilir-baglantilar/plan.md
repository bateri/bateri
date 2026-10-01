# Tıklanabilir bağlantılar

## Hedef

⌘ basılıyken fare bir URL'nin, var olan bir dosya yolunun ya da bir OSC 8
bağlantısının üstündeyse bağlantı alt çizgiyle vurgulanır ve imleç el olur;
⌘-tık onu açar — ızgarada, doldurma bandında ve dock'ta, fare kipinde de.
Gerekçeler `discussion.md` → Karar.

## Gereksinimler

- **R1** — Algılama: saf tarayıcı (`bt-core::link`) bir mantıksal satırda
  URL'leri (`http`, `https`, `ftp`, `mailto`, `file`) ve yol adaylarını
  bulur.
  - **R1.1** — Sondaki noktalama kırpılır, parantez/köşeli parantez dengesi
    korunur; `:satır`, `:satır:sütun`, `(satır,sütun)` soneki ayrılır.
  - **R1.2** — Sarılmış satırdaki bağlantı tek bağlantıdır (`search::wraps`,
    `WRAP_REACH`); geniş karakter ve emoji kümesi hücre ↔ karakter
    eşlemesini kaydırmaz.
- **R2** — OSC 8: bağlantı, aynı `Hyperlink`'in mantıksal satırdaki bitişik
  koşusu ve metin taramasını yener; `bateri://` bağlantı sayılmaz, açma
  yolunda `NSWorkspace`'e gitmeden yutulur.
- **R3** — Hit test: `Session::link_at(LinkPoint)` → aralık + ham hedef +
  tür + damga; `LinkPoint::Screen { row: i32, col }` (negatif = doldurma
  bandı, `drawn_lines` ile kapılı) ve `LinkPoint::Dock`. Uzak oturumda yol ve
  `file://` bağlantı değil; `file://`'nin yerel yetkisi boş, `localhost` ya
  da makine adı (`SessionOptions`, OSC 7 ile tek fonksiyon).
- **R4** — Vurgu: hover yuvası `frame()`'in iki sink'inde ve `Session::dock`'ta
  tek yardımcıyla `underline`'ı ezer (⌘: `Single`; ⌘'siz OSC 8 hover'ı:
  `Dashed`; renk metnin).
  - **R4.1** — Yuva `LedgerMark` (+ OSC 8'de `Hyperlink`, dock'ta aynanın
    nesli) damgalı; tutmazsa çizilmez, düşer ve ana kuyruğa haber gider.
  - **R4.2** — Aynı değerle kurmak no-op, uyandırmaz; hover yokken kare
    yolunun maliyeti tek dal; boşta sıfır kare korunur.
- **R5** — Çözümleme: `bt-shell-common`'da saf `resolve` (stat enjekte
  edilebilir; `~`, OSC 7 dizini, sonek) ve açma politikası tablosu; var
  olmayan yol bağlantı değil.
  - **R5.1** — Politika beyaz listeli: URL → varsayılan uygulama; dizin →
    Finder'da aç; bilinen içerik tipi ve `x` bitsiz dosya → varsayılan
    uygulama; kalan her dosya → Finder'da göster; OSC 8'in `http`/`https`/
    `ftp`/`mailto`/`file` dışı şeması → onay; `bateri://` → yut.
- **R6** — ⌘-tık arbitrajı (jest defteri): ⌘ + doğrulanmış bağlantı her kipte
  raporu ve Shift'i yener — `mouse_button` çağrılmaz, sürükleme hiçbir şey
  yapmaz, bırakma rapor göndermez; açma bırakmada, basışta kilitlenen aralığın
  üstündeyse ve çift tıklamada bir kez.
- **R7** — AppKit: `flagsChanged:`, ⌘'nin her harekette yeniden okunması,
  key'lik ve uygulama deaktivasyonunda temizleme, el imlecinin yükleme
  düğmeleriyle tek cursor-rect listesinde olması, yol doğrulamasının arka
  plan kuyruğunda olması ve onay sayfası.
- **R8** — Dock yüzeyi: dock'un giriş satırındaki bağlantı aynı tarayıcıdan,
  isabet `dock_layout` + son çizilen pencereden (`DockWindow`).
- **R9** — Ek yüzeyler: ⌘'siz OSC 8 hover'ında kesikli alt çizgi, ⌘ ile OSC 8
  üstündeyken hedefin sol alttaki etiketi, bağlantı üstünde sağ tık menüsü
  (fare kipi kapalıyken).

## Yaklaşım

1. `bt-core`: tarayıcı ve hit test (R1–R3) — saf, sınanır, kare yoluna
   girmez.
2. `bt-core`: hover yuvası ve alt çizgi ezmesi (R4) — kare yolunun tek
   değişikliği; paylaşılan durum.
3. `bt-shell-common`: çözümleme, açma politikası ve jest defterinin ön-rotası
   (R5, R6) — saf, Linux'ta da sınanır.
4. `bt-shell-macos`: ızgara ve bant için uçtan uca bağlama (R7) — ⌘-hover,
   ⌘-tık, açma, onay; sözleşme `CLAUDE.md`'ye.
5. Dock yüzeyi (R8).
6. Ek yüzeyler (R9).

## Kapsam Dışı

- `satır:sütun`'a atlama (editör komutu ayarı; iTerm2'nin semantic history
  komutu) — dosya açılır, satır atlanmaz.
- Aynı OSC 8 id'sini taşıyan **bitişik olmayan** hücrelerin birlikte
  vurgulanması.
- Bağlantısız yerde genel bir sağ tık menüsü.
- Ayar anahtarı (bağlantıları kapatmak, değiştirici tuşu seçmek).
- Göreli yolun satırın basıldığı andaki dizine çözülmesi (bugünkü OSC 7
  dizinine çözülür; `discussion.md` → Karar 5, bilinen sınır).
- Doldurma bandında süre sayacı ve seçim (değişmiyor).

## Akış

```
mouseMoved / flagsChanged (⌘ basılı, hücre değişti)
  → Session::link_at(LinkPoint)            [bt-core, Term kilidi, saf tarama]
      → None ─────────────────────────────→ hover temizle
      → URL / OSC 8 → set_link_hover(aralık, damga) → wake → frame() alt çizgi
      → yol adayı → arka plan kuyruğu: links::resolve (stat)
            → ana kuyruk: aday hâlâ aynı mı? → set_link_hover
frame(): damga tutmuyor → çizme, düşür, Wake → view: ⌘ basılıysa yeniden link_at

mouseDown (⌘ + doğrulanmış hover'ın aralığında)
  → Gesture::pressed_link  (mouse_button çağrılmaz; sürükleme: Ignore)
mouseUp → Release::Link → kilitli aralığın üstünde ve clickCount == 1
  → links::action (politika) → NSWorkspace aç | Finder'da göster | onay | yut
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | |
| phase-5 | |
| phase-6 | |
| kapı | |
