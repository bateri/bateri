# Phase 3 — Tazelik, bozuk dosya ve belgeler

## Özet

Pencereyi dosyanın hâline bağlamak — dışarıdan değişim, kabul edilmeyen
değer, ayrıştırılamayan dosya, yazma hatası — ve sözleşmeyi belgelere
işlemek.

_Requirements: R6, R7_

## Değişiklikler

- **`crates/bt-shell/src/settings.rs`** — `Loaded::live`'ın tüketicisine
  pencerenin ihtiyacı olan hâl (`Usable` / `Missing` / `Locked(sebep)`) ve
  tanıların anahtarları (`Diagnostic::key`) kaybolmadan ulaşır; mevcut
  alt başlık metni aynı kaynaktan (iki metin üretilmez).
- **`crates/bt-shell/src/app.rs`** — `reload_settings` her dalda (başarılı
  okuma, ayrıştırılamayan dosya, okunamayan dosya) açık pencereyi tazeler:
  etkin ayar, yükleme hâli, satır tanıları, güncel tema adları. Yazma hatası
  `save_edit`'ten pencereye de gider.
- **`crates/bt-shell/src/settings_window.rs`**
  - Kilit hâli: bütün kontroller devre dışı, sağ bölmenin üstünde şerit
    (sebep + "Fix the file and save it; this window follows."), "Open
    settings.toml" varsayılan düğme. Hâl düzelince şerit kalkar.
  - Satır tanısı: `Diagnostic::key`'i o satıra eşle, açıklamanın yerine tanı
    (uyarı renginde, küçük); başka bir tazelemede kalkar.
  - Yazma hatası: şeritte, bir sonraki başarılı yazmaya ya da tazelemeye
    kadar; kontrol dosyadaki değere döner (tazeleme zaten döndürüyor).
- **`docs/AYARLAR.md`** — "Dosyanın yeri" ve "Settings…" bölümleri: Cmd-,
  pencereyi açar, "Open settings.toml" bugünkü davranış; "Uygulama bu dosyaya
  iki yerden yazar" → üç (pencere de yalnız değiştirdiği satırı yazar, dosya
  yoksa şablonla yaratır, bozuk dosyaya yazmaz ve kilitlenir); yazma
  zamanlaması (slider bırakınca, sayı onaylayınca) ve `scrollback`
  küçültmesinin anlık silmesi pencerede de geçerli. `### Şablon` bloğu
  değişmez.
- **`CLAUDE.md`** — `bt-shell` satırında "ayar penceresi" ve Ayarlar
  maddesinde "Dosyaya yazan iki yol var" cümlesi (üç; pencerenin kuralı tek
  cümle + işaretçi `.tasks/029-ayarlar-penceresi/discussion.md`); Bugünkü hâl
  paragrafındaki menü listesi ("Settings…") gerekiyorsa.
- **`docs/YOL-HARITASI.md`** — 029 satırı (set açılışında yazıldı) dokunulmaz;
  "Var olan ayar dosyası yeni anahtarları hiç görmüyor" borcuna tek cümle:
  pencere eksik anahtarı yorumsuz ekliyor, doldurma borcu yerinde.

## Kabul

- Sınama (saf): yükleme hâli + tanılar → pencerenin göreceği model (kilit mi,
  hangi satırda hangi tanı) üç hâlde doğru.
- Gözle: pencere açıkken editörde `cursor = "beam"` kaydetmek popup'ı Beam
  yapar; `cursor = "bar"` Shape satırının altında tanı gösterir, popup
  ekrandaki değerde kalır; dosyayı bozmak (`[terminal` ) şeridi ve kilidi
  getirir, düzeltip kaydetmek kaldırır; `themes/`'e yeni dosya koymak Theme
  popup'ına ekler; salt okunur dosyada seçim şeritte hata gösterir ve
  kontrol eski değere döner.
- `make hepsi` yeşil; `make duman` yeşil.

## Checklist

- [x] Hâl + tanı modeli, alt başlıkla tek kaynak
- [x] `reload_settings` her dalda tazeliyor; yazma hatası pencerede
- [x] Kilit şeridi, satır tanısı
- [x] `docs/AYARLAR.md`, `CLAUDE.md`, `docs/YOL-HARITASI.md` borç notu
- [x] Test: hâl/tanı modeli üç hâlde
- [x] Doğrulama geçti (`make hepsi` + `make duman`)

## Uygulama Notları

- **`live()`/`at_launch()`'ın imzası değişmedi**, yanlarına `Loaded::state()`
  → `settings::FileState` (`Missing` / `Locked(metin)` / `Usable(tanılar)`)
  geldi ve ikisinin iletileri artık `state().notices()`'ten: şerit ile alt
  başlık yapısal olarak aynı metin. Hâl `AppDelegate`'te bir ivar
  (`settings_state`), açılışta da yazılıyor — bozuk dosyayla açılan
  uygulamada pencere ilk açılışta kilitli.
- **Yazma hatası ayrı bir yuva değil**: pencere alt başlığın yazma yuvasını
  okuyor; o yuva başarılı yazmada ve dosya okunup uygulanınca boşalıyor.
- **Satıra düşmeyen tanı şeritte** (phase metninde yoktu): bölüm olmayan
  bölüm (`terminal = 5`) ve emekli `shell.prompt` alt başlıkta görünüp
  pencerede görünmeseydi kullanıcı iki yere bakardı. Satırın tanısı yalnız
  iletisi (satır numarası ve dosya adı yok); şeritteki alt başlık biçiminde.
- **Satır ↔ anahtar eşlemesi `Key::path`'te**, ayrıştırıcıyla bağı bir
  sınama tutuyor: bütün anahtarları yanlış türde yazan dosyanın 19 tanısı
  19 satıra bire bir düşüyor; yazma tarafı da `SettingsEdit::path` (yeni,
  `place()`'in yolu) ile aynı sınamada bağlı (`/code-review` bulgusu: yol üç
  yerde yazılıydı, ikisi bağlıydı).
- **Her satırın gizli bir not satırı var**; kilit ve bağımlı satır tek
  kapıdan (`Row::set_enabled`), kapalı satırın açıklaması da soluyor.
  `set_enabled` serbest fonksiyonu ve üç etiket alanı kalktı.
- **Pencerenin boyu 500 → 560**: şeritli Cursor bölmesinin son açıklaması
  "Open settings.toml"a yapışıyordu (gözle).
- Şerit `NSBox` (özel tip, sistemin turuncusunun %10'u zemin, %35'i kenar):
  `objc2-app-kit`'e `NSBox` başlık bayrağı, `Cargo.lock` oynamadı.
- Canlı okumada dosya yok olursa pencere **etkin** ayarı gösterir
  (varsayılanları değil): `live()`'ın editör kaydı gerekçesi; Karar 7'nin
  "varsayılanları gösterir"i açılış hâli.
- **Gözle** (debug derlemesi geçici bir `.app` sarmalında, `HOME` geçici):
  editörde `cursor = "beam"` → popup Beam; `cursor = "bar"` → Shape altında
  turuncu tanı, popup etkin değerde; `[terminal` → şerit + bütün kontroller
  kapalı + düğme mavi, düzeltince kalkıyor; `themes/paper.toml` → Theme
  listesinde; salt okunur dosyada tema seçimi → şeritte "could not be
  written: Permission denied", popup Match System'e döndü; tanılı dosyayla
  açılışta emekli anahtar ilk açılışta şeritte; Beam seçmek satır tanısını kaldırdı.
- **Set kapısı `/code-review` düzeltmeleri:** değişmemiş sayı alanından
  geçmek (Tab) artık yazmıyor (yuvarlanmış yazılış `1.125`'i `1.13` yapardı);
  tazeleme düzenlenmekte olan alana dokunmuyor, reddedilen girdi alanı
  doğrudan geri alıyor; stepper'ın aralığı dosyadaki aralık dışı değeri
  kapsıyor (`size = 100`'de "yukarı" küçültüyordu); slider basınç ve
  periyodik olayı da jestin ortası sayıyor; pencere yalnız ilk açılışta
  ortalanıyor; kapalı pencere tazelenmiyor (açılış tazeliyor); Open
  settings.toml'un hatası yazma yuvasına, yani şeride de gidiyor (terminal
  penceresi yokken hiçbir yerde görünmüyordu).
- **Waive (`/code-review`):** `with_theme` üretimde çağrılmıyor ama planın
  R1'i onu `with_edit`'in çağıranı olarak tutuyor ve menünün sınamaları onun
  üstünde — kaldırmak plan dışı. `monospaced_families` pencerenin ilk
  doğuşunda ana thread'de bir kez koşuyor (phase-2 kararı, süreç başına bir
  kez); maliyeti ölçülmedi, iddia yazılmıyor.
