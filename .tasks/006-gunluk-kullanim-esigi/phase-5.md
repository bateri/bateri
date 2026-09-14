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

- **Sonuç: `IDLE_FRAME_LIMIT` `8` kaldı, sayı değişmedi.** Koşu tabloları,
  ortam ve türetme `docs/OLCUMLER.md` → `## Boşta kare`'de (bu ölçüm dosyayı
  kurdu); sabitin doc'u iki ölçümün kutuplarını ve türetmeyi taşıyor. Buraya
  yalnız nereye yazıldığı düşüyor — sayının sahibi o dosya.

- **Koşular kullanıcı makineye dokunmazken, art arda koşuldu** (2026-09-15
  02:00–02:09, prizde, açık `bateri` penceresi yok). Sağlıklı: debug
  `make duman` 3 sn, release paket (`open`) 3 sn, ikisinden de 5 sn'lik
  kıyas. Bozuk: `link.rs` `needs_update` sonuna koşulsuz `iv.waker.wake();`,
  iki profilde üçer koşu; mutasyon `git checkout -- crates/bt-gpu/src/link.rs`
  ile geri alındı (`git diff | wc -l` → `0`) ve iki derleme de yenilendi.

- **Pencere gerçekten görünürdü — yoklandı, varsayılmadı (sapma, ekleme).**
  Kılavuz görünürlüğü kanıtlayan bir adım istemiyordu; ama phase'in bütün
  gerekçesi "görünür pencere" ve jeton satırı pencereyi görmüyor. Kısa bir
  CGWindowList yoklaması (t≈1,5 sn, `kCGWindowIsOnscreen`, öndeki uygulama)
  bir debug ve bir paket koşusunda ana pencereyi **ekranda ve önde** gördü.
  Tarifi `docs/OLCUMLER.md` → `## Nasıl yeniden ölçülür`'de; yoklama ikilisi
  scratchpad'de, depoya girmedi.

- **Yan bulgu: bundle'sız süreç de öne çıkıyor.** `make duman`'ın
  `cargo run` penceresi yoklamada önde ve ekrandaydı (`app.rs`'teki
  `activate()` 001'den beri var). İki cümleyle uyuşmuyordu: `proje.md`'nin
  kapı paragrafındaki "bundle'sız süreç öne çıkma hakkı taşımaz" düzeltildi;
  `docs/YOL-HARITASI.md`'nin 006 satırındaki gerekçe ("bundle'sız süreç öne
  çıkamıyor … görünmeyen bir pencerede ölçüldü") setin planlama anındaki
  gerekçesi olduğu için yeniden yazılmadı, yanına tarihli bir not düştü.
  005'in penceresinin o gün görünmez olup olmadığı geriye dönük bilinemiyor.

- **İki profil ayrı tutuldu; kapıya debug bağlı.** Gerekçe sabitin doc'unda,
  dağılımlar `docs/OLCUMLER.md`'de.

- **Devir (phase-4c) `glif=` gürültüsü: tekrarlanmadı.** Sağlıklı koşuların
  hiçbiri reçetenin beklenen `glif=`/`yuva=` değerinden sapmadı; ayıklanan
  yok (dağılım `docs/OLCUMLER.md`'de). Tuş vuruşu
  hipoteziyle uyumlu, onu kanıtlamıyor.

- **Devir (phase-4) paket komutuna `--stderr` tuzağı eklendi.** Kılavuzun
  komutunda `--stderr` yok. Kapının düştüğü koşu jeton satırı basmıyor, tanı
  satırı stderr'e gidiyor: bozuk paket koşusu `--stdout` dosyasını **boş**
  bıraktı, sonuç yalnız `--stderr` dosyasındaydı. Komut ve tuzak
  `docs/OLCUMLER.md`'de.

- **005 yönteminin 5 sn'lik kolu da koşuldu** (kılavuz "005'in yöntemi
  tekrarlanır" diyor, 005'in en yüksek sağlıklı gözlemi 5 sn'lik bir
  koşudan geliyordu).
  On saniyelik kol koşulmadı: 005'te iki koşuydu ve süreyle artmadığını zaten
  gösterdi; pencere süresini kısa tutmak için bırakıldı.

- **`docs/OLCUMLER.md` yalnız boşta kare ile kuruldu.** `/measure` skill'i
  dosyayı kuran ölçümün `## Yöntem`'i `Measured`'ın doc'undaki "dürüst
  sınırlar" listesinden taşımasını söylüyor. O liste **kare süresi ve açılış**
  kancasının sınırları ve bu phase o türü ölçmedi; taşımak ölçülmemiş bir
  türün yöntemini sayısız bir bölüme koymak olurdu. Liste yerinde kaldı,
  dosya ona bağlanıyor ve "henüz yok" diyen üç cümle (`CLAUDE.md`,
  `Measured`'ın doc başlığı, `/measure` skill'i) zamandan bağımsız bir
  kurala çevrildi: hangi türün sayısı olduğunu dosyanın başı söylüyor.

- **005'in koşu tablosu `docs/OLCUMLER.md`'ye taşındı.** `8`'i bağlayan
  iki kutup o ölçümden geliyor; tablo yalnız bir görev dosyasında
  kalsaydı sınırın dayanağı sahibin dışında olurdu. Sabitin doc'u yalnız
  kutupları ve türetmeyi taşıyor; bu istisna artık kuralın sahibinde yazılı
  (`proje.md` → tuzaklar, `/audit` mercek 6).

- **`/measure` `Skill` aracıyla çağrılmadı; akışı birebir izlendi.** Ölçüm bu
  implementer'ın içinde ve kullanıcının verdiği sınırlı pencerede koştu;
  skill'in adımları (önce `## Yöntem` ve `## Nasıl yeniden ölçülür`, ortam
  kaydı, yolun ateşlendiğini gösterme, dağılım, tek sahip, iş setini kapatma)
  `SKILL.md`'den okunarak uygulandı. Skill'in tür tablosunda boşta kare satırı
  yoktu; eklendi, çünkü dosyada artık o bölüm var.

- **Hücre sayısı (grid) kayda girmedi — ölçülmedi.** Jeton satırında yok;
  içerik boyutu kaynaktan ve yoklamadan yazıldı (`docs/OLCUMLER.md` → ortam).

---

### `/simplify` — dört mercek, dördü döndü (diff belge ağırlıklı)

Kod değişikliği yok; mercekler belgeler üzerinde koştu ve asıl baktıkları şey
**sahiplik ve tekrar** oldu. Uygulananlar:

1. **Sahip yeni kuruldu ama çevresi eski düzende kalmıştı** (altitude, reuse,
   simplification birlikte): "kapı debug'a bağlı" gerekçesi dört kopyadaydı →
   sahibi sabitin doc'u, diğerleri tek cümle + işaret. `proje.md`'nin kapı
   hücresi her yeniden ölçümde büyüyen bir tarihçeye dönüyordu → sözleşme +
   işaret. `CLAUDE.md`'nin `make duman` satırından phase anmaları çıktı.
2. **"Dosya yalnız boşta kare ile kuruldu" dört yerdeydi** — ilk kare süresi
   ölçümü dördünü birden bayatlatırdı, yani "henüz yok" desenini yeni bir
   değerle yeniden kuruyordu. Kural zamandan bağımsız yazıldı; hangi türün
   sayısı olduğunu yalnız dosyanın başı söylüyor.
3. **Sahiplik istisnası sahip dosyada değil sahipli dosyadaydı** ve "iki
   istisna" deyip birini sayıyordu. İstisna `proje.md` → tuzaklar ile
   `/audit` mercek 6'ya yazıldı; `docs/OLCUMLER.md` oraya işaret ediyor.
   Olmasaydı `/audit` sabitin doc'undaki kutupları kural ihlali sayardı.
4. **005'in tablosu sahibe taşındı** (yukarıdaki not).
5. **Tarif:** paket tuzaklarından "profil ayrımı" `## Yöntem`'le tekrardı →
   çıktı, dört tuzak; yoklama ayrı koşu açmıyor, döngünün ilk turunda koşuyor;
   debug koşuları da dosyaya düşüyor (`tee`); sıra bozuk → geri al → sağlıklı
   (geri almadan sonraki derleme sağlıklı kolun derlemesi olur).
6. **Yol haritasının 006 satırına tarihli not**, `/measure`'ın açıklamasına
   ve `.claude/README.md`'ye boşta kare türü.

Uygulanmayanlar:

- Checklist'in phase-4 devir maddesi hâlâ "dört tuzak" diyor ve `--stderr`'i
  saymıyor — o metin kılavuzun kendisi, kutu işaretlendi ama metni yeniden
  yazılmadı; güncel liste `docs/OLCUMLER.md`'de.
- "Dosya yalnız boşta kare ile kuruldu" notu Uygulama Notları'nda kaldı:
  skill talimatından sapmanın gerekçesi, Yayın Etkisi değil.
- **Takip önerisi (kapsam dışı, kod/Makefile):** paket duman koşusu için bir
  `Makefile` hedefi dört tuzağın üçünü tasarımla kapatırdı (`open`'ın çıkış
  kodu, `env -u`, mutlak yol; `--stderr`); Swift yoklaması markdown'da
  derlenmeden duruyor. `report_and_exit`'in doc'u kapı düşünce jeton
  satırının basılmadığını ve tanının stderr'e gittiğini söylemiyor. Hedef
  phase yok → orkestratörün kararı.
- `.tasks/README.md`'nin 006 satırı bayat ("`make kur` henüz yok") — setin
  kapanışının (adım 10) işi.

### `/code-review` — Skill fork'u koştu, dört bulgu

Doğrulananlar: iki tablonun her satırı `scratchpad/olcum/` altındaki koşu
dosyalarıyla birebir; 005 tablosu kaynağıyla aynı; kapının `n > limit` ve
yalnız `Smoke` yükünde, profilden bağımsız ateşlediği, düşen koşuda jeton
satırı basmayıp tanıyı stderr'e yazdığı koddan okundu; yoklamanın sahip adı
filtresi paketin `CFBundleName`'iyle tutuyor.

Giderilenler:

1. **Tarif bozuk kolun kanıtını eziyordu** (orta): iki kol aynı dosya adlarına
   yazıyordu ve bozuk paket koşusunun tek kaydı `.err` dosyası; "en az üç kez"
   de sabit on turluk döngülerle çelişiyordu. Döngüler `KOL`/`N` alıyor, dosya
   adı kolu taşıyor.
2. **005'in bozuk dağılımının tamamı "kısılmış" okunuyordu**: 3 sn'lik üst uç
   bu koşuyla aynı tam hız; kısılmış olan 5 sn'likler. Gözlem ayrıştırıldı.
3. **Sabitin doc'u yoklanan iki koşunun görünürlüğünü bütün ölçümün koşulu
   gibi yazıyordu** → "yoklanan iki koşuda".
4. **`/measure` kutusu notla çelişiyordu** → `[~]` + gerekçe.

Uygulanmayan: "Sayı ayrı commit" kutusu commit'ten önce işaretli bulundu.
Kutu, kendisini karşılayan commit'le birlikte iniyor; ayrı bir damga
commit'i yalnız hash'i yazıyor.

### `/audit` — on merceğin yedisi ilgisiz, üçü inline

**İlgisiz (sebebiyle):** 1 (katman: `Cargo.toml` ve kaynak `use`'u
değişmedi), 3 (`bt-core` el değmedi), 4 (ayar/tema yok), 5 (`assets/shell/`
yok), 7 (yorum dışı satır yok — `git diff -U0` + `///` süzgeci boş), 9
(`Cell`/`.metal` yok). **2 temiz:** `Cargo.toml`/`Cargo.lock` diff'te yok.
Yargı mercekleri iki tane çıktı (8, 10), yani ajan kurulmadı.

Beş bulgu, beşi de giderildi:

1. **Mercek 6 — ölçülmemiş iddia:** "release'in daha kısa açılışı" hem
   `docs/OLCUMLER.md`'de hem sabitin doc'unda hipotezin dayanağı gibi
   duruyordu; açılış bu koşuda ölçülmedi (`ornek=off`). Cümle "profil farkı
   `acilis=` ile sınanabilir, sınanmadı" oldu.
2. **Mercek 6 — ölçülmemiş iddia:** `## Yöntem`'deki "tuş o pencereye düşer
   ve sayaçları oynatır" phase-4c'nin kanıtsız hipotezini kural gibi
   yazıyordu → hipotez olarak.
3. **Mercek 6 — `.tasks/*` sayı tekrarı:** bu dosyanın notları ölçüm
   sonuçlarını (sayaç değerleri, koşu sayısı, kutuplar, pencere boyutu)
   tekrarlıyordu → nereye yazıldığı.
4. **Mercek 8 — sınır kuralının üst korkuluğu yoktu:** "sağlıklının iki katı,
   bozuğun altında" kuralı bozuk dağılımın altındaki **her** büyütmeye izin
   veriyordu, yani kapıyı gevşeten bir yeniden ölçüm kurala uygun
   görünürdü. Kural artık sınırın yalnız iki koşulun zorladığı kadar
   oynadığını ve büyütmenin algılama tabanını yükselttiğini söylüyor.
5. **Mercek 10 — dil:** tarifin kabuk değişkenleri Türkçeydi (`KOL`,
   `bozuk`/`saglam`) → `ARM`, `broken`/`healthy`.

Temiz çıkan: mercek 6'nın sahiplik yarısı — `CLAUDE.md` ve `proje.md` yalnız
sözleşmeyi (`8`) taşıyor; sabitin doc'u kutuplar, türetme ve rejim/taban
gerekçesi taşıyor (`kare=594`, `21`/`597`, ~12 kare türetmenin parçası,
koşu tablosu değil). Yeni yazılan istisna iki kural sahibinde de aynı
cümleyle ve `docs/OLCUMLER.md` onlara işaret ediyor. Mercek 8'in geri kalanı:
`8` kutuplar arasında, algılama tabanı paragrafı yerinde, kapının `n > limit`
koşulu koddan okundu (`/code-review`).

## Yayın Etkisi

- **Ölçüm:** `IDLE_FRAME_LIMIT` görünür pencerede yeniden ölçüldü ve `8`
  kaldı — kapı değişikliği **yok**. İlk ölçüm kaydı `docs/OLCUMLER.md`'yi
  kurdu (`## Yöntem`, `## Nasıl yeniden ölçülür`, `## Boşta kare`); kare
  süresi, açılış, bellek, giriş gecikmesi ve bench bölümleri sayısız.
- **Belgeler:** `IDLE_FRAME_LIMIT` ve `Measured` doc'ları, `CLAUDE.md`
  (`make duman` satırı + ölçüm maddesi), `proje.md` kapı paragrafı,
  `/measure` skill'i, `docs/YOL-HARITASI.md` borç maddesi.
- Kod değişmedi (yalnız doc yorumları): `.metal`/terminfo/ayar/tema/shell/
  bundle etkisi yok, `Cargo.lock` değişmedi.

---

## Checklist

- [x] **Devir (phase-4): paketten açılan ölçüm koşusu.** Komut:
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
- [x] **Devir (phase-4c): `make duman` `glif=` gürültüsü.** phase-4c'nin ilk üç koşusu `glif=7/9/8` (`kare=3/4/3`, `yuva=14/16/15`) verdi, HEAD tabanı `glif=6`. Hipotez (kanıtsız): koşu sırasında öne çıkan pencereye kullanıcının tuş vuruşu düşüyor. Ölçüm koşuları kullanıcı bilgisayarı kullanmıyorken yapılır; `glif=`/`yuva=` dağılımı da kaydedilir ve 6'dan sapan koşu **ayıklanmadan önce** nedeni yazılır — aynı gürültü `kare=`'yi de oynatıyorsa sınırın türetmesine karışır
- [x] Görünür pencerede sağlıklı + bozuk koşu dağılımı ölçüldü
- [x] Sayı ayrı commit + gerekçeyle donduruldu (kod değişikliği yok)
- [x] Sabit doc'u + `CLAUDE.md` sözleşme satırı + proje.md kapı paragrafı güncel
- [~] `/measure` ile koşuldu, sonuç `docs/OLCUMLER.md`'ye düştü — sonuç düştü, ama skill `Skill` aracıyla **çağrılmadı**; akışı `SKILL.md`'den okunarak elle izlendi (gerekçe Uygulama Notları'nda)
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**) — kapıdan sonra: `make hepsi` 0, `make duman` 0; `make kur` ölçüm sırasında dört kez yeşil (kaynağı değişmedi, kapı gerektirmiyor)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
