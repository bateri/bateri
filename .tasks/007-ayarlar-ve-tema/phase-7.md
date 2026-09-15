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

- **ayar şeması** — uygulama artık `settings.toml`'a **yazıyor**, yalnız
  `[appearance] theme`; yeni anahtar yok. "Tanımadığını bırakır" sözleşmesi
  sınamayla bağlı (`theme_write_keeps_every_other_byte`). Bilinen yan etki:
  dosya sonunda satır sonu yoksa eklenir — `docs/AYARLAR.md` → View ▸ Theme ▸.
- **bağımlılık feature'ı** — `toml_edit` `display`; `Cargo.lock`'a
  `toml_writer` (MIT OR Apache-2.0) girdi. Kararın kendisi phase-1'de
  kayıtlı, crate aynı; lisans borcu listesine adıyla eklendi
  (`docs/YOL-HARITASI.md`).
- **`bt-core` pub API** — `Settings::with_theme`, `SYSTEM_THEME` (`pub`),
  `Theme::embedded_names`.
- **Klavye** — View ▸ Bigger/Smaller/Actual Size (Cmd +/−/0); Türkçe-QWERTY-PC
  düzeninde AppKit Bigger'ı `⌘:` gösteriyor (Metalterm'le aynı). AppKit'in
  pencere sekmeleri kapandı (View menüsündeki Show Tab Bar / Show All Tabs).
- **Bekleyen göz kontrolü** `[elle]`: Cmd +/−/0 tuşları (menü tıklamasıyla
  sınandı, tuşla değil) ve Cmd tuşlarında Theme ▸'nin yeniden dolmaması.
- `CLAUDE.md` bugünkü hâl ve "Ayarlar" maddesi, `bt-shell` başlık yorumu
  güncellendi.
- `make duman` jetonları değişmedi: `kare=1 hucre=8 glif=6 kural=15`.

## Checklist

- [x] `bt-core` biçim koruyan yazma
- [x] View menüsü: Theme ▸ (`menuNeedsUpdate:`), punto öğeleri; hermetik dal
- [x] Tema seçimi → taze oku, yaz, yerinde; ayrıştırılamayan dosyada ret
- [x] Yazma yuvası
- [x] Geçici punto farkı ve ayar değişiminde sıfırlanması
- [x] Test: yazma (yorum, bölüm, çift, ret), symlink, dosya yok, punto farkı
- [x] `docs/AYARLAR.md`, `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (`Cargo.lock`: `toml_writer`) — dördün üçü düzeldi, biri waive (Uygulama Notları)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Geçici punto ayrı modülde** (`zoom.rs`; plan: `app.rs`), saf ve
  sınanıyor. Adım 1 punto. **Basışların aralığı 4–72 punto** (planda yok):
  dışındaki basış atlasın `punto × ölçek` kırpmasına çarpıp görünmezdi ve
  tuşu basılı tutan kullanıcı geri dönmek için onları tek tek geri basardı.
  Ayardaki `size` aralığa bağlı değil; dışındaysa basış yalnız aralığa doğru.
- **Fark `size` değişince sıfırlanıyor, `Changes`'e alan eklenmedi**
  (plan: "fark fonksiyonu görür"): `Zoom::after_reload` eski ve yeni fontu
  karşılaştırıyor. Aile değişimi farkı koruyor.
- **Seçimden sonra dosya hemen okunuyor** (`reload_settings`; plan:
  "uygulamayı izleyici yapar"): dizin yeni yaratıldıysa onu gören kaynak
  yok ("Settings…"ın gerekçesi). Menünün kendi uygulama yolu yine yok;
  ardından gelen vnode olayı boş fark ve no-op tema takası.
- **Yazma yuvası dosya okunup uygulandığında da boşalıyor** (plan: yalnız
  bir sonraki başarılı yazmada). Düzeltilmiş dosyada "…invalid TOML…; the
  theme was not saved" dosya hâlâ bozukmuş gibi okunurdu. Yuva alt başlıkta
  **ilk** sırada: kullanıcının az önceki tıklamasının cevabı.
- **Ret genişledi:** ayrıştırılamayan metnin yanında `appearance` bölüm
  değilse (`appearance = 1`, `[[appearance]]`) ya da `theme` bir bölümse de
  `Err` — üstüne yazmak içeriği silerdi. Kabul edilmeyen türdeki
  `theme = 3` değişir. Değerin yanındaki yorum için değerin süsü yeni değere
  taşınıyor (`toml_edit` atamada düşürüyor).
- **`toml_edit` satır sonlarını LF yazıyor:** ilk satırı CRLF olan dosyada
  çıktı CRLF'ye geri çevriliyor; son satıra satır sonu eklenmesi kaldı
  (belgede).
- **Dosya yoksa önce `create_if_missing`, sonra taze okuma + yazma** —
  iki yazma (şablon, anahtar), tek yol; hedefi olmayan bağ böylece
  "Settings…"la aynı kuraldan geçiyor.
- **Tema öğesinin değeri başlığı** (`selectTheme:`), "Match System" ayrı
  eylem. Liste `settings::user_theme_names`: `.toml` olmayan, nokta ile
  başlayan (AppleDouble `._x`), `system`, gömülü adı gölgeleyen ve düz dosya
  olmayan girdiler yok; sıra harf duyarsız. `Theme::embedded_names()`
  eklendi, `theme.rs`'in belge sınaması kendi kopyası yerine onu okuyor.
- **`menuHasKeyEquivalent:forEvent:target:action:` elle** (planda yok):
  tanımlanmazsa AppKit her Command'lı tuşta karşılık aramak için
  `menuNeedsUpdate:`'i çağırır, her tuşta `themes/` okunurdu.
  `objc2-app-kit` yöntemi üretmiyor; `Sel` işaretçi kodlaması taşımadığı için
  iki çıkış argümanı `*mut c_void`. Tuşla **sınanmadı**.
- **AppKit'in pencere sekmeleri kapatıldı** (planda yok): "View" adlı menü
  doğunca AppKit Show Tab Bar / Show All Tabs ekledi, tek pencerede boş sekme
  çubuğu açıyordu. "Enter Full Screen" yerinde.
- **Kısayol yerelleştirmesi:** Türkçe-QWERTY-PC düzeninde AppKit Bigger'ın
  `+`'sını `⌘:` gösteriyor; aynı makinede Metalterm ("Increase Font Size")
  ve Claude ("Zoom In") de `⌘:`. `-` ve `0` değişmedi. Dokunulmadı.
- **`NSCell` bayrağı** `bt-shell/Cargo.toml`'da: `NSMenuItem::setState`'in
  tipi (`NSControlStateValue`) orada.
- **Test-first:** `bt-core`'un dört yazma, `bt-shell`'in beş yazma/liste ve
  `zoom`'un iki sınaması iskelete karşı düştü; `after_reload` sınaması
  iskelette boşuna geçti (`bigger` de iskeletti).
- **Göz kontrolü** geçici `HOME` + sembolik bağlı, yorumlu `settings.toml` +
  `themes/paper.toml`; menüler System Events'le **tıklanarak**, tuş
  gönderilmeden. Gözlenen: Theme ▸ listesi (Match System işaretli, gömülüler,
  ayırıcı, `paper`); `paper` seçimi pencereyi anında boyadı, dosya bağın
  hedefinde yerinde yazıldı (yorum kaldı, bağ bağ); Bigger ×5 büyüttü;
  dosyaya `size = 11` yazmak farkı bıraktı; Actual Size ve Match System
  (`theme = "system"`, açık görünümde `bateri-light`); bozuk dosyada seçim
  dosyayı değiştirmedi (md5 aynı), alt başlık "…; the theme was not saved
  (+1 more)", düzeltilen kayıt iki yuvayı da boşalttı; `themes/ink.toml`
  bir sonraki açılışta listede; `~/.config/bateri` silinmişken seçim dizini
  ve şablonu yarattı, temayı uyguladı, sonra kabuktan yazılan dosyayı
  izleyici gördü; bateri ▸ Quit temiz, stderr'de yalnız beklenen iki satır.
  **Düzeneğin tuzağı:** AX ile menü çubuğu öğesini açıp bırakınca menü
  izleme döngüsünde kaldı ve sonraki öğe tıklamaları işlemedi (`AXCancel`
  ile kapanınca düzeldi) — uygulamanın değil sınamanın izi.
- **`/code-review` (high), dört bulgu.**
  - *Orta, düzeltildi:* `[appearance]` yokken yeni bölüm belgenin
    **kuyruğundaki** yorumun önüne ekleniyordu; `[font]`'un altındaki
    `# family = "Menlo"` `[appearance]`'a geçer, yorumu kaldıran kullanıcının
    satırı sessizce yoksayılırdı. Kuyruk yeni başlığın önüne alınıyor
    (`theme_write_leaves_trailing_comments_where_they_were`).
  - *Düşük, düzeltildi:* CRLF dosya tek seçimde bütünüyle LF'ye dönüyordu
    (`theme_write_keeps_crlf_line_endings`).
  - *Düşük, düzeltildi:* adı tam `.toml` olan dosya boş başlıklı öğe olarak
    listeleniyordu (gizli dosya sınaması gövdeye bakıyordu), seçilince
    `theme = ""` yazardı.
  - *Düşük, waive:* yerinde yazma önce boşaltıyor; yazma hata verirse ya da
    süreç o anda ölürse dosya boş kalır. Önerilen çözülmüş hedefe geçici
    dosya + yeniden adlandırma pencereyi kapatır ama sabit bağı koparır,
    izinleri ve genişletilmiş öznitelikleri düşürür, yazılamayan dizinde
    başarısız olur; yerinde yazma Karar 7'nin panelden geçmiş hâli ve pencere
    tek bir 1 KB'ın altındaki `write`. Sınır `settings::write_theme`'in
    doc'unda.
