# Phase 3 — Sol kenar payı, tek sabitten

## Özet

Şeridin oturacağı pay pencerenin solundan ayrılır; genişlik **tek sabit**
olarak yaşar ve `cols` hesabı, çizim orijini ile fare eşlemesi onu aynı
yerden okur.

_Requirements: R5, R5.1, R5.2_

## Değişiklikler

- **`crates/bt-gpu`** — gutter genişliği burada doğar ve hücre ölçüsünün
  yanında yayımlanır (`Renderer::cell_metrics` emsali: ölçek uygulanmış
  piksel değeri tek kaynaktan çıkar). Katman yönü bunu zorunlu kılıyor —
  `bt-shell` `bt-gpu`'yu görüyor, tersi değil.
- **`crates/bt-shell/src/app.rs`** — grid ölçüsü payı düşerek hesaplanır.
  Bu `cols`'u bir azaltabilir, yani PTY `winsize`'ını da değiştirir;
  sabit ölçülü sınamalar (`split_into_grid`) mekanik olarak düşer ve aynı
  commit'te düzelir.
- **`crates/bt-shell/src/view.rs`** — `point_to_cell` payı çıkararak böler.
  Doc'undaki "pencere kenar payı yok" cümlesi artık yanlış, düzelir.
  Payın içine düşen tıklama ilk sütuna kırpılır; seçim payda başlamaz.
- **`crates/bt-gpu/src/frame.rs`** — hücrelerin piksel konumu paydan sonra
  başlar. Orijin ile `cols` **aynı** sabitten beslenmezse belirti "fare bir
  sütun kayıyor" olur ve sessiz değildir ama geç fark edilir; üç tüketicinin
  tek kaynağı bu phase'in asıl işi.

## Kabul

- Pay **her zaman** ayrılır ve oturum ortasında değişmez: entegrasyonsuz
  oturumda (bash/fish, `shell.integration = false`, SSH) boş kalır. Kabul
  edilen bedel — alternatifi ilk prompt'ta bir SIGWINCH ve üç tüketicinin
  aynı anda güncellenmesiydi.
- Alternatif ekranda da pay ayrılmış kalır; `cols` vim açılışında oynamaz.
- Tıklama ve sürükleme doğru sütunu seçer: metnin ilk karakterine tıklamak
  ilk sütunu verir, payın içine tıklamak da.
- `make duman` yeşil: `hucre=8 glif=6 kural=15` **oynamaz** (şerit ızgaranın
  dışında ve `smoke_shell` ilk satırda sekiz hücre basıyor), `icerik` ve
  `sessiz` sınırları yerinde kalır.
- Pencere paydan dar kalırsa mevcut davranış **korunur**, yeni bir alt sınır
  getirilmez: çıkarma `f64`'te yapılır, negatif değer `as u16`'da sıfıra
  doygunlaşır ve sıfır sütunu `Session::resize` zaten yoksayıyor (simge
  durumundaki pencerenin yolu). Çıkarmayı tam sayıda yapan bir uygulama bu
  zinciri **taşmayla** kırardı.

## Yayın Etkisi

- **`make duman` zorunlu** (`proje.md`: pencereyi açan davranış değişti).
  Şerit henüz çizilmediği için jeton sayıları değişmemeli; değişirse pay
  ızgaraya sızmış demektir.
- **shader yok:** `.metal` dosyaları ve `#[repr(C)]` düzenleri bu phase'de
  değişmiyor, `make shader` gerekmiyor.
- Ayar şeması: `command_gutter` bu sette **yok** (Karar 6), yani anahtar
  eklenmiyor ve `docs/AYARLAR.md` değişmiyor.
- Tema biçimi, shell entegrasyonu, terminfo, bundle: değişiklik yok.
- Yeni bağımlılık yok. Ölçüm iddiası yok.

## Uygulama Notları

- **Pay ayrı bir sabit değil, `CellMetrics`'in alanı oldu.** Phase "tek
  sabitten üç tüketiciye" diyordu; sabiti üç yerin okuması "tek kaynak"
  değil, tek kaynağın **üç kopyası** olurdu — biri tazelenip öteki
  kalabilirdi. Alan olunca ayrışma yapısal olarak imkânsız: `cols` hesabı,
  `Frame::pos_at` ve `point_to_cell` aynı **değeri** taşıyan tek yapıdan
  okuyor. `CellMetrics::new` üçüncü bir argüman aldı ve tipin doc'u
  "hücre ölçüsü"nden "ızgaranın piksel geometrisi"ne genişledi.
- **`GUTTER_PT = 8.0` ve `private`.** `docs/ARASTIRMA.md` Metalterm'in
  `command_gutter`'ını bir **ayar** olarak sayıyor ama genişliğini vermiyor;
  değer bu yüzden ürün kararı (şerit artı iki yanında nefes payı) ve tipik
  punto/ölçekte `cols`'tan en çok bir sütun götürüyor. Ölçülmüş bir sayı
  değil, `docs/OLCUMLER.md`'nin konusu da değil. `private` olması bilinçli:
  sabiti okuyan ikinci bir yer, tipin önlediği ayrışmayı geri getirirdi.
- **Payı ekleyen tek satır `Frame::pos_at`.** Dört tüketici (arka plan,
  glyph, kural, imleç) zaten o satırdan geçiyordu; ikinci bir yerde
  eklenseydi pay iki kez uygulanırdı. `cell.metal` `it.pos + corner *
  cell_px` diyor, yani shader payı hiç görmüyor — `make shader` gerekmedi.
- **Offscreen GPU sınamaları dördüncü tüketici çıktı.** `cell_rows`
  örnekleme noktasını `col * cw + x` ile kuruyor, yani orijini sıfır
  varsayıyor: sıfır olmayan bir pay o noktaları kaydırır ve on dört sınama
  hücre yerine clear rengini okurdu (gutter 8 px, `cw ≈ 9 px`'te tam bu
  olurdu). Çare sınamaları payla uyumlu kılmak değil, **payı sıfır vermek**:
  onların konusu payın geometrisi değil, GPU'nun hangi rengi hangi hücreye
  boyadığı. Aynı gerekçe `frame.rs` ile `view.rs`'in sahneleri için de
  geçerli; payın kendi sınamaları ayrı ve adıyla anılıyor.
- **`resize`'ın kabul kapısı payı da doğru taşıyor** ve bu bir tesadüf
  değil: pay da hücre ölçüsü de ölçeğin fonksiyonu ve `cell_metrics` ikisini
  **tek çağrıda** veriyor, yani pay hücre ölçüsü değişmeden değişemez. Kapı
  ayrılsaydı reddedilen bir boyutta pay yeni, ızgara eski kalırdı. Gerekçe
  `DisplayLink::resize`'ın doc'una yazıldı, yoksa yarın sessizce kırılırdı.
- **Dar pencerede yeni bir alt sınır getirilmedi.** Çıkarma `f64`'te
  yapılıyor: paydan dar pencerede fark negatife iniyor, bölme negatif kalıyor
  ve `as u16` sıfıra doyuruyor — sıfır sütunu `Session::resize` zaten
  yoksayıyor. Aynı çıkarma `u16`'da yapılsaydı **taşar** ve 65535'e yakın bir
  sütunla o boyda bir `TIOCSWINSZ` üretirdi; `a_window_narrower_than_the_gutter_yields_no_columns`
  o kırılmanın bekçisi.
- **Phase'in beklediği `split_into_grid` düşüşü olmadı.** Sabit ölçülü
  sınamalar paya sıfır verdiği için mekanik olarak geçtiler; düzeltilen tek
  şey `metrics()` yardımcısının imzası. `cols`'un bir azalması üretimde
  gerçek, sınamada değil — ve payı sorgulayan sınama onu kendi adıyla
  (`the_gutter_costs_columns`) tutuyor.

## Checklist

- [x] Gutter genişliği `bt-gpu`'da tek sabit, ölçekle birlikte yayımlanıyor
      (`CellMetrics::GUTTER_PT` → `cell_metrics(scale)` → `CellMetrics.gutter_px`)
- [x] `cols` hesabı payı düşüyor (`split_into_grid`, çıkarma `f64`'te)
- [x] Çizim orijini paydan sonra başlıyor (`Frame::pos_at`, tek satır)
- [x] Fare eşlemesi payı çıkarıyor; `point_to_cell` doc'u düzeldi
- [x] Test: `point_to_cell` payın içinde ve ilk sütunda doğru sonuç
      (`the_gutter_shifts_the_grid_origin`)
- [x] Test: `split_into_grid` yeni ölçülerle; dar pencerede `cols` alt sınırı
      (`the_gutter_costs_columns`, `a_window_narrower_than_the_gutter_yields_no_columns`)
- [x] Test: `Frame` orijini — dört tüketici de pay kadar kayıyor, boyut kaymıyor
      (`the_gutter_offsets_every_pixel_position`, `a_zero_gutter_leaves_the_origin_at_the_edge`)
- [x] Doğrulama geçti (`make hepsi` + `make duman`: `kare=29 hucre=8 glif=6
      kural=15 icerik=2 hareket=27 sessiz=1750.84ms kapanis=clean` — jetonlar
      oynamadı, yani pay ızgaraya sızmadı)
- [x] Yayın etkisi yazıldı
