# Phase 6 — Ayar penceresinde Remote Files

## Özet

Ayar penceresine beşinci kategori: önizleme, temizlik ve indirme satırları,
klasör seçici ve Clear Now.

_Requirements: R8_

## Değişiklikler

- **`crates/bt-shell-macos/src/settings_window.rs`** — `Category::RemoteFiles`
  ("Remote Files", sembol `network`); `Key`'e sekiz anahtar; popup satırları
  (`preview_max_size`, `preview_keep`, `preview_limit`, `download_conflict`
  — seçenek listeleri `bt-core`'un `NAMES` tablolarından), switch satırları
  (`preview_read_only`, `download_notify`). İki yeni satır türü: **klasör
  satırı** (yol etiketi + Change… → `NSOpenPanel` klasör seçimi →
  `SettingsEdit`; önizlemede Show in Finder) ve **kullanım satırı** ("340 MB
  · 12 files" + Clear Now → phase 4'ün yöntemi; boyut arka planda ölçülür,
  pencere açılınca ve Clear Now sonrası tazelenir). Pencere yine durum
  tutmaz; yazma tek düzenlemeden (029).
- **`docs/AYARLAR.md`** — pencerenin yeni kategorisi bir cümleyle.

## Kabul

- `make check` yeşil.
- Gözle kontrol: kategori görünür; her satır değişince `settings.toml`'da
  yalnız o anahtar yazılır ve canlı uygulanır; Change… klasörü değiştirir;
  Clear Now önizlemeleri siler ve kullanım sıfırlanır.

## Checklist

- [x] Kategori ve popup/switch satırları
- [x] Klasör satırı (NSOpenPanel) ve kullanım satırı (Clear Now)
- [x] (phase-4'ten) Clear Now `AppDelegate::sweep_previews(Sweep::ClearNow)`'u çağırır (arka plan thread'i, kurtarılanları kendisi bildirir); bitince kullanımı tazelemek için bugün tamamlanma kancası yok — gerekirse yönteme eklenir. Kullanım ölçümü `preview_cache`'e (taramanın `scan`'i, `.index` ve `.bateri-download-*` hariç) eklenebilir
- [x] `docs/AYARLAR.md`
- [x] Doğrulama geçti (`make check`, `make linux`)

## Uygulama Notları

- Boyut anahtarlarının (`preview_max_size`, `preview_limit`) `NAMES` tablosu yok: popup'lar tasarım sabiti preset listeleri (`PREVIEW_SIZE_PRESETS` 10 MB–5 GB, `PREVIEW_LIMIT_PRESETS` 500 MB–20 GB; Karar 8'in varsayılanları içinde). Dosyadaki değer preset değilse ayraçtan sonra eklenip seçili gösteriliyor, onu seçmek bir şey yazmıyor (Font kuralı, `size_items`); başlık yazılan değerin kendisi (`format_size` + birimden önce boşluk). `preview_keep` ve `download_conflict` `Choice` ile `NAMES`'ten.
- `preview_read_only` tasarım tuvalinde popup ("Read-only") ama phase dosyası switch diyor; switch seçildi ("Open read-only:").
- Klasör satırı: yol etiketi dosyanın yazımıyla (`~` açılmadan, ortadan kısaltılır), altında düğme satırı (`Form::actions`): Change… → pencereye sayfa olarak `NSOpenPanel` (o an açık sayfa varsa düşer), mevcut klasörde başlar; seçilen ev dizininin altındaysa `~/…` yazılır (`folder_text`). Önizlemede Show in Finder klasörü açar; klasör henüz yoksa bip — phase-4'ün "onaydan önce diskte bir şey yaratılmaz" kuralı, klasör ilk önizlemeyle doğuyor.
- Kullanım satırı bir ayar anahtarı değil: `Key` yok, satır `Form::plain_row` (kilit ve tanı ona uğramaz; Clear Now ve Show in Finder dosyaya yazmadığı için kilitte de açık). Ölçüm `preview_cache::usage` (süpürmenin `scan`'i: `.index`, `.index.tmp` ve `.bateri-download-*` sayılmaz) `AppDelegate::measure_preview_usage`'ta arka plan thread'inde; pencerenin her `refresh`'inde (açılış dahil — değişen `preview_dir` başka klasör) ve her süpürmenin sonunda. Sonuçlar sırasız gelebildiği için pencerede bir nesil sayacı (`show_usage`) eskiyi düşürüyor; pencere kapalıyken ölçülmüyor.
- Clear Now: `AppDelegate::sweep_previews(Sweep::ClearNow)`; süpürme thread'i artık her durumda ana kuyruğa dönüyor (kullanımı tazeler, kurtarılan varsa bugünkü sayfa/bildirim). Onay sorusu yok: değiştirilmiş kopya silinmiyor, taşınıyor. **Kullanım her zaman sıfırlanmaz:** indekste kaydı olmayan kopya (phase-1 kuralı) hiçbir tetikte silinmiyor, sayıda kalıyor.
- Pencerenin yüksekliği 560 → 680 (`WINDOW_SIZE`, tasarım sabiti): Remote Files dokuz satır, ikisi iki satırlık; 560'ta "Open settings.toml" düğmesine binerdi (hesapla, gözle görülmedi).
- `CLAUDE.md` bugünkü sözleşmeye getirildi: 044'ün bağlantı paragrafına ⌘-tık önizleme, ⌘-sürükle, sağ tık menüsünün öğeleri ve önizleme klasörünün süpürmesi; 037 paragrafına iki yönlü kuyruk ve "Show transfers (N)"; ayar listesine sekiz `[remote]` anahtarı; katman tablosunda `bt-shell-macos`'un aktarım satırı.
- Gözle kontrol (Kabul) kullanıcıda, işaretlenmedi.

### Set kapısı (`/code-review` + `/audit`)

`/audit` temiz (mekanik `audit: clean`; bağımlılık, ayar şeması, ölçüm, thread, boşta kare, dil mercekleri temiz, hücre/shader ilgisiz). `/code-review` on bulgu verdi, altısı giderildi:

- Kullanıcı önizleme klasörü olarak `~/Downloads` gibi bir klasör seçerse süpürme oradaki **boş klasörleri** siliyordu → `prune` yalnız süpürmenin sildiği/taşıdığı kopyaların boşalttığı klasörleri yukarı doğru siliyor (sınaması `the_launch_sweep_…`'te).
- `download_conflict = "replace"` altında tek **dosya** aynı adlı yerel **klasörü** sorusuz `remove_dir_all` ile siliyordu ve `rename` düşerse eski de yeni de yoktu → dosya klasörün yerine geçmiyor (hata), klasör/klasör değişimi eskiyi gizli bir kardeşe alıp yenisi yerleşince siliyor, düşerse geri koyuyor.
- Önizleme ya da Finder şeridinin hatası bütün kuyruğu (`ending`) bitirip bekleyen yüklemeleri düşürüyordu → kuyruk şeridinde bekleyen iş varsa yan şeridin hatası öğeyi listeden çıkarıp `side_failure` olarak saklanıyor; kuyruk sonunda sonuç yine hata (R2.3: yalnız kuyruk şeridi olan akış değişmedi).
- Uzak öğe symlink'se `tar` linki taşıyordu (yardımcı `stat -L` ile hedefe cevap veriyor) → öğenin kendisi link ise `tar -h`; klasör içindeki linkler link kalıyor.
- Copy as scp Path `[`/`]`'yi tırnaksız bırakıyordu (zsh'te glob) → tırnaklanıyor.
- Yardımcının çıktı okuyucusu UTF-8 olmayan ilk satırda (uzak rc'nin banner'ı) oturumu bitiriyordu → bayt okuyup kayıpsız dönüştürüyor.
- Aynı dosyaya ikinci ⌘-tık ikinci akışı başlatıp ilkinin yeni kopyasını "düzenlenmiş" sanabiliyordu → `Transfers::previewing` yoldaki önizlemeyi görüyor, ikinci tık bekliyor (sorudan önce ve şeride girmeden önce iki kez).

Giderilmeyen dört bulgu (WAIVE önerisi, karar orkestratörün):

- **Önizleme klasörü kullanıcı klasörüyse boyut ve kullanım ona göre:** indekssiz dosyalar boyut toplamına (phase-1 kuralı) ve "In use"a giriyor; `~/Downloads` seçilirse açılışta bateri'nin kendi önizlemeleri sınırdan erken siliniyor, kullanım bütün klasörü sayıyor. Kullanıcının dosyası silinmiyor (indekssiz kopya hiçbir tetikte silinmez); düzeltme planlayıcının kuralını değiştirmeyi ister.
- **Yardımcı oturum tek işçi:** büyük klasörün `Count`'u (120 sn'ye kadar) o pane'de ⌘-hover'ı ve önizlemeyi bekletiyor. Karar 10'un tek oturum tasarımı; ikinci kanal ayrı iş.
- **Günlük süpürme uykuda saymıyor:** `DispatchQueue::after` monoton saatle, uyuyan dizüstünde "günde bir" birkaç güne uzuyor; açılış süpürmesi yine koşuyor. Duvar saati tetiği ayrı iş.
- **Temizlik:** testlerin kullandığı eski tek şeritli sarmalayıcılar (`start_next`, `finish`, …) ve üç kopya `file_url` / iki kopya klasör paneli duruyor; davranış hatası değil.

Düzeltmelerden sonra `make check` ve `make linux` yeniden yeşil.
