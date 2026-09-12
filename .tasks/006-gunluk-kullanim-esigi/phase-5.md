# Phase 5 — `IDLE_FRAME_LIMIT` yeniden ölçümü (görünür pencere)

## Özet

Boşta sıfır kare kapısı görünür pencerede yeniden ölçülür ve ayrı commit'le dondurulur.

_Requirements: R5, R5.1_

---

## 1. Neden ayrı phase

Bugünkü `8` görünmez pencerede ölçüldü. Bundle görünür pencere getirince
meşru kare sayısı değişir. Yeni sayı ile yeni kod **aynı sette inerse**
regresyon maskelenir: sınır yükseltilip kod yeşil geçirilebilir (işletme 2 —
tarihçe: 005 phase-3'te `2`→`8` çünkü eski dayanak çürümüştü).

O yüzden bu phase'de **kod değişmez**. Yalnız ölçüm + sayı + gerekçe. Kapı
değişikliği kod fazlarından ayrıktır; bu ayrıklık bu setin panel şartıdır.

## 2. Yöntem

Görünür pencerede `make duman` koşuları: sağlıklı dağılım + bozuk koşu
ayrımı (005 phase-3'ün yöntemi tekrarlanır). Sayı `bt-shell`'de sabitin
doc'una gerekçesi, koşu sayıları ve türetmesiyle yazılır; `CLAUDE.md`'deki
sözleşme satırı ve proje.md'deki kapı paragrafı güncellenir.

---

## Uygulama Notları

## Yayın Etkisi

- **Ölçüm:** `IDLE_FRAME_LIMIT` sayısı değişebilir — bu bir kapı değişikliği,
  gerekçesi doc'unda. Sayı `docs/OLCUMLER.md`'nin tekelinde değil (o kapı
  değil eşik bilgisidir); ama ölçüm koşusu `/measure` ile yapılır ve sonuç
  oraya da düşer.
- Kod değişmediği için `.metal`/terminfo/ayar/tema/shell etkisi yok.

---

## Checklist

- [ ] Görünür pencerede sağlıklı + bozuk koşu dağılımı ölçüldü
- [ ] Sayı ayrı commit + gerekçeyle donduruldu (kod değişikliği yok)
- [ ] Sabit doc'u + `CLAUDE.md` sözleşme satırı + proje.md kapı paragrafı güncel
- [ ] `/measure` ile koşuldu, sonuç `docs/OLCUMLER.md`'ye düştü
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
