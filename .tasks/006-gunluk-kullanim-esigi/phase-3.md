# Phase 3 — Kaydırma: tekerlek + Shift+PgUp

## Özet

Tekerlek ve Shift+PgUp geçmişe kaydırır; alternate screen'de tekerlek yoksayılır.

_Requirements: R3, R3.1, R3.2, R3.3, R6.1, R6.2_

---

## 1. İnce `scroll_display` API'si

`crates/bt-core/src/session.rs` — viewport kayması icat edilmiyor:
`display_offset` zaten `frame()`'de tüketiliyor (`session.rs:603-617`,
`offset` hesabı ve imleç görünürlüğü orada). Eksik olan yalnız onu hareket
ettiren ince bir API + kaydırınca kirli bayrağını dikmek.

```rust
/// Görünen pencereyi kaydırır; artı değer geriye. Kaydırınca kirli bayrağı
/// dikilir, yoksa kaydırma hiç boyanmaz (seçimdeki R1.2 ile aynı tuzak).
```

## 2. İki tetikleyici, çubuk yok

`crates/bt-shell/src/view.rs` — `scrollWheel:` + Shift+PgUp (tuş yolu
`encode_key` üzerinden, AppKit fonksiyon tuşu `F1/Home/PageUp…` aralığında
yutuluyor — `keys.rs`'te PgUp dizisi tanımlı olmalı, yoksa eklenir).
Kaydırma çubuğu **yok**: AppKit kroniği (thumb, orantı, sürükleme), eşik için
gerekli değil.

## 3. Alternate screen'de yoksayma — karar `bt-core`'da

Tekerlek vim/less/tmux'ta uygulamaya fare dizisi göndermelidir — ama fare
raporlaması (SGR-pixel) ayrı bir iş ve bu sette yok. O yüzden bu sette
tekerlek alternate screen'de **yoksayılır**. Karar `bt-core`'da `Term`
kipine bakılarak verilir (katman korunur); `view.rs` körü körüne
kaydırmaz.

---

## Uygulama Notları

- **API: `scroll_display(lines) -> Option<i32>`** — kılavuzun "ince API"si,
  ama dönüşü iki soruyu ayırıyor: `None` = kip reddetti (alternate screen),
  `Some(n)` = pencere `n` satır kaydı (uçta `0`). Kare **yalnız `n != 0`**
  iken isteniyor; "kaydırınca kirli bayrağı dikilir" cümlesinin boşta sıfır
  kareye uyan hâli bu — trackpad momentumu geçmişin ucunda da olay yağdırıyor.
  Kirli bayrağı **ve uyandırma** elle (devir maddesi): `Term::scroll_display`'in
  tek olayı `MouseCursorDirty` ve `Adapter` onu yutuyor. Bayrak + uyandırma
  artık seçim ve kaydırma yollarının ortak `request_frame()`'i ve o,
  `Adapter`'ın `Wakeup` kolunu **çağırıyor** (sıra tek yerde); `resize` onu
  kullanmıyor, uyandırması `bt-shell`'in. İkinci API `scroll_page(pages)`:
  sayfa = `Term`'in görünen satır sayısı, gövde (kip kapısı, kırpma, kare
  talebi) `scroll_by`'da ortak.
- **Alternate screen kapısı davranışta görünmez — ayrım bu yüzden tipte.**
  alacritty alternate grid'i geçmişsiz kuruyor (`Grid::new(.., 0)`), yani kip
  kapısı silinse de ofset oynamaz, kare istenmezdi; yalnız ofsete/kareye bakan
  bir sınama o silinmeye **yeşil** kalırdı. `alternate_screen_ignores_scroll`
  bu yüzden `None`'u soruyor: kapısız taslakta `Some(0)` döndü ve kırmızı düştü.
- **Kırpma (sapma, kılavuzda yok):** alacritty ofseti `i32`'de topluyor
  (`offset + count`); `bt-shell` deltayı `f64`'ten doyurarak çevirdiği için
  `i32::MAX` ulaşılabilir ve kırpılmamış hâli debug'da **panik** (mutasyonla
  görüldü: `attempt to add with overflow`, alacritty `grid/mod.rs:166`).
  Delta geçmişin boyuna kırpılıyor.
- **phase-1 devri kapandı — çapa `bt-core`'a taşındı (sapma).** View çapayı
  pencere hücresi olarak tutup her sürüklemede `set_selection(çapa, uç)`
  kuruyordu; kaydırma ortada olunca aynı satır numarası başka içeriği
  gösteriyordu (sınamada `30`'dan başlayan sürükleme `27`'den başladı).
  Çapayı view'da kaydırmayla birlikte kaydırmak da işlemezdi: ileri kaydırmada
  satır negatife düşer, `SelectionPoint.row` `u16`. Çözüm yeni
  `Session::update_selection(end)`: basış `set_selection(a, a)` ile seçimi
  kurar, sürükleme yalnız bitişi taşır (`Selection::update`) ve başlangıç
  alacritty'nin grid-mutlak ucunda kalır — hem viewport kaymasını hem
  çıktının içeriği itmesini (`rotate`) kendiliğinden taşıyor. View'ın
  `anchor` ivar'ı `dragging: bool`'a indi; `cell_under`/`drag_cells`
  yardımcıları gitti. Basılı sürüklemede tekerlek dönerse uç, farenin **yeni**
  altındaki hücreye taşınıyor (fare kıpırdamadı ama içerik değişti) — tuşu
  basılı tutup geçmişe inmek seçimi oraya uzatıyor.
- **Tekerlek çevirisi (`wheel_lines`, saf):** trackpad (`hasPreciseScrollingDeltas`)
  nokta verir, birim hücre boyu (fiziksel ölçü ÷ ölçek); klasik tekerlek
  zaten satır verir, birim 1. Hücre boyundan küçük deltaların artığı
  `scroll_carry`'de taşınıyor, yoksa yavaş trackpad kaydırması hiç satır
  üretmezdi. Çarpan yok (alacritty 3 kullanıyor) — ölçülmemiş bir hız
  seçimi yazmamak için AppKit'in verdiği birim aynen; göz kontrolü yavaş
  bulursa ayar setinin (007) konusu.
- **Shift+PgUp:** karar saf `keys::page_scroll`'da (±1 sayfa), sayfanın boyu
  `bt-core`'da (`scroll_page`). Düz PgUp/PgDn artık
  yutulmuyor: `\e[5~`/`\e[6~` (`xterm-256color`'ın `kpp`/`knp`'si, `TERM`
  oynamadı). **Sapma:** alternate screen'de kaydırma reddedilince Shift+PgUp
  yutulmuyor, uygulamaya düz PgUp olarak düşüyor (less/vim'de sayfa çevirir;
  alacritty'nin `~Alt` bağlamasıyla aynı sonuç). `None`/`Some(0)` ayrımının
  üretimdeki tüketicisi de bu dal.
- **`/simplify` (4 mercek):** sayfa boyu view'ın ölçü önbelleğinden
  `bt-core`'a indi (`scroll_page`; altitude + reuse, ikisi de buldu) — ölçü
  yokken Shift+PgUp'ın birincil ekranda sessizce uygulamaya düşmesi de
  onunla kapandı; `anchor` ucu doğrudan `SelectionPoint` olarak alıyor
  (ofset + `viewport_point` reçetesi üç yerden bire); `update_selection`
  seçim yoksa ucu çözmeden dönüyor; `request_frame` `Wakeup` kolunu
  çağırıyor; sınamalarda `wait_until` + `wait_settled` yardımcıları
  (`wait_bracketed_mode` ve `paste_empty_writes_nothing`'in durulma döngüsü
  onlara indi); view'da iç içe `if`'ler let-chain/koruma koluna.
  **Atlananlar:** (1) kaydıramayan tekerlek olayı (uçta ya da alternate
  screen'de) yine `Term` kilidini alıyor — efficiency önerisi atomik bir
  "dipte/alt ekran" önbelleğiydi; atlandı, çünkü sürükleme yolu
  (`set_selection`/`update_selection`) olay başına aynı kilidi phase-1'den
  beri alıyor ve önbellek kilidin dışında ikinci bir doğruluk kaynağı
  doğururdu. (2) Sürüklemede kaydırma olay başına iki kilit + iki
  `request_frame` — tek çağrılık birleşik API önerisi; yol yalnız tuş basılıyken
  tekerlek dönerken koşuyor, uyandırmalar zaten tek kareye birleşiyor.
  (3) İki uçlu `set_selection`'ı tek noktalı `start_selection`'a indirmek —
  ~30 sınama çağrı yerine dokunur, bu phase'in kapsamı değil. (4) `row_text`
  ile `glyph_text`'in birleşmesi — sınama yardımcısı, kazancı yok.
- **Sapma (kılavuzda yok): girdi pencereyi dibe döndürüyor.** İlk taslakta
  "bilinen sınır" diye not düşülmüştü; `/code-review` gerçek oturumun ilk
  çarptığı şey olduğunu gösterdi ve kaynağa bakınca doğrulandı: alacritty
  kaydırılmış pencereyi yeni çıktıya karşı sabitliyor (`grid.scroll_up` ofseti
  artırıyor), yani geçmişte yazılan `ls` + Enter'in çıktısı görünmez kalıyor;
  `swap_alt` grid'i bütün olarak takas ettiği için geçmişte başlatılan `vim`
  kapanınca istem de hâlâ görünmez. R3 bununla çelişmiyor (planın boşluğu,
  değişikliği değil). `write` artık `write_owned`'dan geçiyor ve kullanıcı
  girdisinin tek gönderim noktası orada boş olmayan girdide `scroll_by`'ı
  dibe çağırıyor; `Adapter::reply` (uygulamaya yanıt) bu kapının dışında.
  Bedel girdi başına bir `Term` kilidi (sürükleme yolunun olay başına
  ödediği). "Kaydırılmış mı" atomik önbelleği **kurulmadı**: alternate
  screen'de alt grid'in ofseti okunur ve birincil hâlâ kaydırılmışken
  önbellek "dipte" derdi. Sınama `input_returns_the_view_to_the_bottom`
  (dönüşten önce kırmızıydı; boş girdinin pencereye dokunmadığını da soruyor).
- **`/code-review` (15 bulgu).** **Giderilen:**
  - Sınama hazırlığı yarışıyordu: `history_session` "0. sütunda `3`, 1.
    sütunda `0`" diye iki bağımsız soruyla `3` ve `10` ekrandayken dönebilirdi;
    ölçüt artık `row_text(.., 8) == "30"` (`wait_seq_tail`) ve aynı kusurlu
    ölçütü taşıyan `selection_scrolled_into_history_is_not_drawn` da ona indi.
  - `wait_settled` ilk `None`'da dönüyordu (kendi doc'uyla çelişik); artık
    arada 20 ms olan **iki** ardışık `None` istiyor. Çıplak `while frame()`
    döngüleri de ona indi.
  - Kırpma `[-geçmiş, geçmiş]` değil **ulaşılabilir aralık**
    `[-ofset, geçmiş - ofset]`: toplam `[0, geçmiş]`'ten çıkamaz.
  - Uyandırma yarısı sınanmıyordu: `frame()` bayrağı doğrudan okuduğu için
    bayrağı diken ama uyandırmayı unutan bir `request_frame` yeşil geçerdi.
    Sınama artık uyandırma sayısını da soruyor (mutasyonla kırmızı görüldü);
    kaymayan kaydırmanın uyandırmadığını da.
  - Bayat `dragging`: `mouseUp:` view'a varmazsa tuşsuz her kaydırma eski
    seçimi uzatırdı. Kaydırmanın seçimi izlemesi artık
    `NSEvent::pressedMouseButtons()`'a da soruyor, bayat bayrağı orada indiriyor.
  - Shift+PgUp basılı sürüklemede seçimin ucunu taşımıyordu, tekerlek
    taşıyordu. İki tetikleyici tek yoldan geçiyor (`follow_pointer`, fare
    konumu `mouseLocationOutsideOfEventStream`'den — tuş olayının konumu yok).
  - `scroll_carry` yalnız reddedilen kaydırmada sıfırlanıyordu; artık geçmişin
    ucunda (`Some(0)`) ve yeni jestin başında (`NSEventPhase::Began`) da.
  - `encode_key`'in PgUp/PgDn kolları ilk karaktere bakıyordu, `page_scroll`
    tüm dizgiye; iki kol da artık `single` korumalı (Control kolu aynı
    korumayı adıyla paylaşıyor). Okların kolları aynı biçimi 006'dan önce
    taşıyordu, dokunulmadı.
  - Modül doc'unun kilit listesi `scroll_page`/`scroll_by`'ı ve girdinin dibe
    dönüşünü saymıyordu.
  - `update_selection`'da çift kontrol (`is_none()` + `if let`): tek
    `let … else` oldu — `/simplify`'ın "önce seçim var mı" önerisini geri
    çeviriyor; ucu çözmenin bedeli nadir yolda mikro, çift kontrol ise iki
    doğruluk kaynağı.
  **Atlanan:** kaydıramayan tekerlek olayının `Term` kilidi — `/simplify`'daki
  (1) ile aynı bulgu, aynı gerekçe.
- **WAIVE önerisi — alternate screen'de tekerlek → ok tuşları (DECSET 1007).**
  `/code-review`: `TermMode::ALTERNATE_SCROLL` alacritty'de varsayılan açık ve
  fare raporlaması gerektirmiyor; `man`, `git log`, `less` tekerlekle
  kaydırılamıyor. Sınanmış taslak: `scroll_by`'ın alternate screen kolunda
  `ALTERNATE_SCROLL && !MOUSE_MODE` ise satır başına `\e[A`/`\e[B`
  (`APP_CURSOR`'da `\eOA`/`\eOB`) `send` (~20 satır). **Uygulanmadı**, çünkü
  R3.3 ve `discussion.md` → Karar 4 "alternate screen'de tekerlek
  **yoksayılır**" diyor — değiştirmek bir gereksinim değişikliği, kararı
  orkestratörün/kullanıcının. Uygulanırsa Shift+PgUp'ın alternate screen'deki
  düşüşü de yeniden düşünülmeli (bugün `None`'a bağlı).

- **`/audit`:** mercek 1 (katman — `cargo tree` + kaynak grep), 2 (`Cargo.toml`/
  `Cargo.lock` el değmedi), 3 (yeni `expect`'lerin hepsi `#[cfg(test)]`'te),
  6 (ölçüm sayısı yok) temiz; 4 (ayar), 5 (shell), 9 (hücre/shader) ilgisiz.
  7, 8, 10 üç ayrı ajana dağıtıldı:
  - **7 (thread/blokaj) temiz:** dört çağrı yerinin hepsi uyandırmayı
    `Term` kilidi düştükten sonra yapıyor; `Term::scroll_display`'in tek olayı
    yutulduğu için kilide geri giriş yok; yeni yollar `size` kilidine hiç
    dokunmuyor. **Gözlem:** girdi başına kilit, yoğun çıktı altında ana
    thread'i okuyucunun bir ayrıştırma turu kadar bekletebilir — `frame()` ve
    sürüklemenin zaten kabul ettiği sınıf, yeni bir sınıf değil.
  - **8 (boşta sıfır kare) temiz:** kare yalnız ofset ya da çizili aralık
    değişince isteniyor; uçta momentum, alternate screen, dipteyken tuş, boş
    yapıştırma kare istemiyor; zamanlayıcı/animasyon eklenmedi; duman koşusu
    bu yolların hiçbirinden geçmiyor. **Bilinen sınır (açığa çıkan, getirilen
    değil):** geçmişe kaydırılmış pencerede akan çıktı ekranda hiçbir şey
    değiştirmediği hâlde her `Wakeup`'ta kare ister — `dirty` pencereyi
    bilmiyor. Kaydırma gelene kadar ulaşılamazdı; çaresi pencereye duyarlı
    hasar, bu setin işi değil. `AdapterInner::dirty` doc'una yazıldı.
  - **10 (belge/üslup):** sekiz belge kayması giderildi — `write`/`paste`
    doc'ları dibe dönüşü ve kilidi söylemiyordu, `paste` "`write`'ın kapısı"
    diyordu (artık `write_owned`), `FUNCTION_KEYS` ve `keys` modül doc'u
    PgUp/PgDn'i ve kaydırma kararını saymıyordu, `view` modül doc'u olay
    sayısında ve "yalnız aritmetik" iddiasında yanlıştı, `point_to_cell`
    silinen çağıranları sayıyordu, `SelectionPoint` indirmeyi yalnız
    `set_selection`'a veriyordu, bu dosyadaki devir maddesi silinen
    `ViewIvars::anchor`'a işaret ediyordu, çizili aralık kapısının gerekçesi
    sürüklemenin artık `update_selection`'dan geçtiğini söylemiyordu (sınama
    da `update_selection`'ın kapısını sormuyordu — iki satır eklendi).
    `bt-shell` crate doc'u fareyi saymıyordu, eklendi. **Dokunulmadı:**
    `CLAUDE.md`'nin `bt-shell` satırı da fareyi saymıyor — çelişki değil
    eksik ve phase-1'den beri öyle.

## Yayın Etkisi

- Tekerlek/trackpad ve Shift+PgUp artık görünen pencereyi geçmişe kaydırır;
  alternate screen'de tekerlek susar, Shift+PgUp uygulamaya düz PgUp olarak
  düşer. Düz PgUp/PgDn artık yutulmuyor, `\e[5~`/`\e[6~` gönderiyor
  (`xterm-256color`'ın `kpp`/`knp`'si — **`TERM` ve terminfo değişmedi**).
- **Davranış değişikliği:** geçmişe bakarken klavye girdisi ve yapıştırma
  pencereyi dibe döndürür.
- Basılı sürüklemede kaydırma seçimi geçmişe uzatır (phase-1 devri kapandı).
- Yeni bağımlılık yok (`Cargo.toml`/`Cargo.lock` el değmedi). Ayar şeması,
  tema, shell entegrasyonu, `.metal`, bundle: el değmiyor.
- Ölçüm bekleyen iddia yok — performans iddiası yazılmadı. Girdi başına bir
  `Term` kilidinin ve kaydırılmış pencerede akan çıktının kare maliyeti
  niteliksel olarak kayıtlı (yukarıda); sayı istenirse `/measure`'ın işi.
- `CLAUDE.md` / `docs/MIMARI.md`: güncelleme gerekmedi (kaydırma sözleşmede
  zaten `bt-core` + `bt-shell` sorumluluğu; `docs/MIMARI.md` yok).

---

## Checklist

- [x] **(phase-1'den devir)** Sürükleme **basılıyken** kaydırma olursa view'daki
      çapa bayat kalır: `set_selection` aralığı grid mutlağında tutuyor ve
      alacritty döndürmesi onu içerikle taşıyor, ama `ViewIvars::anchor`
      viewport cinsinden. Kaydırma tetikleyicisi phase-1'de yoktu, bu phase'de
      geliyor — göz kontrolü bu hâli de kapsamalı — **kapandı:** çapa
      `bt-core`'a taşındı (`update_selection`), `ViewIvars::anchor` ve
      `mouseUp:`'taki kayıt kalktı; sınama `drag_anchor_survives_a_scroll`,
      gerekçe `## Uygulama Notları`. Göz kontrolü aşağıdaki `[elle]`'de
- [x] `scroll_display` API'si + kaydırınca kirli bayrağı (+ `scroll_page`; kare yalnız ofset değişince)
- [x] **Devir (seçim yarısı düzeltmesinin `/audit`'i):** kirli bayrağı **elle**
      dikilmeli — alacritty'nin `Term::scroll_display`'i yalnız
      `Event::MouseCursorDirty` gönderiyor ve bizim `Adapter` onu yutuyor;
      bayrak dikilmezse kaydırma hiç çizilmez. Seçim kapısı (`visible_range`)
      iki tarafı aynı `display_offset`'le hesaplıyor, yani kaydırmanın kendi
      karesini kendisi istemesi yeterli; kapıya dokunmak gerekmiyor — bayrak
      **ve uyandırma** elle (`request_frame`), kapıya dokunulmadı
- [x] `scrollWheel:` ve Shift+PgUp tetikleyicileri; çubuk yok
- [x] Alternate screen'de yoksayma `bt-core` kipiyle (`None`; ayrım tipte, gerekçe notlarda)
- [x] Test: kaydırma `display_offset`'i oynatıyor + kirli dikiliyor (`scroll_moves_the_display_offset_and_marks_dirty` — uyandırmayı ve uçta sessizliği de soruyor; kirli bayrağı ve uyandırma mutasyonla kırmızı)
- [x] Test: alternate screen'de tekerlek yoksayılıyor (`alternate_screen_ignores_scroll` — kapısız taslakta `Some(0)` ile kırmızı)
- [ ] `[elle]` göz kontrolü: tekerlekle geçmişe git, alternate screen'de sus — ayrıca: trackpad'le yavaş kaydırma satır üretiyor mu; tuşu basılı tutup tekerlek/Shift+PgUp ile geçmişe inince seçim uzuyor mu (phase-1 devri); geçmişe bakarken yaz → dibe dönüyor mu; `less`'te Shift+PgUp sayfa çeviriyor mu
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**) — kapı sonrası son hâlde: `make hepsi` exit 0 · `make duman` exit 0 (`kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=2 kapanis=clean profil=debug ornek=off pipeline=ok`) · `make test-yaris` exit 0 (yeni `Term` kilidi yerleri ve girdi yolunda kilit → gerekli)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (4 mercek; atlananlar gerekçesiyle notlarda)
- [x] `/code-review` çalıştırıldı, bulgular giderildi (15 bulgu; DECSET 1007 waive önerisi notlarda)
- [x] `/audit` çalıştırıldı, bulgular giderildi (7/8/10 ajanla; 8'in bilinen sınırı notlarda)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
