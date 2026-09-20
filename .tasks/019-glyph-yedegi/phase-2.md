# Phase 2 — Belgeler ve borcun kapanması

## Özet

Kullanıcıya bakan belgeler yedeği anlatıyor; yol haritasının borcu kapanıyor.

_Requirements: R6_

## Neden ayrı phase

Phase-1 yalnız **kodla çelişen** cümleleri düzeltiyor (`CLAUDE.md`,
`lib.rs` başlığı) — `CLAUDE.md`'nin aynı-commit kuralı bunu zorunlu kılıyor.
Buradakiler **çelişmiyor, eksik kalıyor**: kullanıcı diline çevrilmiş
açıklamalar ve indeks bakımı. Tek başına doğrulanabilir ve `make hepsi`'yi
yeşil bırakıyor.

## Değişiklikler

- **`docs/AYARLAR.md`** — iki kalem:
  1. `[font] family` bölümünün "aile verilmezse SF Mono" ifadesi
     **netleşiyor**: zincir SF Mono'yu tercih ediyor ama o Xcode ile geliyor
     ve **her makinede yok**; yoksa Menlo. Ölçüldü (bu makinede SF Mono
     kurulu değil, `CTFontCreateWithName` Helvetica veriyor ve zincir
     Menlo'ya düşüyor).
  2. Kırpma cümlesinin yedek ikizi: seçili fontta olmayan **tek hücrelik**
     karakter sistemden geliyor; sığmayan (emoji, CJK, geniş ok) kutu kalıyor
     ve **bu bilinçli** — yarım çizilmiş bir glyph sessiz bozulma, kutu
     görünür eksiklik.
- **`crates/bt-core/src/settings.rs`** — şablon yorumunun `AYARLAR.md` ikizi;
  ikisi birlikte değişir.
- **`docs/YOL-HARITASI.md`** — "Font fallback yok" borcu kapanıyor; kalan
  kısım (emoji, geniş glyph) zaten 021'de.
- **`.tasks/README.md`** — indeks notu.

## Kabul

- `AYARLAR.md` ile `settings.rs` şablonu aynı şeyi söylüyor.
- Yol haritasında kapanmamış bir "font fallback" maddesi kalmıyor.
- `make hepsi` yeşil.

## Yayın Etkisi

- **`docs/AYARLAR.md`** ve **`crates/bt-core/src/settings.rs`** — yukarıda.
- **`docs/YOL-HARITASI.md`** — borç kapanıyor.

## Checklist

- [ ] `docs/AYARLAR.md`: SF Mono netleştirmesi + yedek cümlesi
- [ ] `settings.rs` şablon yorumu (ikizi)
- [ ] `docs/YOL-HARITASI.md`: borç kapandı
- [ ] `.tasks/README.md`: indeks notu
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] Yayın etkisi yazıldı
