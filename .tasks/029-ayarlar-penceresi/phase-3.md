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

- [ ] Hâl + tanı modeli, alt başlıkla tek kaynak
- [ ] `reload_settings` her dalda tazeliyor; yazma hatası pencerede
- [ ] Kilit şeridi, satır tanısı
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`, `docs/YOL-HARITASI.md` borç notu
- [ ] Test: hâl/tanı modeli üç hâlde
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
