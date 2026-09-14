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

- [ ] **Devir (phase-4): paketten açılan ölçüm koşusu.** Komut:
      `make kur && env -u BT_SCROLL_TEST -u BT_FRAME_STATS open -W -n --env BT_RUN_SECONDS=3 --stdout "$PWD/target/duman-paket.out" "$PWD/target/release/bateri.app"`
      → jeton satırı dosyada. Dört tuzak: (1) `open`'ın çıkış kodu
      uygulamanınki **değil**, hep 0 — karar jeton satırından okunur; çıkış
      kodu gerekiyorsa `target/release/bateri.app/Contents/MacOS/bateri`
      aynı ortamla doğrudan koşulur ama o LaunchServices yolu değil;
      (2) paket **release** (`profil=release`), bugünkü `8` ve `make duman`
      **debug** — iki profilin dağılımı ayrı tutulur, hangisinin kapıya
      bağlanacağı gerekçeyle yazılır; (3) `open` çağıranın ortamını geçiriyor
      (phase-4'te `open`'la açılan bir probe kabuğun değişkenlerini gördü),
      yani `env -u` hermetikliği burada da şart; (4) `--stdout` yolu mutlak
      olmalı — LaunchServices süreci `cwd=/` ile başlatıyor
- [ ] **Devir (phase-4c): `make duman` `glif=` gürültüsü.** phase-4c'nin ilk üç koşusu `glif=7/9/8` (`kare=3/4/3`, `yuva=14/16/15`) verdi, HEAD tabanı `glif=6`. Hipotez (kanıtsız): koşu sırasında öne çıkan pencereye kullanıcının tuş vuruşu düşüyor. Ölçüm koşuları kullanıcı bilgisayarı kullanmıyorken yapılır; `glif=`/`yuva=` dağılımı da kaydedilir ve 6'dan sapan koşu **ayıklanmadan önce** nedeni yazılır — aynı gürültü `kare=`'yi de oynatıyorsa sınırın türetmesine karışır
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
