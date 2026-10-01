# Phase 4 — Önizleme ve temizlik

## Özet

⌘-tık dosyayı önizleme klasörüne indirip salt okunur açar; önbellek açılışta,
günde bir kez ve Clear Now ile temizlenir.

_Requirements: R5, R5.1, R5.2, R5.3, R5.4, R6_

## Değişiklikler

- **`crates/bt-shell-macos/src/hyperlink.rs`** — uzak hit'te `open_link`
  `remote_files`'ın politikasına gider: dosya → önizleme, klasör → hiçbir
  şey; Open Preview menü öğesi etkin.
- **`crates/bt-shell-macos/src/preview.rs`** (yeni) — önizleme akışı: boyut
  `preview_max_size`'ı aşıyorsa sayfa (R5.2: "Open Preview", "Save to
  Downloads instead"); önbellekte aynı boyut+mtime varsa doğrudan aç (R5.4);
  yoksa önizleme şeridine iş; bitince `0444` (ayar `preview_read_only`),
  bateri'nin yazdığı boyut/mtime kaydı ve son açılış damgası (önbelleğin
  küçük indeks dosyası, `{preview_dir}/.index`), sonra açma: düz metin kolu
  `NSWorkspace.URLForApplicationToOpenContentType(public.plain-text)` +
  `openURLs:withApplicationAtURL:`, diğeri `openURL`.
- **`crates/bt-shell-macos/src/app.rs`** — açılışta temizlik (arka planda,
  planlayıcı + uygulayıcı), günde bir kez yalnız saklama (`dispatch` gecikmeli
  iş; boşta kare üretmez — kare yoluna dokunmaz), farklılaşmış kopyayı
  `download_dir`'e taşıma + bildirim. Clear Now'ın çağıracağı tek yöntem.
- **`crates/bt-shell-common/src/remote_files.rs`** — gerekiyorsa indeksin
  okuma/yazma biçimi (saf).

## Kabul

- Sınamalar: indeks round-trip; temizlik uygulayıcısı geçici bir dizinde
  (saklama, boyut sınırı en eski önce, farklılaşmış kopyanın taşınması,
  bozuk indeks → hiçbir şey silinmez).
- `make check` yeşil.
- Gözle kontrol: küçük metin dosyası ⌘-tıkta açılır ve "Read Only" görünür;
  `.sh` TextEdit'te düz metin; 100 MB üstünde soru ve "Save to Downloads
  instead"; ikinci ⌘-tık yeniden indirmez; TextEdit'te Unlock + değiştirilen
  kopya bir sonraki açılışta Downloads'a taşınıp bildirilir.

## Checklist

- [x] ⌘-tık → önizleme akışı, sınır sayfası, önbellekten açma
- [x] (phase-3'ten) Uzak bağlantı `hyperlink::Verified::remote`'ta `(uzak mutlak yol, RemoteEntry)` taşıyor; menünün gri "Open Preview"ı `openLinkFromMenu:` → `open_link`'e gidiyor: uzak kolda önizlemeye bağlanıp öğe etkinleştirilir. Hover'ın cevabı yardımcının önbelleğinden gelebilir — R5.4'ün boyut+mtime karşılaştırması yardımcıya taze sorulmalı (`remote_helper::Query::Count`)
- [x] Popover'ın "Open"ı (phase-2) bugün düz `openURL`: `remote_files::preview_open` (R5.3) üstünden geçsin; tek başına biten önizlemenin sonuç satırı (`✓ x → …/Previews/…`) ve arka plan bildirimi istenip istenmediği karara bağlansın
- [x] Salt okunur + düz metin kolu
- [x] İndeks; açılış ve günlük temizlik; farklılaşmış kopyanın taşınması
- [x] Test: Kabul listesi
- [x] Doğrulama geçti (`make check`)

## Uygulama Notları

- Diskteki yarı `bt-shell-common/src/preview_cache.rs`'te (phase dosyasının `preview.rs`'i yalnız AppKit yarısı): indeks dosyası, önbellek sorusu, mühür (`0444` + kayıt), farklılaşmış kopyanın kurtarılması ve süpürmenin uygulayıcısı platformsuz — Kabul'ün geçici dizin sınamaları orada ve `make linux`'ta koşuyor (phase-3'ün `remote_helper` emsali). İndeksin metni saf yarıda (`remote_files::PreviewIndex`: başlık `bateri-previews 1`, satır `{last_open} {size} {mtime} {göreli yol}`; tek bozuk satır bütün indeksi bozuk sayar → süpürme hiçbir şey yapmaz). Okuma-değiştirme-yazma üç thread'den (yardımcı, akış, süpürme) tek süreç kilidiyle; indeks tek `rename`'le yazılıyor.
- **Planlayıcı değişti (phase-1 kodu):** `Sweep::Launch` ve `ClearNow` farklılaşmış **her** kopyayı kurtarıyor, süresi dolmasa da — yoksa Kabul'ün "değiştirilen kopya bir sonraki açılışta Downloads'a taşınır" sahnesi varsayılan `7d`'de geçmezdi. `Daily` yalnız vadesi gelenleri (kopya açık olabilir).
- **Yeniden indirme kullanıcının düzenlemesini ezmiyor** (planda yoktu, Karar 9'un ikinci yolu): R5.4 sorusu (`remote_files::cache_state`) yerel kopya yazılan hâlden farklıysa — ya da indekste kaydı yoksa — `Diverged` diyor; kopya `Conflict::Replace`'ten önce `download_dir`'e taşınıyor (Keep both adı, yazılabilir yapılarak) ve bildiriliyor; taşınamazsa önizleme hata sayfasıyla duruyor, kopyaya dokunulmuyor.
- Kurtarma bildirimi: bateri öndeyken UN bildirimi görünmediği için (delege yok, yeni özellik bayrağı istemedik) anahtar pencerede sayfa ("Your edited preview was kept", OK / Show in Finder), arkadayken bildirim (`uploader::deliver_notification`, `notify`'ın arka plan kapısız yarısı). Önizleme sırasında kurtarma sayfası kapanınca akış sürüyor (sınır sorusu ondan sonra).
- Önizleme akışı: ana thread yol + taze `Query::Count` (hover önbelleğine bakmaz) → yardımcının thread'inde önbellek kararı (taze kopyada `last_open` tazelenir) → ana thread politika (`preview_open`, ad uzantısının UTType'ı; `x` bit taze cevaptan) ve sınır sorusu ("Open Preview" / "Cancel" Esc / solda "Save to Downloads instead" → sağ tık indirmesinin yolu) → önizleme şeridi (`Conflict::Replace`). Akış thread'i inen kopyayı mühürlüyor, ana thread satırı güncelleyip açıyor. Açma kararı pane'de iniş yoluna göre (`PreviewTicket`; `0444` yerel `x` bitini sildiği için popover'ın "Open"ı da onu okuyor, yoksa düz metin).
- Düz metin kolu `NSWorkspace URLForApplicationToOpenContentType:` + `openURLs:withApplicationAtURL:configuration:completionHandler:` çalışma zamanı üstünden (`msg_send`, `extension_content` emsali): `UTType` başlığı ve `NSWorkspaceOpenConfiguration` bayrağı açılmadı. Düz metin uygulaması bulunamazsa kopya Finder'da gösteriliyor (çalıştırılmıyor). Bip `NSBeep`'in kendi `extern` bildirimiyle — `NSGraphics` bayrağı `objc2-foundation`'a `objc2-core-foundation` kenarı ekleyip `Cargo.lock`'u oynatabilirdi. `Cargo.toml`/`Cargo.lock` değişmedi.
- **Karar (tek başına biten önizleme):** sonuç satırı kalıyor (`✓ app.log → …/Previews/prod/var/log`, LINGER — nereye indiğini söylüyor), arka plan bildirimi yalnız-önizleme kuyruğu `Done` bittiğinde **yok** (açılan pencere haberin kendisi); hata bildiriyor, önizlemenin yanında bir indirme varsa indirmenin bildirimi duruyor (`upload::end`).
- Temizlik: açılışta `load_settings`'ten hemen sonra, günlük süpürme ana kuyruğun 24 saatlik gecikmeli bloğu (`DAILY_SWEEP`, tasarım sabiti; her seferinde bir sonrakini kuruyor), iş kendi thread'inde — kare yoluna dokunmuyor, boşta sıfır kare korunuyor. Süreli koşuda (`Inputs::Hermetic`) hiç koşmuyor. Clear Now'ın tek yöntemi `AppDelegate::sweep_previews(Sweep::ClearNow)`. Süpürme önizleme klasöründe `.index`'i ve `.bateri-download-*` geçicilerini atlıyor, nokta dosyalarını (`.bashrc`) kopya sayıyor, boşalan klasörleri siliyor ve kopyası olmayan kayıtları unutuyor.
- **(e) onaydan önce diskte bir şey yaratılmıyor:** `landing` artık klasör yaratmıyor; klasör akış başlarken `download::transfer`'da (`create_dir_all`, önizlemenin `{dir}/{host}/…` ağacı da). `free_space` henüz olmayan klasörde en yakın var olan atasına soruyor (sınaması güncellendi: eskiden `None` bekliyordu).
- **(f) sonuç satırı beklerken indirme:** kod okumasıyla saf katmanda sonuç satırına özgü bir reddetme bulunamadı — kuyruk `end()`'de kalkıyor, `can_accept` doğru; yeni sınama (`a_download_during_the_result_line_starts_at_once`) indirmenin ve önizlemenin hemen başladığını ve eski LINGER'ın yeni satırı silmediğini bağlıyor. Reddin bugün iki gerçek hâli var: bir sayfa/yoklama sürerken (`asking`, iki sayfa üst üste açılamaz) ve durdurulan kuyruğun akan öğesi henüz bitmemişken (`ending`). `can_accept` değişmedi (R2.3; o iki hâlde kabul etmek işi park etmeyi ve upload sınamalarını değiştirmeyi isterdi); onun yerine **hiçbir red artık sessiz değil**: `download_remote`, önizleme, onaylanıp kuyruğa giremeyen iş (`upload_confirmed` artık `bool`) ve oturumu biten cevap bip çalıyor ya da sebebi sayfada söylüyor. Kullanıcıdaki belirti başka bir yoldan geliyorsa gözle kontrolde macOS akışı (`accepts_drop`'un `upload_stop`'u, `download_prepared`'ın erken dönüşleri) izlenmeli.
- Gözle kontrol (Kabul) gerçek ssh sunucusu ve kullanıcı istiyor, işaretlenmedi: küçük metin dosyası ⌘-tıkta açılır ve "Read Only"; `.sh` TextEdit'te düz metin; 100 MB üstünde soru ve "Save to Downloads instead"; ikinci ⌘-tık yeniden indirmez; TextEdit'te Unlock + değişiklik → sonraki açılışta Downloads'a taşınır ve sayfa/bildirim; sonuç satırı beklerken sağ tık › Download başlar.
- `make check` ve `make linux` son iki küçük düzeltmeden önce yeşil koştu (düz metin açmanın `completionHandler`'ı blok tipiyle — `@?` kodlaması —, kurtarma sayfasından sonra akışın bir ana kuyruk turu ertelenmesi); onlardan sonra `cargo fmt` + `cargo clippy -p bt-shell-macos -D warnings` temiz.
