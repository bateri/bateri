# Phase 6 — Ölçüm ve süreli kapı

## Özet

Yeni duman reçetesinin sağlıklı ve bozuk dağılımları ölçülür;
`IDLE_FRAME_LIMIT` bu ölçümle yeniden türetilir ve `sessiz=` bir alt sınırla
kapıya bağlanır. Kod phase'lerinden **ayrı** commit.

_Requirements: R7_

## Değişiklikler

- **Ölçüm koşusu** — tarif `docs/OLCUMLER.md` → `## Nasıl yeniden ölçülür`:
  iki kol (sağlıklı / kasıtlı bozuk), iki profil (debug ve release paket),
  profil başına en az on sağlıklı ve üç bozuk koşu, pencere görünürlüğü
  yoklaması. **Bozuk kol yeniden doğrulanır:** mutasyonun yeri
  (`needs_update`'in sonuna koşulsuz `wake()`) bu sette değişen bir gövdede;
  hâlâ her çizilen karede koştuğu gösterilir, değilse tarif güncellenir.
  Ölçüm gerçek pencere ve sessiz makine ister; başsız ortamda koşmaz.
- **`crates/bt-shell/src/app.rs`** — `IDLE_FRAME_LIMIT`'in değeri ölçümün
  zorladığı kadar oynar (kural: en yüksek sağlıklı gözlemin en az iki katı,
  en düşük bozuk gözlemin altında; ikisi çelişirse sınır oynatılmaz ve iş
  durur). Yanına `sessiz`in alt sınırı gelir ve kapıya bağlanır: duman
  yükünde `sessiz ≥ T` **ve** `sessiz=none` kırmızı. Kural bu jetonda
  **terstir** — sağlıklı koşuda `sessiz` büyük, bozuk koşuda küçük: `T`, en
  düşük sağlıklı gözlemin en çok yarısı ve en yüksek bozuk gözlemin üstünde.
  `IDLE_FRAME_LIMIT`'in "sınır zorladığı kadar oynar, fazlası değil" ilkesi bu
  jetonda ters işler: duyarlılık `T` ile **büyür** (yakalanan en yavaş
  sızıntının periyodu ≈ `T`), yani `T` kuralın izin verdiği **en büyük** değer
  olarak seçilir — ortadan seçilen bir sayı kapıyı yavaş sızıntıya körleştirir.
  Sabitin doc'u türetmeyi, ortamı ve **bilinen yanlış pozitifi** yazar:
  koşunun son `T`'sinde pencereyi sürüklemek, örtüp açmak ya da ekranı
  uyandırmak meşru kare üretir ve kapıyı düşürür; kalıcı çare geometri
  kaynaklı kareleri sayaç dışında tutmak (kayıtlı borç).
- **`docs/OLCUMLER.md`** — yeni giriş: ortam, iki kolun tabloları, tam jeton
  satırları, `IDLE_FRAME_LIMIT`'in ve `T`'nin türetmesi. `## Yöntem`'e
  `sessiz`in kuralı; sabit jeton bloğu yeni satır biçimiyle yenilenir.
  `BT_RUN_SECONDS`, reçetedeki uyku ve `T` **aynı blokta** gerekçelenir —
  üçü üç dosyaya dağılırsa biri oynadığında kapı sessizce kırılganlaşır.
- **`Makefile` + `.claude/is-akisi/proje.md` + `CLAUDE.md`** — kapı artık üç
  şey soruyor (sayaçlar, `icerik ≤ IDLE_FRAME_LIMIT`, `sessiz ≥ T`) ve
  animasyonun yerleşmesini; üç dosyadaki duman cümlesi bunu söyler.
- **`docs/YOL-HARITASI.md`** — borcun kapandığı ve kapanmayan yarısının ne
  olduğu (hareket saatini atlayan kod yalnız yapısal kuralla ve `/audit` ile
  tutuluyor) kayda geçer.

## Kabul

- İki kolun tabloları `docs/OLCUMLER.md`'de; sağlıklı ve bozuk dağılımlar
  **ayrık** (kesişirlerse kusur sayıda değil koddadır ve iş durur).
- Bozuk kol her iki kapıyı da ateşliyor: durma koşulu bozulunca
  `MotionUnsettled`, kare sızıntısında `sessiz` alt sınırın altında.
- Sağlıklı on koşunun onu da yeşil; `make duman` ardışık koşularda kararlı.
- Mutasyon commit'e girmiyor: `git diff` boş ve iki derleme de geri alındıktan
  sonra yenilendi.

## Yayın Etkisi

shader yok · terminfo yok · ayar şeması yok · tema yok · shell entegrasyonu
yok · app bundle yok · yeni bağımlılık yok.

Ölçülmüş iki sabit değişiyor ya da doğuyor; ikisi de bu commit'te **tek
başına** iniyor (kod değişikliğiyle aynı commit'e girerse regresyonu
maskeler). `docs/OLCUMLER.md` bu türün sahibi; `Measured`'ın doc'unda emanet
duran "dürüst sınırlar" listesinden `acilis=` ile ilgili olan yarı, ölçüm
sırasında o dosyaya taşınır.

## Checklist

- [ ] Bozuk kol mutasyonunun yeri yeniden doğrulandı (tarif hâlâ geçerli mi)
- [ ] İki kol, iki profil koşuldu; tablolar çıkarıldı
- [ ] `IDLE_FRAME_LIMIT` ve `T` türetildi; kural sağlandı (aksi hâlde iş durur)
- [ ] `app.rs`: `sessiz ≥ T` kapısı + `sessiz=none` kolu + sabitlerin doc'ları
- [ ] `docs/OLCUMLER.md` (giriş + yöntem + sabit jetonlar)
- [ ] `Makefile` + `proje.md` + `CLAUDE.md` + `docs/YOL-HARITASI.md`
- [ ] Mutasyon geri alındı (`git diff` boş), derlemeler yenilendi
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı
