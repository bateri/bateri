# Phase 7 — Görünüm menüsü: tema seçimi ve punto

## Özet

Görünüm ▸ Tema ▸ ile temayı menüden seç ve dosyaya biçimini bozmadan yaz;
Cmd +/−/0 ile geçici punto.

_Requirements: R8, R1, R1.2, R2, R10_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — **biçim koruyan yazma**, saf: metin
  + (anahtar yolu, değer) → yeni metin. `toml_edit` belgesi üstünde;
  kullanıcının yorumları, boş satırları, anahtar sırası ve tanımadığımız
  anahtarlar yerinde kalır; bölüm yoksa eklenir. Ayrıştırılamayan metin →
  hata, metin **üretilmez**. `toml_edit` yalnız `bt-core`'da kalır.
- **Kök `Cargo.toml`** — `toml_edit`'e `display` feature'ı (yazma onu
  ister): `toml_writer` `Cargo.lock`'a girer, **phase riskli** (phase-1
  yalnız `parse`'ı açtı, Uygulama Notları).
- **`crates/bt-shell/src/menu.rs`** — **View** menüsü:
  - **Theme ▸** alt menüsü, `NSMenuDelegate`'in `menuNeedsUpdate:`'i ile
    **açılırken** doldurulur: "Match System", ayırıcı, gömülü temalar,
    `themes/*.toml` adları. Seçili olan işaretli (etkin ayardan). `themes/`
    liste için izlenmez.
  - **Bigger** (Cmd +), **Smaller** (Cmd −), **Actual Size** (Cmd 0).
  - Hermetik dalda Tema ▸ doldurulmaz (dördüncü giriş; dal yine tek).
- **`crates/bt-shell/src/app.rs`** —
  - **Tema seçimi yalnız dosyaya yazar**, uygulamaz — uygulamayı izleyici
    yapar (tek zincir). "Match System" → `theme = "system"`; ad → `theme =
    "{ad}"`; `light_theme`/`dark_theme`'e dokunulmaz, çift yerinde kalır.
  - Yazma sırası: dosya **o anda** okunur → `bt-core` yazma fonksiyonu →
    **yerinde** yazılır (symlink'li dosyada hedefe; geçici dosya + rename
    yok, bağı koparırdı). Dosya yoksa "Ayarlar…"ın oluşturma yolu (şablon +
    anahtar) ve izleme yeniden kurulur.
  - Ayrıştırılamayan dosyaya **yazılmaz**; yazma ya da okuma hatası →
    **yazma yuvası** (alt başlığa eklenir). Yuva bir sonraki başarılı
    yazmada boşalır.
  - **Geçici punto:** Bigger/Smaller ayardaki `size`'ın üstüne bir fark
    tutar, Actual Size farkı sıfırlar; renderer'a ayar + fark gider.
    Adım bir tasarım sabiti. **Dosyadaki `size` değişince fark sıfırlanır**
    (fark fonksiyonu görür) — iki punto kaynağı yarışmaz. Fark dosyaya
    yazılmaz.
- **`docs/AYARLAR.md`** — menüden tema seçimi ve dosyaya ne yazdığı,
  yorumların korunması, symlink'li dosya; Cmd +/−/0'ın geçici olduğu.

## Kabul

`bt-core` yazma sınamaları:

- Yorumlu ve bilinmeyen anahtarlı bir dosyada `theme` değişir, geri kalan
  bayt bayt aynı.
- `[appearance]` bölümü yoksa eklenir; boş metinde de çalışır.
- `light_theme`/`dark_theme` varken `theme` yazmak onlara dokunmaz.
- Ayrıştırılamayan metin → hata.

`bt-shell` yazma sınamaları (geçici dizin):

- Symlink'li `settings.toml`: hedef güncellenir, bağ symlink olarak kalır.
- Ayrıştırılamayan dosya: içerik değişmez, yazma yuvası dolar.
- Dosya yok: şablon + anahtar oluşur.

Geçici punto: fark ayar değişince sıfırlanır; Actual Size farkı sıfırlar.

- `make duman` jetonları değişmez.
- Göz:
  - Görünüm ▸ Tema ▸ temaları listeler, seçili işaretli; seçmek pencereyi
    değiştirir ve yeniden açılışta aynı tema gelir.
  - "Match System"e dönmek açık/koyu çiftini geri getirir.
  - `themes/`'e yeni dosya koymak menüyü bir sonraki açılışta günceller.
  - Cmd +/−/0 çalışır; editörde `size` değiştirmek geçici puntoyu bırakır.

## Yayın Etkisi

- **ayar şeması** — uygulama artık `settings.toml`'a **yazıyor** (yalnız
  `appearance.theme`); "tanımadığını bırakır" sözleşmesi sınamayla bağlı.
- `CLAUDE.md` "Ayarlar" maddesi: dosyayı yazan ayar penceresi değil, Tema
  menüsü.
- **bağımlılık feature'ı** — `toml_edit` `display`; `Cargo.lock`'a
  `toml_writer` girer (kararın kendisi phase-1'de kayıtlı, crate aynı).

## Checklist

- [ ] `bt-core` biçim koruyan yazma
- [ ] View menüsü: Theme ▸ (`menuNeedsUpdate:`), punto öğeleri; hermetik dal
- [ ] Tema seçimi → taze oku, yaz, yerinde; ayrıştırılamayan dosyada ret
- [ ] Yazma yuvası
- [ ] Geçici punto farkı ve ayar değişiminde sıfırlanması
- [ ] Test: yazma (yorum, bölüm, çift, ret), symlink, dosya yok, punto farkı
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (`Cargo.lock`: `toml_writer`)
- [ ] Yayın etkisi yazıldı
