# Ölçüm profili — bateri

`/measure` projeden bağımsızdır; bu projede **neyin ölçüldüğünü, hangi
kancayla ve neyin nerede emanette durduğunu** burası söyler (`proje.md` →
Belgeler). Yöntem kuralları ve sayılar `docs/OLCUMLER.md`'dedir ve burada
tekrarlanmaz — orası `## Yöntem` ile `## Nasıl yeniden ölçülür`'ün tek
sahibi.

## Koddaki emanetler

Kare süresi ve açılışın yöntemini sıfırdan yazmak gerekmiyor: kancanın dürüst
sınırları **kodda** emaneten duruyor ve o türün ilk ölçümü onları
`docs/OLCUMLER.md` → `## Yöntem`'e taşır.

- `crates/bt-shell-macos/src/app.rs` → `Measured`'ın doc'u, "**Ölçümün dürüst
  sınırları**" başlığı — her kalem **kapsam** ya da **açık kalem** diye
  etiketli; kopyalanmıyor, eksik kopyalanan bir liste sessizce ayrışır.
- Aynı dosyada `IDLE_FRAME_LIMIT` ve `Report::token_line`: kapının gerekçesi
  ve jeton sözleşmesinin dil kuralı.
- `crates/bt-gpu/src/stats.rs`: `MIN_SAMPLES`'ın türetimi (p95'in tabanı),
  halka kapasitesi, hangi karenin **elendiği**.

## Türler

| tür | nasıl | kanca | bölüm |
|---|---|---|---|
| kare süresi | `BT_FRAME_STATS=1 BT_SCROLL_TEST=1 BT_RUN_SECONDS=30` — jeton satırı üç sütun basar: `cpu_kare_*` (`session.frame`: kilit + ayrıştırma + grid + sink), `cpu_encode_*` (encode + commit), `gpu_*` (Metal'in kendi saatinden). Çapraz kontrol `xcrun xctrace record --template 'Metal System Trace'` | **var** | `## Kare süresi` |
| wgpu offscreen kare döngüsü | `cargo test --release -p bt-gpu offscreen_frame_loop -- --ignored --nocapture` — tek satır (`arka_uc=wgpu`; Metal yarısı 040 phase-7'de söküldü), değerler µs: `cpu_kare_*` (karenin kurulması — tanık), `cpu_encode_*` (encode + gönderim), `gpu_*` (`TIMESTAMP_QUERY` yoksa `unsupported`). Pencere yolu değil: `## Kare süresi` ile karşılaştırılmaz | **var** (`#[ignore]`'lu sınama) | `## wgpu denemesi` |
| düşen kare | — | **yok** (bilerek kapsam dışı). `dusen=` halkaya sığmayan **örnek**, atlanan kare **değil**; ikisini karıştırma | `## Kare süresi` |
| giriş gecikmesi | `BT_INPUT_LATENCY_SAMPLES=200` — tuş → PTY → echo → parse → commit → presented zinciri, medyan ve p95 | **yok** (zincirin orta halkaları `alacritty_terminal`'de) | `## Giriş gecikmesi` |
| bellek | `footprint -p {pid}` ya da `vmmap --summary`; 1 sekme boş, 1 sekme 10 000 satır dolu, 8 sekme | araç dışarıdan, sekme yok | `## Bellek` |
| açılış | aynı koşunun `acilis=` jetonu — `main()`'in **ilk satırından** ilk **tamamlanan** kareye | **var**, ama tarifi dar: süreç başlangıcı ve *presented* değil (dürüst sınırlar) | `## Açılış` |
| boşta kare (`IDLE_FRAME_LIMIT`) | sağlıklı `make duman` (debug) + paketten `open` (release) koşuları, üstüne kasıtlı bozulmuş bir koşu; tarif ve paket yolunun tuzakları dosyada | **var** (`kare=`/`istek=` jetonları); kapının kendisi değil, sınırını doğuran ölçüm | `## Boşta kare` |
| ayrıştırıcı / atlas bench | `cargo bench -p bt-core --bench parse`, `cargo bench -p bt-atlas` | **yok** — `criterion` ayrı bir bağımlılık kararı; `cargo bench --workspace -- --list` → `0 benchmarks` | `## Bench` |

Kancası **yok** olan satırda ölçüm "yok" değil "ölçüm aracı yok"tur.
Metalterm'in aynı iş için kullandığı kancalar `docs/ARASTIRMA.md` → "Shell
entegrasyonu" altındadır.

## Koşu şartları

- **Dağılım:** kare süresinde **p95 ve en kötü kare**, gecikmede **medyan ve
  p95**; düşen kare sayısı ayrı sütundur.

- `BT_FRAME_STATS` ile `BT_SCROLL_TEST` **sıfırdan büyük** bir
  `BT_RUN_SECONDS` ister (yoksa süreç çıkış 1 verir — rapor yalnız deadline
  yolunda basılıyor), ve ölçüm yükü olmadan kare akmaz.
- Satır profilini kendi söylüyor (`profil=debug|release`): **release** ölçülür,
  `cargo build --release` ve `target/release` altındaki binary.
- `ornek=` ile `taban=` birlikte okunur: taban altında p95 **ve** en kötü
  değer basılmaz, ikisi de `insufficient` çıkar — o koşu bir sayı değil, bir
  **arıza** raporudur; yorumlama, yeniden koş.
- Sütun sayaçları ayrı ve karıştırılmaz — `ornek=` CPU'nun, `gpu_ornek=`
  GPU'nun, `gpu_elenen=` ölçülemeyip atılan kare, `dusen=` halkaya sığmayan
  örnek. Neden eşit olmayabildikleri `Measured` ve `bt_gpu::Samples`'ın
  doc'unda.
- Ortam kayda yazılır: güç kaynağı **prizde**, ekran Hz'i ve pencere boyutu
  (hücre sayısı) — 120 Hz ile 60 Hz'in kare bütçesi farklıdır, `118×34` ile
  `200×60` aynı sayı değildir.
- Kare süresinde yolun ateşlendiğinin tanığı satırın kendisi: `kare=`,
  `ornek=`, `gpu_ornek=` ve `gpu_elenen=` beklenenden küçükse ölçüm durmuştur.
