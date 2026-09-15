# Phase 5 — Canlı font

## Özet

`[font] family` ve `size`'ı ayardan oku, canlı uygula; bulunamayan aileyi
pencerede söyle.

_Requirements: R6, R2, R10_

## Değişiklikler

- **`crates/bt-atlas/src/font.rs`** — zincir tercih edilen aileyi parametre
  alır: istenen aile (varsa) → `PREFERRED` → `FALLBACK`. `open`'ın aile adı
  karşılaştırması istenen ailenin **bulunamadığını** raporlar (bugün yalnız
  tabanın ikamesi için stderr basıyor). Eşaralık CoreText'in mono trait'iyle
  sorulur; yoksa **uyarı**, ret yok. Sonuç `eprintln!` değil, çağırana dönen
  bir durum.
- **`crates/bt-atlas/src/lib.rs`** — `Atlas`'ın anahtarı (aile, punto,
  ölçek); `new` ve `ensure` aileyi alır. Font durumu atlasla birlikte
  saklanır ve okunabilir. Punto kırpması (`effective_point_size`) **sessiz**
  kalır: tanısı `punto × ölçek`'e bağlı olurdu ve pencere ekran değiştirdikçe
  gelip giderdi. Dolan atlas tofu çizer (`lib.rs:39-42`).
- **`crates/bt-gpu/src/renderer.rs`** — `POINT_SIZE` sabiti kalkar; `Renderer`
  aile ve puntoyu ayar olarak tutar, değiştiren bir yöntem "değişti mi"
  döndürür. `sync_atlas` anahtarı yine **tek yerde** değiştirir. Font
  bildirimi `bt-gpu`'nun **kendi tipinde** yayımlanır (aile bulunamadı,
  eşaralıklı değil) — `bt-atlas` tipi yeniden ihraç edilmez
  (`renderer.rs:96-99`'un gerekçesi, 003 R5).
- **`crates/bt-core/src/settings.rs`** — `[font] family` (metin, boş →
  zincir) ve `size` (sonlu ve > 0; aralık bilinmez). Fark fonksiyonu fontu
  kapsar.
- **`crates/bt-shell/src/app.rs`** —
  - Açılışta ve uygulayıcıda font değişimi: renderer'a ver → değiştiyse
    `refresh_geometry` (atlas yeniden kurulur, grid ve PTY yeniden boyutlanır).
  - **Font yuvası** alt başlığa eklenir; alt başlığın tek sahibi hem
    uygulayıcının hem `sync_geometry`'nin sonunda çağrılır (ekran değişimi
    atlası yeniden kurar).
  - Hermetik dalda varsayılan font.
  - **Karar 6 kapanışı:** `sync_geometry`'nin doc'una tek cümle — ölçek bir
    kez okunur ve iki kapıya aynı yerelden gider, birleştirme gerekmez.
- **`docs/AYARLAR.md`** — `[font] family`, `size`; aile bulunamayınca ne
  olur; çok büyük puntoda atlasın dolup kutu çizmesi.

## Kabul

- Atlas: aynı aile + punto + ölçek yeniden kurmaz; aile değişince kurar.
- Olmayan bir aile adı → zincirdeki font açılır, durum "bulunamadı" der.
- Orantılı bir sistem fontu → açılır, durum "eşaralıklı değil" der.
- Ayar ayrıştırma: `size` negatif/sıfır/NaN → varsayılan + tanı; `family`
  yok → zincir.
- Fark fonksiyonu font değişimini görür.
- `make duman` jetonları değişmez (`hucre=8 glif=6`: hermetik dal varsayılan
  fontu kullanır).
- Göz: `family` ve `size`'ı editörde değiştirmek pencereyi anında günceller;
  içinde vim açıkken yeniden boyutlanma düzgün; olmayan aile alt başlıkta
  görünür, düzeltince kaybolur; pencereyi başka ekrana taşımak font yuvasını
  oynatmaz.

## Yayın Etkisi

- **ayar şeması** — `[font] family` (metin; boş ya da yok → zincir) ve
  `[font] size` (tam sayı ya da ondalıklı, `> 0`, varsayılan `13`). Eski
  anahtar yok. `docs/AYARLAR.md` → `[font]`, iki hata tablosuna font
  satırları.
- **`bt-core` pub API** — `FontOptions`, `Settings.font`, `Changes.font`;
  `Settings` ve `Parsed` artık `Eq` değil (`f64`).
- **`bt-atlas` pub API** — `Atlas::new`/`ensure` aileyi alıyor,
  `FontIssue`, `Atlas::font_issue`.
- **`bt-gpu` pub API** — `FontNotice`, `Renderer::set_font`,
  `Renderer::font_notice`; `POINT_SIZE` kalktı.
- **ölçüm bekliyor:** font değişiminde geçmişin yeniden sarılması (`Term`
  kilidi ana thread'de tutulurken) — dolu 10 000 satırlık geçmişte süresi.
- Borç kapanışı: ölçeğin iki kapısı (003'ten beri) — `sync_geometry` doc'u.
- `CLAUDE.md` bugünkü hâl, `bt-atlas` `lib.rs` başlığı — güncellendi.
- Yeni bağımlılık yok, `Cargo.lock` değişmedi. `make duman` jetonları
  değişmedi: `kare=2 hucre=8 glif=6 kural=15 yuva=13/2048`.

## Checklist

- [x] Font zinciri istenen aileyi alır; durum (bulunamadı / eşaralıksız) döner
- [x] Atlas anahtarında aile
- [x] Renderer: `POINT_SIZE` kalktı, font ayarı, `bt-gpu` tipinde bildirim
- [x] `[font]` ayarları ve fark
- [x] Uygulayıcı ve açılış: font → `refresh_geometry`; font yuvası; hermetik dal
- [x] `sync_geometry` doc'unda Karar 6 kapanışı
- [x] Test: atlas yeniden kurulumu, bulunamayan ve orantılı aile, ayrıştırma
- [x] `docs/AYARLAR.md`
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **`FontOptions` `bt-core`'da bir tip** (plan: `Settings`'te iki anahtar,
  renderer "aile ve puntoyu tutar"). Varsayılan puntonun tek sahibi
  `FontOptions::default()`; renderer onu bütün olarak tutuyor ve açılış
  değeri de o. Hermetik dalda bu yüzden çağrı **yok**: `set_font` hiç
  çağrılmayınca font zaten varsayılan. Bedeli `Settings`/`Parsed`'ın `Eq`'yi
  kaybetmesi.
- **Aile adı harf duyarsız karşılaştırılıyor** (plan: aile adı
  karşılaştırması). Ölçüldü: CoreText `"menlo"`'yu açıyor ve `"Menlo"` diye
  bildiriyor. PostScript adı (`Menlo-Regular`) eşleşmiyor, belgede. Aynı
  yoklamada: olmayan ad için CoreText **Helvetica** veriyor; SF Mono bu
  makinede yok, zincir Menlo'ya düşüyor.
- **`post_notices` yuva aynıysa hiçbir şey yapmıyor** (planda yok). Font
  yuvası her `windowDidResize:`'da yazıldığı için bulunamayan bir aile
  stderr'e olay başına satır basıyordu. Yan etkisi: aynı hatalı ayar
  dosyasını ikinci kez kaydetmek stderr'e tekrar basmıyor (alt başlıkta
  zaten duruyor).
- **Font yuvasının dizgisi `bt-shell`'de** (`notices::font_messages`);
  `FontNotice` metin taşımıyor, öteki yuvaların dili tek yerde.
- **Yüz eksikliği** (Monaco'da Bold/Italic yok) stderr satırı olarak kaldı,
  alt başlığa taşınmadı; belgede.
- **Test-first:** `bt-core`'un dört font sınaması iskelete karşı düştü;
  `bt-atlas`'ta bulunamayan ve orantılı aile sınamaları düştü, aile anahtarı
  sınaması imzayla birlikte gelen `Key` yüzünden baştan yeşildi.
- **Göz kontrolü** geçici `HOME` + `ZDOTDIR` + `screencapture -l` ile.
  Kabuk `login` üzerinden açılıyor ve `HOME`'u gerçek ev dizinine çekiyor;
  `ZDOTDIR`'daki `.zshrc` mutlak yolla `vim` açınca bateri **içinde** vim
  sınanabildi (phase-4'ün bekleyen göz kontrolü de bu yoldan koşabilir).
  Gözlenen: vim açıkken 13 → 20 punto anında, satırlar yeni sütuna
  sarıldı; olmayan aile alt başlıkta, Monaco'ya düzeltince kalktı; Helvetica
  uyarısı ve harflerin hücreye kırpılması; `size = 0` 20'yi tuttu, tanı
  yuvasında; `[font]` silinince 13 punto Menlo, iki yuva boş. Pencere
  boyutlandırma stderr'e satır eklemedi. **Tek ekran:** başka ekrana taşıma
  gözle sınanmadı; `missing_family_becomes_a_notice_after_the_atlas_opens`
  aynı fontun iki ölçekte aynı bildirimi verdiğini sınıyor.
