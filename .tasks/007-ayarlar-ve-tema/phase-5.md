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

- **ayar şeması** — `[font] family`, `[font] size`.
- **ölçüm bekliyor:** font değişiminde geçmişin yeniden sarılması (`Term`
  kilidi ana thread'de tutulurken).
- Borç kapanışı: ölçeğin iki kapısı (003'ten beri) — `sync_geometry` doc'u.

## Checklist

- [ ] Font zinciri istenen aileyi alır; durum (bulunamadı / eşaralıksız) döner
- [ ] Atlas anahtarında aile
- [ ] Renderer: `POINT_SIZE` kalktı, font ayarı, `bt-gpu` tipinde bildirim
- [ ] `[font]` ayarları ve fark
- [ ] Uygulayıcı ve açılış: font → `refresh_geometry`; font yuvası; hermetik dal
- [ ] `sync_geometry` doc'unda Karar 6 kapanışı
- [ ] Test: atlas yeniden kurulumu, bulunamayan ve orantılı aile, ayrıştırma
- [ ] `docs/AYARLAR.md`
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı
