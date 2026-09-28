# Phase 6 — Duman sabitlerinin yeniden gözlemi

## Özet

Kare yolu değişti:

- sessizliğin saati artık `Pacer`'ın damgası;
- kare sayımı gönderim indeksi + `poll`.

Bu yüzden `IDLE_FRAME_LIMIT` ve `QUIET_FLOOR`, kendi doc'larının istediği gibi
yeniden gözleniyor. Değer ya da doc değişikliği kod phase'lerinden **ayrı**
commit'le iniyor (`proje.md` → Doğrulama).

_Requirements: R5_

## Değişiklikler

- **Gözlem** (kod değil). `docs/OLCUMLER.md` → `## Yöntem` → Boşta kare'nin
  reçetesi uygulanıyor:
  - sağlıklı dağılım, debug ve release;
  - aynı bölümün bozuk (sızıntılı) reçetesiyle bozuk dağılım.
  - Sonuç `docs/OLCUMLER.md` → `## Boşta kare`'ye tarihli bir blok olarak
    giriyor.
- **`crates/bt-shell/src/app.rs`** — yalnız gözlem gerektiriyorsa:
  - `IDLE_FRAME_LIMIT` / `QUIET_FLOOR`'un değeri ya da türetmesini taşıyan
    doc'u;
  - sınamadaki sağlıklı örnek (`HEALTHY_QUIET`).
  - Kural (`CLAUDE.md` → Komutlar): sessiz'in kuralı ters, iki sınır da
    ölçülmüş ve türetmesi doc'ta.
- Değer değişmiyorsa: doc'a "040 geçişinden sonra yeniden gözlendi, aynı"
  tek satırı ve `docs/OLCUMLER.md` bloğu. Commit yine ayrı.

## Kabul

- İki dağılım `docs/OLCUMLER.md`'de. Sağlıklı dağılım iki sınırın doğru
  tarafında, bozuk dağılım kırmızı tarafta. Öyle değilse sabit yeniden
  türetiliyor, tahmin yazılmıyor.
- `make hepsi` ve `make duman` yeşil.

## Checklist

- [ ] Sağlıklı dağılım (debug + release)
- [ ] Bozuk dağılım
- [ ] `docs/OLCUMLER.md` bloğu; gerekiyorsa sabit/doc ve `HEALTHY_QUIET`
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
