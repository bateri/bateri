# Uzak sunucunun yük göstergesi — Bağlam

## Mevcut Durum

**Durum çubuğu.** ssh/mosh oturumunda dock tek satırlık bir durum çubuğuna
iniyor (036; sözleşme `CLAUDE.md` → "ssh'ta dock bir durum çubuğuna iniyor"):
bağlam satırı `⇄ host  /uzak/yol`, host işaretin renginde, yol iki kademede,
satırın sağı **boş**. Satırın yerleşimi `bt-core`'da,
`dock::render_remote_context` (`crates/bt-core/src/dock.rs`): bütçe önce
`⇄ host`'a, kalanı yola; yol `path_cells` ile soldan `…`'la kısalıyor, host
asla. Satır küçük boy sınıfında çiziliyor (`CONTEXT_SCALE`, sütun adımı
`context_cell_px`; bütçe `DockCols::context`). Aktarım sürerken satırın
yerini aktarım satırı alıyor (`DockContext::transfer`, `render_transfer`);
onun sağa yaslı düğmeleri çizim ve fare için tek yerleşimden okunuyor
(`transfer_layout` → `transfer_button_at`/`transfer_button_span`), el imleci
`BateriView::hand_cursor_rects`'in cursor-rect listesinden, liste popover'ı
`uploader::toggle_upload_list`'ten (`NSPopover`, `transient`, Esc'i yerel
olay izleyicisi yutuyor, aynı düğmeye ikinci basış `popoverWillClose:`'un
olay zamanıyla ayırt ediliyor; delegate `TerminalPane`).

**Uzak veri kanalı.** 045'in pane başına yardımcı ssh oturumu
(`bt-shell-common::remote_helper`; gerekçe `.tasks/045-uzak-dosya-indirme/discussion.md`
→ Karar 10): `upload::ssh_argv` (`BatchMode=yes`) + `remote_files::helper_script`,
uzakta stdin'den **satır satır `eval` eden seri bir `sh` döngüsü**. İstek
`request_line` (`bt_stat`/`bt_count {seq} …`), cevap `BT-R {seq}` …
`BT-END {seq}` arasında; `parse_reply` o aralıkta protokol dışı satırı
`Malformed` sayıyor. Worker tek thread, istekler sırayla; oturum ilk soruda
açılıyor, nesil değişince, pane kapanınca ve `IDLE` (120 s) soru gelmezse
kapanıyor. Açılış hatası `RETRY_AFTER` (10 s) boyunca aynı nesle aynı cevabı
veriyor, sonra **yeniden deniyor**. `Count` isteği `COUNT_TIMEOUT` (120 s)
kadar sürebiliyor ve o sürede worker başka soru almıyor.

**Yordamsal karakterler küçük sınıfta kapalı.** U+2580–259F (sparkline'ın
`▁…█`'i dahil) `bt-atlas`'ta fonta sorulmadan çiziliyor
(`raster::is_procedural`, 021), ama kapı `SizeClass::Small`'da kapalı
(`Atlas::slot`'un `(Sprite::Char(ch), SizeClass::Normal) if is_procedural`
kolu; bekçisi `the_small_class_still_asks_the_font`). Gerekçe döşeme değil
ölçü ayrışması: `draw_procedural` büyük hücrenin `Metrics`'iyle çiziyor
(`block` geometrisini `m.cell_wh()`'tan alıyor, `assert_eq!(target.len(),
m.slot_bytes())`), bağlam satırının sütun adımı ise küçük yüzün ilerlemesi —
dolu bir `█` komşusunun üstüne binerdi. Bugün bağlam satırında `▁…█` Menlo'nun
kendi glyph'inden geliyor ve Menlo'nun bloğu hücreyi doldurmuyor (021'in
varlık sebebi), yani bir sparkline orada döşemeyen, kopuk çubuklar olurdu.
Kapının ölçü sorunu **bugünkü aritmetiğin** sonucu: yuva büyük hücre
boyunda, ama küçük yüzün kendi `Metrics`'i (`rules::metrics(&small, …)`,
genişlik `context_cell_w`) türetilip ayrı bir tampona çizilerek yuvaya
taşınabilir.

**Pane'in görünürlüğü ve odak.** Örtülme pencereden `SplitView::apply_visibility`
ile pane'lere dağılıyor (arka sekme, küçültülmüş pencere, büyütülmüş
bölmenin arkasındaki pane); uzak oturumun kenarı
`TerminalPane::remote_or_title_changed`'de (orada yardımcı oturum zaten
kapanıyor); hedef ve nesil `Session::remote_target`'tan.

**Ayarlar.** `[remote]` bugün `hosts` ve 045'in sekiz önizleme/indirme
anahtarı (`bt-core::settings::RemoteFiles`); ayar penceresinde Remote Files
kategorisi (`settings_window`, `Category::RemoteFiles`). Belge
`docs/AYARLAR.md` → `[remote]`.

## Motivasyon

Kullanıcı isteği (2026-10-02): ssh'tayken bağlandığı makinenin ne kadar
yüklü olduğunu görmek için bugün ayrı bir `htop`/`uptime` koşturmak
gerekiyor. Durum çubuğunun boş sağ tarafı tam o bilginin yeri: "neredeyim"
solda, "makine nasıl" sağda.

Kullanıcı tasarımı onayladı; onaylı taslak bu setin içinde: `design.html`
(kaynak artifact https://claude.ai/artifact/7Ey2KXib8bYrn6ipuckpTn). Ürün
kararları (yeniden açılmaz):

- Varsayılan gösterim sparkline: `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%` — son 8 örnek,
  U+2581–2588.
- `[remote] stats = "sparkline" | "numbers" | "alerts" | "off"` ve
  `stats_interval`; ayar penceresinde Remote Files kategorisinde.
  `numbers` = `cpu 23%  mem 61%`; `alerts` = normalde küçük yeşil `●`, eşik
  aşılınca yalnız aşan değerler.
- Eşikler cpu 70/90, mem 80/92, disk 85/95 (tasarım sabiti); disk yalnız
  %85'i geçince görünür.
- Dar pencerede düşme sırası: grafik → yalnız sayılar → yalnız en kötü
  değer → hiç; eşiği aşmış değer varsa yoldan önce gelir; sonra yol bugünkü
  gibi soldan `…`; host asla kısalmaz.
- Aktarım sürerken gösterge gizli, bitince geri.
- Göstergeye tık → popover (host + OS, çekirdek sayısı + CPU %, load
  1/5/15, bellek ve swap, disk `/`, uptime, en çok CPU yiyen 3 süreç);
  açıkken tazelenir, el imleci, Esc ve dışarı tık kapatır.
- Veri 045'in yardımcı oturumundan, yeni bağlantı açılmaz; ilk sürüm yalnız
  Linux uzak, `/proc` yoksa gösterge sessizce yok; parola isteyen sunucuda
  gösterge yok; iç içe ssh'ta algılanan hedef ölçülür.
- Boşta sıfır kare: kare yalnız gösterilen değer değişince; örnekleme uzak
  oturum bitince, pane örtülüyken ve uzun etkileşimsizlikte durur, geri
  gelince ilk örnek hemen.

**Taslak ile brief'in tek çelişkisi — brief geçerli.** `design.html`'in
betiği sayıyı ön plan renginde (`f`) çiziyor ve sparkline'ı CPU'nun eşik
rengine boyuyor. Onaylı brief "değerler sönük (`dim`); eşik aşınca **yalnız
sayı** `warning`/`error` rengini alır, kritikte sayının başında `▲`" diyor —
gerekçesi production işaretinin kırmızısıyla (host adı ve saç çizgisi)
karışmaması. Uygulama taslağın renklerini kopyalamaz: etiket, sparkline ve
eşik altındaki sayı `dim`; yalnız eşiği aşan sayı (ve `▲`'sı) rengini alır.
Taslağın Türkçe popover etiketleri de kopyalanmaz: UI dizgileri İngilizce
(`CLAUDE.md` → Dil).
