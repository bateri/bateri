# Ayarlar penceresi

## Hedef

bateri ▸ Settings… (Cmd-,) yerel bir macOS ayar penceresi açsın: solda dört
kategorili kenar çubuğu, sağda anlaşılır satırlar. Pencere `settings.toml`'a
yorumları ve bilinmeyen anahtarları koruyarak yazar, uygulayan bugünkü izleme
yoludur; dosya dışarıdan değişince pencere de değişir.

## Gereksinimler

- **R1** — Yazma yolu `bt-core`'da saf ve tipli: tek anahtarı değiştiren
  düzenleme, `with_theme`'in bütün güvenceleriyle; `with_theme` onun çağıranı.
  - **R1.1** — Yorum, sıra, bilinmeyen anahtar, süs, CRLF korunur; bölüm/anahtar
    yoksa eklenir; ayrıştırılamayan metin ve bölüm-olmayan bölüm/bölüm-olan
    anahtar `Err`. Her anahtar türü (dizge enum, tamsayı, ondalık, tema adı,
    aile) için round-trip sınaması.
  - **R1.2** — Yazılan değer ayrıştırıcıdan geri aynı değer olarak okunur
    (yaz → `parse` → eşit), ondalıklar iki basamakta.
- **R2** — Geçerli değerlerin tek kaynağı: her dizge enum'unun yazılış tablosu
  ve her aralık `pub`, ayrıştırıcı ve `name()` aynı tablodan okur; tanı
  metinleri değişmez.
- **R3** — Eşaralıklı ailelerin listesi `bt-atlas`'ta, uyarının kullandığı
  ölçütle; `bt-gpu`'dan ihraç.
- **R4** — Pencere: kenar çubuğu (General / Appearance / Cursor / Motion, SF
  Symbols), sağda etiket–kontrol ızgarası, açıklamalar; tek örnek, sekme
  almaz, terminal pencere listesine girmez.
  - **R4.1** — Satırlar ve kontroller `discussion.md` → Karar 2–6, 9; Font
    popup'ının varsayılan öğesi ve listede olmayan aile (Karar 3).
  - **R4.2** — Yazma zamanlaması Karar 5; her yazma hemen uygulanır.
- **R5** — Cmd-, pencereyi açar; "Open settings.toml" bugünkü
  `edit_settings`'i koşar; süreli koşuda Settings… hiçbir şey yapmaz ve `make
  duman` yeşil.
- **R6** — Tazelik ve bozuk dosya: dışarıdan değişim pencereyi tazeler,
  ayrıştırılamayan/okunamayan dosyada kilit + sebep, kabul edilmeyen değerin
  tanısı kendi satırında, yazma hatası pencerede de görünür (Karar 7).
- **R7** — Belgeler: `docs/AYARLAR.md` (Settings… artık pencere, dosyaya
  yazan yollar üç), `CLAUDE.md`'nin Ayarlar maddesi ve `bt-shell` satırı,
  `menu.rs`'in başlığı; yol haritasının "beş kopya" borcu kapandı diye
  güncellenir.

## Yaklaşım

1. `bt-core::settings`: yazılış tabloları ve aralıklar `pub`; `SettingsEdit`
   + `Settings::with_edit`; `with_theme` → `with_edit(Theme)`. `bt-shell`
   `settings::write_theme` → `write_edit` (aynı okuma/yaratma/yerinde yazma).
   `bt-atlas` aile listesi. Görünür davranış değişmez.
2. `bt-shell/src/settings_window.rs`: pencere, kenar çubuğu, dört bölme ve
   bütün kontroller; `AppDelegate` ivar'ı, Cmd-, yeniden bağlanır, "Open
   settings.toml" düğmesi. Bayraklar `bt-shell/Cargo.toml`'da gerekçeli.
3. Tazelik: `reload_settings` açık pencereyi tazeler (değerler, tema listesi,
   satır tanıları, kilit şeridi); yazma hatası şeritte; belgeler.

## Kapsam Dışı

- **Var olan dosyayı yeni anahtarlarla ve yorumlarıyla doldurmak** (yol
  haritası borcu "Var olan ayar dosyası yeni anahtarları hiç görmüyor"):
  pencere ilk değişiklikte yalnız o anahtarı yorumsuz ekler (`with_theme`
  emsali), başka bir satıra dokunmaz. Borç yerinde kalır.
- Tema **düzenleyicisi** (renk seçiciler), yeni tema yaratma, materyal.
- Geçici punto (Cmd +/−) pencerede görünmez; Size dosyanın değeridir.
- Kısayol, bölme, arama gibi henüz olmayan ayarlar; referansın öteki
  anahtarları.
- Ayar penceresinde geri al (⌘Z) ve "varsayılanlara dön" düğmesi.
- `docs/AYARLAR.md`'nin bölüm örneklerini sınamaya bağlamak (ayrı borç).

## Akış

```
kontrol değişti ──► AppDelegate::save_edit(SettingsEdit)
                      │  settings::write_edit (oku → with_edit → yerinde yaz)
                      ├─ Err → alt başlık Source::Write + pencere şeridi
                      └─ Ok  → reload_settings()
                                 │  (vnode olayı da buraya gelir)
                                 ├─ Settings::changes → terminal pencereleri
                                 └─ settings_window.refresh(etkin ayar,
                                        yükleme hâli, tanılar, tema adları)
Cmd-, ──► settings_window.show()   (Hermetic: no-op)
"Open settings.toml" ──► edit_settings()  (bugünkü yol)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| phase-3 | |
| kapı | |
