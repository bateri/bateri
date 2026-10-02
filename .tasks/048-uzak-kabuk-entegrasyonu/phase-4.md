# Phase 4 — Ayar penceresi ve Shell menüsü

## Özet

Anahtarı arayüze taşı: Remote Files sayfasında genel onay kutusu, Shell
menüsünde host başına aç/kapa.

_Requirements: R6_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `SettingsEdit`'e genel anahtar ve
  host başına `integration` girdisi (`host_mark_plan`'ın emsali: tek girdi,
  biçim korunarak).
- **`crates/bt-shell-macos/src/settings_window.rs`** — Remote Files'ta
  "Set up shell integration on servers" onay kutusu, altında prod
  varsayılanını söyleyen tek satır.
- **`crates/bt-shell-macos/src/`** (Shell menüsü, "Mark “{host}” as ▸"ın
  yanı) — "Shell Integration on “{host}”" aç/kapa; durum ayarın çözümünden.
- **`docs/AYARLAR.md`** — arayüz satırları.

## Kabul

- `SettingsEdit` round-trip: bilinmeyen anahtar ve yorum korunuyor, host
  girdisi `mark`'ı bozmuyor.
- Menü öğesinin durumu çözüm sırasını izliyor (prod işaretli host'ta kapalı
  görünür).
- `make check` yeşil.
- Gözle kontrol: menüden kapat → bir sonraki `ssh` düz; ayar penceresinde
  kutu dosyayla eşzamanlı.

## Uygulama Notları

- **İki varyant**: `SettingsEdit::RemoteIntegration(bool)` (genel anahtar,
  `with_edit`'in tek-değer yolu) ve `RemoteHostIntegration { host, on }`
  (dizi düzenlemesi, `with_host_integration`; kural saf
  `host_integration_plan`, `host_mark_plan`'ın ikizi). "Tam bu host'un
  girdisi" iki kuralda tek yardımcıdan (`exact_entries`), dizinin okunuşu da
  (`remote_rules`); başa yazma iki yazılışta adlı yardımcılarda
  (`prepend_entry`, `prepend_table`; artık `MarkPlan` değil işaret/entegrasyon
  alıyor).
- **Menü host'un kendi kararını yazar, varsayılana göre değil**: no-op
  ölçütü `integration_rule`'un (açık girdi) cevabı, `integration_for`'un
  değil. Menü bir aç/kapa ve çözümün tersini yazıyor; yazılan değer
  varsayılana eşit olsa da açık girdi doğru yazım — sonradan değişen genel
  anahtar ya da işaret kullanıcının bu host için verdiği kararı ezmesin.
- **Gölgede kalan girdinin `integration`'ı silinir, boş girdi doğmaz**:
  önünde `integration` taşıyan bir glob varsa başa `{ host, integration }`
  yazılıyor ve o host'un geride kalan tam girdileri `integration`'ını
  kaybediyor; işaretli olan işaretini koruyor, yalnız `integration`
  taşıyan bütünüyle siliniyor. Ne `mark` ne `integration` taşıyan girdi
  listenin tamamını reddederdi ve reddedilen liste (phase-1) entegrasyonu
  her host'ta kapatır, prod işaretlerini de götürürdü.
- **`/code-review` (medium) iki düşük bulgu**: (1) yalnız `integration`
  taşıyan ama tanımadığımız bir anahtarı da olan gölgedeki girdi
  siliniyordu (`note = "lab box"` kaybolurdu) — artık **dokunulmuyor**:
  silmek anahtarı, `integration`'ı söküp bırakmak ne `mark` ne
  `integration` taşıyan, listeyi reddettiren bir girdi doğururdu; gölgede
  kaldığı için etkisiz. (2) silinen ilk `[[remote.hosts]]` bölümünün
  üstündeki yorumun kaybı bu kolda **erişilemez**: 0. indeksteki tam girdinin
  önünde glob olamaz, yani yerinde düzenleme her zaman tutar ve 0. indeks
  hiç silinmez. Aynı kusur `with_host_mark`'ta erişilebilir (037'den beri);
  bu phase'in kapsamı değil.
- **Satır içi tabloda boşluk düzeltmesi** (`inline_insert`/`inline_remove`):
  `toml_edit` `}`'den önceki boşluğu son değerin süsünde tutuyor ve araya
  eklenen anahtar `mark = "production" , integration = true }` yazıyordu.
  phase-1'in "yalnız `integration` taşıyan girdiye `mark` ekle" kolu da aynı
  kusuru taşıyordu; ikisi de yardımcıdan geçiyor.
- **`with_edit`'in tip tanısına `Boolean` kolu**: üç `bool` anahtarın
  reddi "must be a number" diyordu.
- **"Onay kutusu" bir `NSSwitch`**: pencerenin bütün `bool` satırları
  anahtar (Open read-only, Notify when done); kutu tek istisna olurdu. Satır
  Remote Files'ın en üstünde, notu prod varsayılanını söylüyor. `Key` sona
  eklendi (`tag` sırası değişmesin).
- **Menü öğesi düz bir öğe, alt menü değil**: hedefi app delegate
  (`toggleHostIntegration:`), başlığı/onay işareti/gri hâli app delegate'in
  `validateMenuItem:`'ından (saf model `menu::integration_menu`). Mark'ın
  tutucusu elle gri yapılıyordu çünkü alt menü tutucusu doğrulamadan
  geçmiyor; düz öğe geçiyor, yani yerel sekmede `false` dönmek yetiyor.
  Ayar penceresi key iken de gri (key terminal penceresi yok).
- **Gözle kontrol yapılmadı**: menüden kapatıp bir sonraki `ssh`'ın düz
  açıldığını görmek gerçek bir uzak host ister. Yerine sınanan: menü
  modelinin çözümü izlemesi (prod glob'unda kapalı, host girdisiyle açık),
  yazılan dosyanın `integration_for`'u tersine çevirmesi, ayar penceresi
  satırının tanı/düzenleme yolunun `remote.integration`'a bağlı olması
  (`every_row_receives_its_own_diagnostic`); `make smoke` menüyü kuruyor.
  Ayar her `ssh`'ta o an okunduğu için (phase-1) "bir sonraki ssh" ayrıca
  bir yol istemiyor.

## Checklist

- [x] `SettingsEdit` girdileri
- [x] Ayar penceresi
- [x] Shell menüsü
- [x] `docs/AYARLAR.md`
- [x] Test: round-trip, menü durumu
- [x] Doğrulama geçti (`make check` + `make linux` + `make smoke`)
