# Phase 6 — Yükleme satırının düğmeleri

## Özet

Gözle kontrolde kullanıcı yükleme satırının `▴ list  ✕`'unun tıklanabilir
olduğunu anlamadı (düz metin, host rengi, küçük punto, fareye tepkisiz, hedef
tek hücre, `✕` belirsiz, `⌘.` ipucu yok). Onaylanan tasarım
(https://claude.ai/artifact/Tib7F2FheEM2nQgm5hENBz): simge değil fiil
etiketli, dolgulu ve çerçeveli iki düğme — tek öğede `Cancel ⌘.`, birden çok
öğede `Show files (N)` (liste açıkken `Hide files`) ve `Cancel all ⌘.` —,
üstüne gelince koyulaşan dolgu ve el imleci, tıklama alanı dolgunun tamamı.

_Requirements: R10_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `Transfer::controls` dizge değil
  `Copy` bir durum (`TransferControls`: listedeki öğe sayısı, liste açık mı,
  farenin altındaki düğme); `TransferAction` (List/Cancel) ve düğmenin
  durumu (`ButtonState`: Idle/Hover/Pressed). Etiketler bu durumdan
  `bt-core`'da doğuyor.
- **`crates/bt-core/src/dock.rs`** — `transfer_layout` düğmeleri **sağa
  yaslı** yerleştiriyor ve sığmazsa sırayla düşürüyor: önce `⌘.` ipucu, sonra
  liste düğmesi, `Cancel` en son; gövde kalanı alıp sağdan `…` ile kısalıyor.
  Düğme `pad + etiket [+ boşluk + ⌘.] + pad` sütun. Çizim (`render_transfer`,
  etiket ön planda, ipucu sönük, fare üstündeyken ipucu ön planda) ve fare
  (`transfer_button_at`, `transfer_controls_col`'un yerine) aynı yerleşimi
  okuyor. `Dock::buttons` düğmelerin dock-yerel sütun aralığını, rengini
  (işaretin rengi) ve durumunu sınırdan veriyor. `UPLOAD_GLYPHS`'ten `▴ ✕`
  çıkıyor, `⌘` giriyor.
- **`crates/bt-gpu/src/frame.rs`, `renderer.rs`, `link.rs`** — dolgu ve
  çerçeve caret'in fragment'inden (`caret_fragment`: yuvarlak dikdörtgenin
  SDF'i, kenar kalınlığı zaten var) — yeni pipeline ve shader yok. Düğme
  başına iki çizim (dolgu; `rule_px` kalınlığında çerçeve), alfa durumdan
  (tasarım sabitleri), yarıçap seçiminki; sıra dock zemini ve seçimi → düğme →
  caret → glyph.
- **`crates/bt-shell/src/upload.rs`, `uploader.rs`, `view.rs`** — `CONTROLS`,
  `Control`, `control_at` kalkıyor; `Uploads` farenin altındaki düğmeyi ve
  listenin açıklığını tutup satıra damgalıyor (200 ms'lik tazeleme hover'ı
  ezmesin). `mouseMoved:` ızgaranın reddinden **önce** düğmeyi soruyor;
  değişince satır yeniden yazılıyor (kare yalnız değişimde) ve imleç
  `pointingHandCursor` ↔ ok. Liste açılırken `Hide files` + basılı ton,
  kapanınca geri.
- **`crates/bt-atlas/src/lib.rs`** — küçük sınıfın kutu sınaması yeni sözlükle.
- **`CLAUDE.md`** — yükleme satırının cümlesi yeni düğmelerle.

## Kabul

- `bt-core`: tek öğede `Cancel ⌘.`, iki öğede `Show files (2)` + `Cancel all
  ⌘.`, liste açıkken `Hide files`; dar pencerede düşme sırası (ipucu → liste →
  Cancel) ve gövdenin kısalması; `transfer_button_at` düğmenin her sütununda
  (dolgunun tamamı) doğru eylemi, dışında `None` veriyor; sonuç satırı
  düğmesiz.
- `bt-gpu`: düğme dörtlüleri bağlam satırında, sütun aralığıyla hizalı ve
  durum alfayı değiştiriyor.
- `bt-atlas`: `⌘` küçük sınıfta kutu değil.
- Gözle kontrol: fareyi `Cancel`'a getir → dolgu koyulaşıyor ve el imleci;
  tıkla → iptal; iki dosyada `Show files (2)` → liste.

## Uygulama Notları

- **Dolgu ve çerçeve seçimin değil caret'in fragment'inden**: `selection_fragment`'in
  kenar bandı yok, `caret_fragment`'in var (yarıçap + kenar kalınlığı). Shader
  değişmedi, `make shader` gerekmedi, pipeline sayısı altı. Uniform çizim
  başına olduğu için `encode_caret`'in gövdesi `encode_rounded`'a çıktı; caret
  kendi "tek dörtlü" bekçisini koruyor.
- **Düğmeler sağa yaslı** (phase-5'te gövdenin hemen arkasındaydı): gövde her
  200 ms'de boy değiştiriyor ve düğme farenin altından kayardı; onaylanan
  demo da sağa yaslı.
- **"Etiket açılır" uygulanmadı, ipucu açılıyor**: etiket zaten ön planda
  (temanın en güçlü mürekkep rolü) ve açık temada "daha açık" ters yöne
  giderdi. Fare üstündeyken `⌘.` sönükten ön plana çıkıyor; tepkiyi dolgu,
  çerçeve ve el imleci taşıyor.
- **`Cancel` basışta iptal ediyor** (bugünkü gibi); basılı ton yalnız açık
  listenin düğmesinde (`Hide files`) görünüyor.
- **Sayı listedeki öğe sayısı** (akan + bekleyen), kuyruğun toplamı değil —
  "1 of 2"deki 2 ile ayrışabiliyor, liste neyi gösteriyorsa o.
- **Dikey isabet aralığı dolgunun satır bandı** (`bt_gpu::context_row_offset`
  yeni ve `pub`): phase-5 bağlam satırının üstündeki her yeri kabul ediyordu.
- **`objc2-app-kit`'e `NSCursor` bayrağı** (yalnız başlık, `Cargo.lock`
  oynamadı; `make denetim`'in `Cargo.toml` uyarısının karar kaydı bu satır):
  el imleci ve geri ok.
- **`/code-review` bulguları (ikisi de giderildi):** hover yalnız
  `mouseMoved:`'da hesaplanıyordu, sağa yaslı düğmeler kıpırdamayan farenin
  altında değişince (kuyruk tek öğeye indi, `Hide files`, menü kapandı) eski
  düğme hover'da kalıyordu — tazeleme, menünün kapanışı ve pencerenin key
  oluşu hover'ı farenin şimdiki yerinden yeniden hesaplıyor; pencere key
  olmaktan çıkınca hover düşüyor ve düğmenin üstündeyken el imleci her soruda
  yeniden kuruluyor. **Bilinen sınır:** fare düğmeden doğrudan pencerenin
  dışına çıkarsa (izleme alanı yok) düğme bir sonraki harekete ya da
  tazelemeye kadar hover tonunda kalıyor.
- `/audit`: mekanik temiz (`Cargo.toml` uyarısı yukarıda), mercekler temiz,
  ayar/tema merceği ilgisiz.

## Checklist

- [x] `bt-core` düğme modeli, yerleşim, çizim ve isabet testi
- [x] `bt-gpu` dolgu + çerçeve (caret fragment'i)
- [x] `bt-shell` hover, imleç, liste durumu; eski düğme kodu kalktı
- [x] `bt-atlas` sözlük sınaması
- [x] `CLAUDE.md`
- [x] Test: yukarıdaki Kabul maddeleri
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Gözle kontrol (devir mesajının cümlesi)
