# Phase 4 — Şerit çizilir

## Özet

Blok aralıkları temanın durum renkleriyle sol paya çizilir: komut blokları
ilk kez ekranda görünür.

_Requirements: R4, R4.1, R4.2, R4.3_

## Değişiklikler

- **`crates/bt-gpu/src/frame.rs`** — şerit `Frame`'de **kendi listesi**.
  `bg`'ye girmiyor: `push`'taki `debug_assert_eq!(bg.len(), bg_count)` sıra
  sözleşmesi ve `move_cursor`'ın `bg.truncate(bg_count)`'u dokunulmadan
  kalır. `bg`'ye girip sayılmasaydı imleç her kaydığında şerit silinir ve
  **titrerdi**; sayılsaydı `hucre=` jetonunun anlamı kayardı. Hareket karesi
  listeyi **korur** (`move_cursor` yolu): ızgara değişmediği için şerit de
  değişmemeli.
- **`crates/bt-gpu/src/renderer.rs`** — şerit için ayrı draw call, **mevcut
  `cell_bg` pipeline'ı**. `Instance` genel bir piksel dörtgeni
  (`pos`, `size`, `rgba`), yeni shader ya da `#[repr(C)]` düzeni yok.
  Renk lineer geçer — hedef `BGRA8Unorm_sRGB` ve kodlamayı ROP yapıyor;
  ikinci bir gamma düzeltmesi paleti iki kez kodlar.
- **`crates/bt-gpu/src/link.rs`** — `frame()`'in verdiği blok aralıkları
  `Frame`'in şerit listesine aktarılır. **Animasyon yok** (Karar 5): şerit
  anında belirir, `motion` ikinci bir tüketici kazanmaz, `motion_settled()`
  kapısı ve `Mode::Fade`'in "tek yer" kuralı dokunulmadan kalır.

## Kabul

- Başarısız komut kırmızı, başarılı komut sakin bir şerit alır; koşan komut
  `accent`. Renk **sınırdan geliyor**, bu katmanda üretilmiyor.
- Tema değişince şerit **aynı karede** yeni palete geçer: kaynak
  `Session::frame`'in `Term` kilidinden önce aldığı kopya, hücrelerinkiyle
  aynı.
- `make duman` yeşil ve jetonlar **oynamaz**: `smoke_shell` OSC 133
  basmadığı için blok yok, şerit yok. Bu aynı zamanda bu yolun duman
  kapısının **dışında** olduğu anlamına gelir (aşağıda).
- Boşta sıfır kare korunur: şerit hiçbir kare istemiyor, yalnız çizilen
  karede görünüyor. `icerik` ve `sessiz` sınırları yerinde.
- Gerçek bir zsh oturumunda `false` koşmak kırmızı, `true` koşmak sakin
  şerit bırakır; geçmişe kaydırınca şeritler satırlarıyla birlikte gider;
  pencereyi yatay boyutlandırmak onları prompt satırlarında tutar.

## Yayın Etkisi

- **`CLAUDE.md`** — iki cümle eskiyor: "ürün yüzeyi (blok, dock) henüz yok"
  ve "satıra çıpalanması … henüz yok". Aynı commit'te düzelir. (Tema
  rollerinin cümleleri phase-2'de düzeldi.)
- **`docs/YOL-HARITASI.md`** — 010 satırı kapanır; "(+ blok animasyonları)"
  **ertelenmiş borç** olarak "Sete bağlanmamış borçlar"a iner, gerekçesiyle.
- **`make duman` kapının dışında kalıyor:** `smoke_shell` OSC 133 basmadığı
  için şerit yolu jetonlara hiç girmiyor. Bilerek — kapıyı görür kılmak
  reçeteye OSC 133 eklemek, o da `QUIET_FLOOR` ile `IDLE_FRAME_LIMIT`'in
  yeniden türetilmesi demekti (`proje.md`: ayrı commit). Şerit animasyonsuz
  olduğu için kapının asıl koruduğu şey (boşta sıfır kare) bu yoldan
  tehdit altında değil.
- **shader yok:** `.metal` değişmiyor, `#[repr(C)]` düzeni aynı;
  `make shader` gerekmiyor.
- Ayar şeması, shell entegrasyonu, terminfo, bundle: değişiklik yok.
  Yeni bağımlılık yok. Ölçüm iddiası yok.

## Uygulama Notları

- **Şerit genişliği planda yoktu ve bir sabit doğurmadı.** `docs/ARASTIRMA.md`
  Metalterm'in `command_gutter`'ını bir ayar olarak sayıyor ama genişlik
  vermiyor; phase-3 payı "şerit artı iki yanında nefes payı" diye tanımlamıştı.
  `push_block` o cümlenin aritmetiği oldu: şerit payın **ortasındaki yarısı**,
  iki yanında dörtte birlik nefes. İkinci bir pt sabiti eklenmedi — eklenseydi
  ölçek değiştiğinde payla ayrışır ve şerit paydan taşardı, yani
  `CellMetrics`'in payın ikinci okuyucusunu yasaklama gerekçesi birebir geri
  gelirdi.
- **Şerit `pos_at`'ten geçmiyor.** O satır payı ızgaranın orijinine ekliyor;
  şerit ise payın **kendi içinde** duruyor. Geçseydi ilk sütunun üstüne düşer
  ve metni örterdi — Karar 3a'nın ayırdığı payın tamamı boşa giderdi. Bu
  `pos_at`'in "pay yalnız burada eklenir" cümlesine bir istisna değil: şerit o
  cümlenin konusu olan **ızgara** koordinatı değil.
- **`encode_bg` ikiye bölünmedi, ortak gövdeye indi.** Plan "ayrı draw call"
  diyordu ve ilk refleks `encode_bg`'nin ikizini yazmaktı; iki gövde
  kopyalanınca buffer indeksi ya da pipeline seçimi ayrışabilir ve belirti
  yalnız bir listede görünürdü. `encode_quads(instances)` ortak yol oldu,
  `encode_stripes` ile `encode_bg` ona hangi listeyi verdiklerini söylüyor.
- **Şeridin sayacı yok ve bu bilinçli.** Üç kardeşi (`bg_count`,
  `glyph_count`, `rule_count`) duman jetonları ve jeton satırı bir makine
  sözleşmesi. Dördüncü bir jeton açmak reçete OSC 133 basmadığı için hep sıfır
  okuyan, yani hiçbir şey söylemeyen bir kapı olurdu; var olan bir jetona
  girmek `hucre=`'nin anlamını kaydırırdı. Şeridin tek bekçisi bu yüzden
  offscreen piksel okuması — düşerse şeridin çizildiğini söyleyen başka hiçbir
  şey kalmıyor ve sınamanın doc'u bunu yazıyor.
- **Ters aralık `debug_assert`, release'te doyuruyor.** `last_row < first_row`
  `bt-core`'da bir kusur olurdu; sessizce bir satırlık şerit çizmek onu
  saklardı. Çizim yolunda panik yok, o yüzden bekçi debug'da gürültülü,
  release'te `saturating_sub` ile bir satıra doyuruyor.
- **Hareket karesi yoluna tek satır bile eklenmedi** ve bu "animasyon yok"
  maddesinin kanıtı: `move_cursor` şerit listesine dokunmuyor, yani ızgara
  değişmedikçe şerit olduğu gibi kalıyor. Ayrı liste kararının (Karar, phase
  gövdesi) ödediği şey tam olarak buydu.
- **Offscreen sınaması payı sıfır **vermiyor**.** Kardeşlerinin `grid()`
  yardımcısı payı sıfır kuruyor (phase-3'ün notu: `cell_rows` orijini sıfır
  varsayıyor) ama şeridin sorusu tam olarak payın geometrisi: sınama
  `CellMetrics::new(4, 4, 8)` ile kuruluyor ve `pixel_at` ile pay bölgesinden
  okuyor. İki blok, iki durum rengi — tek şerit `inst[0]`'ı stride'dan bağımsız
  okur, yani 32'den kayan bir stride görünmezdi.

## Checklist

- [x] `Frame`'de şerit için ayrı liste; `bg`/`bg_count` ve `move_cursor`
      dokunulmadı
- [x] Renderer'da ayrı draw call, mevcut `cell_bg` pipeline'ı
- [x] `link.rs` blok aralıklarını aktarıyor; animasyon eklenmedi
- [x] Test: `Frame` — şerit listesi `bg_count`'a girmiyor, `move_cursor`
      şeridi silmiyor
      (`stripes_stay_out_of_the_cell_count_and_survive_motion_frames`,
      `a_stripe_spans_its_rows_inside_the_gutter`)
- [x] Test: offscreen çizim — şerit pikselleri doğru renkte (ara ton bir
      renkle; `0.0`/`1.0` sRGB'nin sabit noktaları)
      (`command_stripes_paint_the_gutter_on_the_gpu`)
- [x] `CLAUDE.md` ve `docs/YOL-HARITASI.md` güncellendi
- [x] Doğrulama geçti (`make hepsi` + `make duman`: `kare=30 hucre=8 glif=6
      kural=15 icerik=3 hareket=27 sessiz=1750.64ms kapanis=clean` — jetonlar
      oynamadı, şerit yolu kapının dışında)
- [x] Gerçek zsh oturumunda gözle kontrol (Kabul'ün son maddesi) — `make kur`
      sonrası doğrulandı: şerit genişliği ve renkler piksel ölçümüyle onaylandı,
      ayrıntısı `teslim.md` → B.2. Koşan bloğun pencerenin dibine kadar uzaması
      plana uygun ama iyi görünmüyor; düzeltmesi 011'e bırakıldı (içerik tabana
      yapışınca aralık kendiliğinden kısalıyor)
- [x] Yayın etkisi yazıldı
