# Phase 6 — Yazım efektleri iki eksende

## Özet

030'un Keypress/Erase efektleri çok satırlı dock'ta da koşsun: anahtar sütun
değil (satır, sütun) konumu, sarma yüzünden satır değiştiren kayma iki
eksende.

_Requirements: R6_

## Değişiklikler

- **`crates/bt-core/src/dock.rs`** — `diff` düz metin üstünde kalır (tek
  bitişik ekleme/silme, `EDIT_MAX`); `DockEdit` konumu `layout` üzerinden
  (satır, sütun) olarak verir, kayma iki eksenli. Satır sayısı atlayan ya da
  eşlenemeyen hâl `Reset`.
- **`crates/bt-gpu/src/glyph_fx.rs`** — girdiler (satır, sütun) anahtarlı;
  `shift` ve `retain` iki boyutlu pencereyle; phase-3'ün çok satırlı
  `Reset`'i kalkar — o kapı `bt-core`'da, `dock::render_with`'in `single`
  koşulu (phase-3 → Uygulama Notları); `DockEdit::Shift` phase-3'ten beri
  üretilmiyor.

## Kabul

- Satır sonunda yazılan harf sarılıp yeni satıra geçerken efektiyle geliyor;
  ikinci satırda Backspace Erase ile gidiyor; kayan harfler yeni konumunda
  animasyonsuz.
- Tek satırda mevcut 030 bekçileri değişmeden yeşil.
- `make hepsi`; gözle kontrol: çok satırlı `BUFFER`'da yazma/silme.

## Checklist

- [x] `DockEdit` (satır, sütun)
- [x] `glyph_fx` iki eksen
- [x] Test: sarma sınırında ekleme, ikinci satırda silme
- [x] phase-5'ten devir (gözle görülen, 032'nin değil): çok satırlı komutun ilk satırı ekranın üstüne kayınca blok işareti (chevron) devam satırına, ızgaranın tepesine oturup orada kalıyor — çıpa `preexec`'e kadar açık, `blocks.anchors` görünen ilk çıpalı satırı komut satırı sayıyor (teşhis `phase-5.md` → Uygulama Notları). Çare: çıpa değişiminde üstteki satır (geçmiş dahil) aynı kimliği taşıyorsa o satıra işaret ve sayaç çizme; doldurma bandının devamında da aynı soru. Kapsam dışı sayılırsa `docs/YOL-HARITASI.md`'ye adıyla
- [x] Doğrulama geçti (`make hepsi`)

## Uygulama Notları

- **Kayma yalnız dikey eksende.** Düzenlemenin önündeki metnin düzeni
  değişmiyor (önek aynı), arkasındaki ise sarmayla tekdüze kaymıyor; o
  harfler için kayma üretilmedi — `Frame::suppress_dock` statik glyph'ini
  bulamayan gelişi zaten bitiriyor ("kayan harfler animasyonsuz"). Tekdüze
  kayan tek şey dikey pencerenin tepesi: `shift`/`Shift` artık **satır**,
  `GlyphFx::apply`'ın penceresi `(ilk, son)` sütun yerine giriş satırı
  sayısı. Oturma kuralı `(satır, sütun)` sözlük sırasıyla; "doğmamış geliş"
  karşılaştırması da iki eksende.
- **Tepe farkı `Session::dock`'ta** (`dock::with_shift`): `render_with`
  önceki tepeyi bilmiyor ve karede tek düzenleme gidiyor, yani kayma
  düzenlemenin alanına yazılıyor, düzenleme yoksa tek başına `Shift`.
  Ölçü **son çizilen** tepe (`DockWindow::painted`): `dock_scroll` izin
  `top`'unu kare beklemeden yazıyor ve ondan okunsaydı tekerlek karesinde
  kayma sıfır çıkardı.
- **Plan dışı iki `Reset`:** `PREBUFFER` değişimi (ZLE bir satırı kabul
  etti; `diff` yalnız `PREDISPLAY`'i soruyordu) ve satır sonu içeren silme
  (hayalet listesi yalnız glyph taşıyor, `\n`'in arkasındakiler aynı satıra
  dizilirdi).
- **Blok işareti (phase-5 devri) çıpa toplamada kapandı:** yeni bir kimlik
  bir satırda ilk kez görününce üstteki satır (geçmiş dahil; defterin dışı
  `false`, yoksa kırpma satırı kendisine çevirirdi) taranıyor
  (`session::block_row_continues`) ve aynı kimlik oradaysa çıpa yazılmıyor —
  işaret de sayaç da komutun başında, görünmüyorsa hiç. Bant aynı kuralla,
  kendi tepesinin üstündeki satıra bakarak. Tarama satır başına bir kez
  (aynı satırın öteki hücreleri önbellekten).
- **`/code-review` (set kapısı) bulguları, ikisi de giderildi:** (1) Ctrl-L
  prompt'u aynı kimlikle yeniden basıyor ve `2J` eski prompt satırını
  geçmişe itiyordu — yeni prompt devam satırı sayılıp işaretini ve sayacını
  kaybediyordu. Temizlemenin geçmişe ittiği en yeni satırın kimliği
  (`row_identity`) `Session::clear_boundary`'de, kural onu devam kaynağı
  saymıyor (kimlik nesli tüketen karede ve damgasız bayrağın karesinde bir kez daha okunuyor — damganın gerekçesi: tüketen kare ızgarayı temizlenmemiş görebilir); bilinen sınır: defter doyunca tampon yeniden kullanılabilir (yön
  görünür — işaret bir devam satırına oturur). (2) Pencere tepesi yerinde
  kalırken bant küçülünce (sarılan satırın son harfi silindi) alt satırdaki
  hayalet bağlam satırının üstünde kalıyordu → `GlyphFx::shift` kaymasız
  karede de pencereyle süzüyor.
- **Gözle kontrol** (bateri-dev, açık ve koyu tema): sarılan satırın ikinci
  satırında yazılan harf orada beliriyor, Backspace hayaleti o satırda;
  heredoc'ta `PREBUFFER`'ın altındaki satırda geliş/hayalet yerinde; `for`
  döngüsü geçmişe kaydırılınca işaret yalnız `for` satırında, `for>`
  satırları tepedeyken işaret yok. Bir kez tema menüsünden hemen sonra
  yazılan ~30 karakter düştü; tekrar edilemedi (menü izlemesinin tuşları
  yuttuğu düşünülüyor, doğrulanmadı).
- **`/audit` (set kapısı):** mekanik yarı temiz; tek bulgu mercek 7 —
  `Session::dock`'un `#[allow(clippy::too_many_arguments)]`'ı gerekçesizdi
  (gerekçe eklendi), `Motion::sync`'inki gereksizdi (kaldırıldı). Öteki
  mercekler temiz (2: bash/fish gerekçesi `plan.md` → Kapsam Dışı; 3, 4, 5)
  ya da ilgisiz (1, 6). Paket envanteri `bateri-dev.app`'e `cmp` ile: beş
  betik dosyası aynı.

