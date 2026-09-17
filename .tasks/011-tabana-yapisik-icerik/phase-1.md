# Phase 1 — Tabana yapışık içerik

## Özet

İçerik pencerenin tabanına yaslansın: `frame()` doluluk sayısını üretsin,
çizim `setViewport` ile kaysın, fare aynı ofseti okusun — **animasyonsuz**.

_Requirements: R1.1, R1.2, R1.3, R1.4_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `frame()` döngüde `content_rows`
  toplar: atlama kapısından geçen en büyük `row` ile `cursor_row`'un
  maksimumu, artı bir. **İkisi birden gerekli** — imleç tek başına yetmez
  (imleci yukarı taşıyan ilerleme çubuğu içeriği aşağı iterdi), kapı tek
  başına yetmez (boş prompt satırı kapıdan geçmiyor). Alternatif ekranda
  değer ızgaranın tamamı olur (ofset 0); bayrak `alt_screen` olarak zaten
  kilidin altında okunuyor. Değer `Cursor`'la döner — `bt-core` **piksel
  değil sayı** veriyor.
- **`crates/bt-gpu/src/link.rs`** — `frame()` **döndükten sonra** origin'i
  kurar (`content_rows` → satır → piksel). Origin'in **tek sahibi** burası;
  `view` onu buradan okur. Hasarlı kolda yazılır; hasarsız kolda bu phase'de
  değişmiyor (phase-2 ikinci yazma noktasını açacak).
- **`crates/bt-gpu/src/renderer.rs`** — `encode_pass` `setViewport`'u
  `originY` ile kurar. **İki pipeline birden** kayar (`cell_bg` + `cell`),
  yani arka plan, glyph, kural, şerit ve imleç tek yerden. `.metal`
  dosyalarına **dokunulmuyor**.
- **`crates/bt-gpu/src/frame.rs`** — `CursorBlock.rect`'e origin **CPU'da**
  eklenir: `[[position]]` viewport dönüşümünden **sonraki** koordinat, yani
  rect kaydırılmazsa imlecin altındaki metnin rengi eski satırda kalır.
  `pos_at` ve `push_block` **değişmez** — payın tek sahibi ve `push_block`'un
  `pos_at`'i bilerek atlaması korunur.
- **`crates/bt-shell/src/view.rs`** — `point_to_cell` origin'i okur ve
  **`f64`'te** çıkarır. Boş alan **üstte**: `u16`'da çıkarma oraya yapılan
  tıklamada taşar. Resize zamanı önbelleği (`metrics`) origin **taşımaz** —
  origin kare başına değişiyor, o önbellek yalnız pencere olaylarında
  tazeleniyor.
  - **Tesisat kararı, adıyla konur:** `view`'ın bugün `link`'e erişimi **yok**
    (`app.rs:547` ikisini de sahipleniyor, `view`'a değerler `set_metrics` ile
    itiliyor). Bu phase **paylaşılan bir ana-thread hücresi** kurar
    (`Rc<Cell<…>>` ya da eşdeğeri): yazan kare yolu, okuyan fare yolu, ikisi de
    ana thread. **`Arc<AtomicU32>`'ye kaçılmaz** — atomik gerekmiyor, ve
    gerekiyormuş gibi yazmak `make test-yaris` tetiğini ve "riskli phase"
    etiketini geri getirir (`discussion.md` → Karar 4 eki).

## Kabul

- Boş kabukta tek satırlık içerik pencerenin **dibinde**; üstünde boşluk.
- Ekran dolduktan sonra görünüm bugünküyle aynı (ofset 0'a iner).
- `vim`/`htop` açıkken ofset **0**: ızgaranın tamamı kullanılıyor.
- **Geçmişte kaydırırken içerik tabana yapışık kalır.** `content_rows`
  **görünür** satırlardan doğuyor, yani `display_offset > 0` iken de kural
  aynı: 99 satır geçmişi olan ve `clear`'lanmış bir pencerede tekerleğin ilk
  çentiği iki satırlık içerik gösterir ve ikisi **dipte** durur; kaydırma
  ilerledikçe pencere dolar ve ofset kendiliğinden 0'a iner. Alternatifi
  ("ofset `display_offset > 0` iken 0'a donar") **reddedildi**: tekerleğe
  dokunur dokunmaz içeriğin tavana sıçraması demekti.
- Tıklama ve sürükleme doğru hücreyi seçiyor; üstteki boş alana tıklamak
  taşmıyor.
- **Üç bekçi yeşil** (aşağıda).
- `make duman` yeşil; `icerik` jetonu **oynamıyor** — ofset çizim zamanı, yani
  yeni içerik karesi doğurmuyor.

### Bekçiler

Bekçiler **CPU-CPU eşitliği değil CPU→GPU dikişi** ölçer: ofset GPU'da
(`setViewport`), yani iki CPU listesini birbirine karşı ölçen bir sınama
inşa gereği doğru olan bir şeyi sınar (`discussion.md` → Muhakeme 2. tur,
kabul 4).

1. **`cell_bg` pipeline'ı** — offscreen render + origin `k` satır; boyanan
   satır okunur ve `k` kadar kaymış olmalı. Emsal
   `cell_bg_paints_pixels_on_the_gpu`.
2. **`cell` pipeline'ı** — aynı kurulum glyph/kural için; **iki pipeline'ın
   aynı miktarda kayması** bu setin asıl riski (birinin unutulması
   `make hepsi`'yi yeşil bırakırdı).
3. **İmleç** — `push_cursor`'ın ürettiği `rect.y` ile aynı satıra basılan
   hücrenin `pos.y`'si eşit. Karar 7'nin üç kör noktasından bugün bekçisi
   olmayan üçüncüsü bu.

## Yayın Etkisi

- **shader:** `.metal` **değişmiyor** (`setViewport` yolu), `make shader`
  gerekmiyor ve `#[repr(C)]` ↔ MSL düzeni dokunulmadan kalıyor.
  **Kanarya şartı:** Metal'in drawable'ı aşan viewport'u scissor'la kırptığı
  doğrulanmalı. Tutmazsa vertex uniform yoluna dönülür ve o zaman `make shader`
  **girer**, phase **riskli** olur, `#[repr(C)]` denetimi yapılır — kalan
  kalemler (rect'in CPU'da kaydırılması, ikinci yazma noktası) **aynı**.
- **Belge:** `docs/YOL-HARITASI.md` **bu phase'in işi değil** — set açılırken
  (`/rfc`, 2. tur) yeniden yazıldı: 011 satırı yeni kapsamıyla, dock için 012
  satırı, üçüncü numara kayması ve iki bilinen hatanın düzeltmesi
  (a: "kirli satır takibi devre dışı kalır" — devre dışı kalacak bir satır
  takibi yok; b: "010'un açık kalemini bu kapatıyor" — o kalem 010'un kendi
  son commit'inde kapandı) orada indi. Phase yalnız `CLAUDE.md`'ye dokunur.
- **`CLAUDE.md`:** "Bugünkü hâl" paragrafı içeriğin tabana yaslandığını söyler.
- terminfo/`TERM`, ayar şeması, tema biçimi, shell entegrasyonu, app bundle:
  **yok**.
- Yeni bağımlılık: **yok**.
- Ölçüm bekleyen iddia: **yok** (bu phase animasyonsuz; kayma phase-2'de).

## Checklist

- [ ] `content_rows` `frame()`'de toplanıyor, alt ekranda 0
- [ ] Origin `DisplayLink`'te, `setViewport` iki pipeline'ı kaydırıyor
- [ ] `CursorBlock.rect` CPU'da kaydırılıyor; `pos_at`/`push_block` değişmedi
- [ ] `point_to_cell` origin'i `DisplayLink`'ten `f64`'te okuyor
- [ ] Test: üç bekçi (iki pipeline dikişi + imleç eşitliği)
- [ ] `setViewport` kanaryası doğrulandı; tutmadıysa uniform yoluna dönüldü
      ve phase riskli işaretlendi
- [ ] `CLAUDE.md`'nin "Bugünkü hâl" paragrafı içeriğin tabana yaslandığını
      söylüyor (yol haritası set açılırken güncellendi, burada iş yok)
- [ ] Doğrulama geçti (`make hepsi` + `make duman`; uniform yoluna dönüldüyse
      ayrıca `make shader`)
- [ ] Yayın etkisi yazıldı
