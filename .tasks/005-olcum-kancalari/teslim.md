# Ölçüm kancaları — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) ·
> [phase-2b.md](phase-2b.md) · [phase-3.md](phase-3.md) · [phase-3b.md](phase-3b.md)

`/measure`'ın bugüne kadar "ölçüm aracı yok" dediği yerde artık sayı üretiliyor:
`BT_FRAME_STATS` + `BT_SCROLL_TEST` kapısı, kare yolunun iki CPU aralığı ve GPU
deltası, açılış damgası, `report_and_exit`'in genişleyen jeton satırı
(`yuva=`, `yuk=`, `istek=`, `kapanis=`, `ornek=`, `dusen=`, `gpu_ornek=`,
`gpu_elenen=`, `taban=`, `cpu_*`, `gpu_*`, `acilis=`), kapanışın artık
`SHUTDOWN_GRACE` ile sınırlı beklemesi ve `IDLE_FRAME_LIMIT`'in ölçümle
`2`'den `8`'e taşınması. Kapı kapalıyken (varsayılan koşu) tek bir
`Instant::now()` bile çağrılmıyor. Sekiz belge kodla uyumlandı.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make test-yaris
make duman
```

`make test-yaris` **zorunlu**: phase-2b `bt-core`'un kapanış sırasını ve
okuyucu thread'in ömrünü değiştirdi — tam bu sınamanın konusu. `make duman`
**iki yükte** koşmalı (phase-2b'nin kendi kapısı bunu istiyor); `Makefile`'ın
`duman` hedefi phase-2'de hermetik yapıldı (`env -u BT_SCROLL_TEST -u
BT_FRAME_STATS`), yani `BT_SCROLL_TEST=1 make duman` **sessizce Smoke'a
düşer** — Load yükünü görmek için binary'yi doğrudan çağırmak gerekiyor:

```sh
BT_FRAME_STATS=1 BT_SCROLL_TEST=1 BT_RUN_SECONDS=5 cargo run -q -p bateri
```

`make shader` ve `make terminfo` **gerekmedi**: `.metal`, `build.rs` ve
`assets/terminfo` beş phase boyunca hiç el değmedi.

### Beklenen çıktı

- `make duman` (Smoke) → `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke
  istek=3 kapanis=clean profil=debug ornek=off pipeline=ok`, çıkış 0. İlk dört
  jeton (`kare`/`hucre`/`glif`/`kural`) bit bit korunuyor; `kare` koşudan
  koşuya `1` ya da `2` çıkabilir (bilinen oynama, `IDLE_FRAME_LIMIT=8` bunun
  cömertçe üstünde durur — sabitin doc'unda gerekçeli).
- Load yükü (yukarıdaki doğrudan komut, `BT_RUN_SECONDS=5`) → gözlemlenen
  satır: `kare=590 hucre=0 glif=1836 kural=0 yuva=37/2048 yuk=load
  istek=61228 kapanis=clean profil=debug ornek=590 dusen=0 gpu_ornek=590
  gpu_elenen=0 taban=20 cpu_kare_p95=0.79ms cpu_kare_max=1.35ms
  cpu_encode_p95=0.84ms cpu_encode_max=2.43ms gpu_p95=0.24ms gpu_max=0.78ms
  acilis=298.16ms pipeline=ok`, çıkış 0. **Bu bir taban değil, bir gözlem**:
  debug profili, `.app` paketi yok, tek koşu. `docs/OLCUMLER.md`'ye
  taşınmıyor (R7.3) — sette bilinçli olarak yazılmadı; `## Yöntem`'in bugünkü
  ev sahibi `crates/bt-shell/src/app.rs`'teki `Measured`'ın doc'u
  ("Ölçümün dürüst sınırları"), ilk `/measure` oradan devralacak.
- **Dürüst uyarılar, yeşil olsa da kalıyor:** (1) `acilis=298.16ms` **süreç
  başlangıcı değil** — damga `main()`'in ilk satırında alınıyor, dyld ve Rust
  runtime kurulumu ondan önce bitmiş oluyor; sayıyı "process başından" diye
  okumak sapma. (2) ölçüm yükü koşularının ölçülen ~%24'ünde
  `kapanis=abandoned` çıkabilir (çocuk çıkışın içinde arkada kalıyor,
  `Teardown` bunu görünür kılıyor ama kapıya bağlanmadı — bilinen sınır,
  `CLAUDE.md`'de). (3) `kare` ile `istek` iki ayrı rejim gösteriyor ve
  aradaki mekanizma ölçülmedi. Üçü de `Measured`'ın doc'unda yazılı.
- `Cargo.lock` beş phase boyunca **hiç oynamadı** — yeni bağımlılık yok
  (`std::time`, mevcut `libc` yeterdi).

### Doğrulama Checklist

- [x] `make hepsi` yeşil (rustc sürümü + `fmt --check` + `clippy -D warnings` + test)
- [x] `make test-yaris` yeşil (iki zamanlama profili)
- [x] `make duman` yeşil — Smoke yükü (jeton satırı yukarıdaki gibi)
- [x] Load yükü yeşil — doğrudan binary çağrısı (`make duman` hermetik, Load'u koşturmuyor)
- [x] `Cargo.lock` oynamadı
- [~] `make shader` / `make terminfo` — girdileri (`.metal`, `assets/terminfo`) bu sette el değmedi, koşulu doğmadı

## B. Yayın (doğrulamadan SONRA)

Beş phase'in `## Yayın Etkisi` bloklarından derlendi. "Yok" diyen kalemler
atlandı: shader, terminfo/`TERM`, ayar şeması, tema/materyal, shell
entegrasyonu, app bundle — beşi de her phase'de "yok".

**Çözülen çelişkiler (Uygulama Notları kazandı):**

1. **Phase-1'in Load satırı ve "doyma" teorisi bayat.** Phase-1'in `##
   Yayın Etkisi`'si ölçüm yükünün satırını `kare=3 ... yuk=load` diye
   veriyor ve `proje.md`'ye "örtülü pencerede kare sayısının doyması"
   yazılacağını söylüyor. Phase-1'in kendi Uygulama Notları'ndaki
   orkestratör düzeltmesi bunu geri çekiyor: doyma bir tavan değil, kapanış
   kilitlenmesinin (phase-2b'de çözülen) belirtisiydi; düzeltme sonrası aynı
   yük `kare=594` veriyor. Bu teslimde eski "tavan/doyma" anlatısı yok.
2. **Phase-3'ün örnek duman satırı Türkçe jeton değeri taşıyor, kod
   İngilizce basıyor.** Phase-3'ün `## Yayın Etkisi`'si `kapanis=temiz
   ornek=kapali` yazmış — ama bu satır, aynı phase'in `/code-review` bulgusu
   üzerine jeton *değerlerinin* İngilizceye çevrilmesinden (`clean`, `off`)
   **önce** yazılmış (phase-3b bunu adıyla "bayat" diye işaretledi). Yukarıdaki
   Beklenen Çıktı `kapanis=clean` / `ornek=off` kullanıyor — gerçek koşuyla
   doğrulanan hâl bu.
3. **Phase-3'ün "kapanan iddialar" cümlesi abartılı.** Aynı phase'in `##
   Yayın Etkisi`'si "kare süresi iddialarının tamamı, açılış/ölçek iddiaları
   ve atlas doluluğu" kapandı diyor. Doğrusu (phase-3b'nin `/audit`'i ve
   `.tasks/README.md`'nin bugünkü satırı, iki kaynak da aynı düzeltmeyi
   yapıyor): yalnız **atlas doluluğu** (003 #3, 004 #2) kapandı; kare
   süresi/açılış ailesinin sekiz iddiası **ölçülebilir** oldu, kapanmadı —
   sayının gideceği `docs/OLCUMLER.md` henüz yok. *(2026-09-15: 006 phase-5
   kurdu; yalnız `## Boşta kare`'yi taşıyor, bu sekiz iddianın sayısı hâlâ yok.)*

### B.1 `/ship` — dallanmamış `main` push'u `[oto]`

26 commit `origin/main`'in gerisinde bekliyor (`c32f13d..86143ea`): altısı
005'in planlama/RFC commit'leri, yirmisi beş phase'in kodu + durum
defteri/sadakat commit'leri + `86143ea` (aşağıya bak). Hiçbiri push
**edilmedi**. `/ship` bu aralığı `make hepsi`'yi yeniden koşturarak gönderir;
bu belge yazılırken tamamı yeşildi (Bölüm A).

Faz commit'leri: `9788d95` (phase-1) → `8df1ef6` (phase-2) → `e991d78`
(phase-2b) → `ee4b31c` (phase-3) → `8be6f68` + `cb2df10` (phase-3b).

**Faz dışı ama aynı push'ta giden bir commit var:** `86143ea`,
002/003/004'ün `teslim.md`'lerindeki "ölçüm aracı yok" cümlelerini bu setin
getirdiği kancalara göre güncelliyor (tarihli kayıtlara dokunmadan). Bu
005'in kendi teslimi değil ama 005'e **bağımlı**: 005 geri alınırsa bu
commit'in iddiası ("kanca seti geldi, komut artık koşulabilir") yalan olur —
bkz. Geri Alma.

### B.2 002/003/004'ün artık ölçülebilir sekiz iddiası `[komut]`

005'in kendi Yayın Etkisi'nde ölçüm bekleyen **hiçbir** iddia yok (beş
phase'in beşi de "Ölçüm bekliyor: yok" diyor — bu set araç üretti, iddia
doğurmadı). Ama `86143ea` üç komşu setin şu satırlarını "artık koşulabilir"e
çevirdi ve komutun kendisi henüz **koşulmadı**:

```sh
/measure 002-vt-motoru
/measure 003-glyph-atlas
/measure 004-yazi-bicimleri
```

Release profili ve önde/görünür bir pencere ister (`profil=` jetonu
debug'ı; `ornek=` örneklemenin örtülü pencerede sessizce durabildiğini
gösterir). İlk koşu aynı zamanda `docs/OLCUMLER.md`'yi kuracak (R7.3) *(2026-09-15:
dosyayı 006 phase-5'in `IDLE_FRAME_LIMIT` ölçümü kurdu; bu ölçüm onun
`## Yöntem`'ine kare süresi kolunu ekler)* —
bu teslimi bloklamıyor, `.app` paketi (`make kur`) gelene kadar da
`IDLE_FRAME_LIMIT`'in kendisi yeniden ölçülmeyecek.

### B.3 Bilinen borç — kayıt `[oto]`

Zaten commit'lerde yazılı, burada yalnız iz: (1) kapanışın "arkada kalan
çocuk" borcu (`CLAUDE.md`, `Session::shutdown`'ın doc'u — sınırlı bekleme
var ama master'ı `wait` bloklarken boşaltan kalıcı çözüm yok); (2) `kare`↔
`istek` iki rejiminin mekanizması ölçülmedi (`Measured`'ın doc'u); (3)
`IDLE_FRAME_LIMIT=8` görünmeyen bir pencerede ölçüldü, `make kur` gelince
yeniden ölçülecek (sabitin doc'u); (4) bench seti (`criterion`) hâlâ yok —
003 #1/#2 tamamen, 002 #2 ile 004 #4'ün yarısı buna bağlı (`CLAUDE.md`'nin
yeniden yazılan borç cümlesi); (5) giriş gecikmesi zinciri ve düşen kare
sayımı — kapsam dışı, sonraki bir sete kalıyor (`plan.md` → Kapsam Dışı).

### Yayın Checklist

<!-- `/ship` bekleyen manuel adımları BU başlık altında arar. -->

- [x] B.1 `/ship` push `[oto]` — **koşuldu 2026-09-12: `c32f13d..3f9ac91`, 27 commit.**
      (Yazıldığında 26'ydı; teslim defterinin kendi damgası son commit'i de kapsıyor.)
      Doğrulama push'tan önce yeniden koştu: `make hepsi` 0 (100 sınama),
      `make test-yaris` 0, `make duman` Smoke 0 / Load 0. `make shader` ve
      `make terminfo` gerekmedi — `.metal`, `build.rs`, `assets/terminfo` bu
      aralıkta el değmedi. `Cargo.lock` oynamadı.
- [~] B.2 `/measure` üçlüsü `[komut]` — **2026-09-21'de koştu ve yarısı kapandı**: kare süresinin CPU sütunları ile açılış taban oldu. Kalan yarı iki ayrı sebeple kapanmıyor — bench iddiaları `criterion` istiyor (**emekli**), GPU iddiaları ise tabanı alınamayan sütunu istiyor. bekleyen iddiaların tek listesi `docs/OLCUMLER.md` → `## Bekleyen iddialar`; kutuyu `[ ]` tutmak seti süresiz 🔨'da bırakıyordu., henüz koşulmadı; 005'i bloklamıyor
- [x] B.3 Bilinen borç kaydı `[oto]` — beşi de `CLAUDE.md` / ilgili modülün doc'unda yazılı

## Geri Alma

Sıra **tersten**, her adım kendinden sonrakine bağımlı olduğu için üstündekiyle
birlikte gider:

- **`86143ea` tek başına geri alınabilir** (yalnız 002/003/004'ün
  `teslim.md`'lerini eski "ölçüm aracı yok" hâline döndürür) **ama** 005'in
  geri kalanı ayakta kalırsa yalan bir kayıt üretir — kancalar fiilen var
  olurken üç set hâlâ "yok" der. 005 tamamen geri alınıyorsa bu commit de
  aynı işlemin parçası olmalı.
- **`8be6f68` + `cb2df10` (phase-3b) geri alınırsa**, yalnız yorum/belge
  değişikliği gider ama phase-3'ün kodu (`IDLE_FRAME_LIMIT=8`, İngilizce
  jeton değerleri) yerinde kalır — `CLAUDE.md` yeniden `2` der, kod `8`
  basar ve R7'nin "kodla çelişen cümle aynı commit'te düzelir" kuralı
  tekrar ihlal edilmiş olur. Bu ikisi phase-3 ile **birlikte** ya da hiç.
- **`ee4b31c` (phase-3) geri alınırsa**, jeton satırı beş eski alana
  (`kare`/`hucre`/`glif`/`kural`/`yuva`) döner ve `IDLE_FRAME_LIMIT` `8`'den
  `2`'ye iner — ama phase-3'ün ölçtüğü sağlıklı-koşu dağılımı (`kare` 1–2,
  bir kez 4) `2` sınırını yeniden **doğru bir build'i kırmızıya düşüren**
  hâline getirir. Geri alınırsa sabit elle yeniden gerekçelendirilmeli,
  körü körüne `2`'ye dönülmemeli.
- **`e991d78` (phase-2b) geri alınırsa**, kapanış yeniden sınırsız bekler —
  ölçüm yükü koşularının ölçülmüş ~%62'si yeniden asılır — **ve** `ee4b31c`
  bunun üstüne kurulu (`Teardown`/`kapanis=` onun döndürdüğü tipi okuyor),
  yani phase-3 de birlikte gitmeli.
- **`8df1ef6` (phase-2) geri alınırsa**, zaman yakalama (`Options`/`Stats`/
  `Ring`) kaybolur; hem phase-2b'nin thread ayrımı hem phase-3'ün raporu bunun
  üstüne kurulu — üçü de zincirin bir parçası.
- **`9788d95` (phase-1) geri alınırsa**, `load_shell`, `Workload`,
  `atlas_occupancy` ve `IDLE_FRAME_LIMIT`'in temeli gider — zincirin kökü,
  geri kalan dördü ona bağımlı.
- **Ayar şeması, tema, terminfo, shell entegrasyonu:** `plan.md`'nin `##
  Göç`'ü net — hiçbiri değişmedi, doğrulanacak bir geri düşüş yok.
- **`Cargo.lock`:** hiçbir adımda oynamadı, hiçbir revert'te de oynamaz.
- **Henüz push edilmediği için** en basit "geri alma" `/ship`'i çalıştırmamak:
  sorun push'tan önce bulunursa hiçbir revert gerekmez, yalnız düzeltme
  forward-fix olarak eklenir. Yukarıdaki zincir yalnız `main`'e gittikten
  **sonra** bir sorun çıkarsa geçerli.
