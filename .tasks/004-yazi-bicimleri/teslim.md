# Yazı biçimleri — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md)

Terminal çıktısının biçimi artık ekrana çıkıyor: kalın ve eğik gerçek font
yüzlerinden geliyor, beş alt çizgi çeşidi (düz, çift, kıvrım, noktalı,
kesikli) ve üstü çizili birer kural sprite'ı olarak atlasa giriyor ve `cell`
pipeline'ından çiziliyor, SGR 58 alt çizgiye ayrı renk veriyor. `make duman`
sözleşmesi `kural=R` jetonunu **ekliyor**, eskisini korur. Kapsam dışı kalanlar
değişmedi: emoji, geniş glyph, kutu çizim karakterleri, sentetik kalın/eğik,
kalın→parlak renk eşlemesi, `cell_rule` pipeline'ı — hâlâ görünmeyecek.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make shader
make test-yaris
make duman
```

`make shader` bu sette **zorunlu**: ride-along `d8ad544`
`crates/bt-gpu/shaders/cell.metal`'e dokundu (`float kapsama` → `coverage`),
yani `.metal` push aralığında değişti — koşulu doğmadı değil, koştu ve geçti.
`make test-yaris` de zorunlu: phase-2 `Session::frame`'in gövdesini `Term`
kilidi altında değiştirdi. `make terminfo` ve `make kur` **koşulmaz** —
girdileri henüz yok (`proje.md`'nin bilinen listesi), `assets/terminfo`
depoda yok.

### Beklenen çıktı

- `make hepsi` → çıkış 0, 79 sınama. (`1dbb084`'ün düzelttiği yarış süzgeci
  sayesinde bu sayı gerçek: eski `--ignored yaris_` deseni `race_*` testlerini
  hiç yakalamıyor, 0 test koşup yeşil dönüyordu.)
- `make shader` → çıkış 0. `.metal`'deki tek değişiklik bir tanımlayıcı adı
  (`kapsama` → `coverage`); MSL `static_assert`'leri ve Rust tarafındaki
  `offset_of!` karşılıkları oynamadı.
- `make test-yaris` → çıkış 0, iki zamanlama profili de (`race_*` stresi ve
  tek thread karşılaştırması).
- `make duman` → **öncesi** `kare=1 hucre=8 glif=6 pipeline=ok`, **sonrası**
  `kare=1 hucre=8 glif=6 kural=15 pipeline=ok`. Jeton **eklendi**; `hucre` ve
  `glif` bit bit korundu. `15`, phase metninin öngördüğü `7` değil: sekiz
  " bateri " hücresi `\033[41;1;4m` ile kalın **ve** düz altı çizili açılıyor,
  kendileri de birer kural üretiyor (8), üstüne beş stil + üstü çizili + SGR 58
  için yedi hücre daha geliyor (7) → 8 + 7 = 15 (phase-3 not 1). Sayı yazılmadı,
  reçeteden okunan bir iş sayımı.
- `docs/OLCUMLER.md` bu depoda **yok**; ölçüm bir kapı değil, B.3'te.
- `Cargo.lock` üç phase boyunca **oynamadı** (phase-1'in `CTFontTraits`
  feature satırı yeni crate çekmedi).

### Doğrulama Checklist

- [x] `make hepsi` yeşil (79 sınama)
- [x] `make shader` yeşil (`.metal` bu pushta değişti — `d8ad544`)
- [x] `make test-yaris` yeşil (iki profil)
- [x] `make duman` → `kare=1 hucre=8 glif=6 kural=15 pipeline=ok`
- [~] `make terminfo` — girdisi yok (`assets/terminfo` depoda yok), koşulu doğmadı
- [x] `Cargo.lock` oynamadı

## B. Yayın (doğrulamadan SONRA)

Üç phase'in `## Yayın Etkisi` bloklarından derlendi; "yok" diyen bloklar
(terminfo, ayar şeması, tema/materyal, shell entegrasyonu, app bundle)
atlandı. Shader hariç tutulmadı — aşağıdaki çelişki onu gerçek bir A adımına
çeviriyor.

**Çözülen çelişki:** phase-3'ün `## Yayın Etkisi`'si "shader — yok, `.metal`
dokunulmadı → `make shader` `[~]`" diyor; bu doğruydu ama yalnız phase-3'ün
kendi diff'i için. `## Uygulama Notları`'ndaki orkestratör kararı (phase-3.md
sonu) `d8ad544`'ün `cell.metal`'e dokunduğunu kaydediyor — ride-along commit,
ama aynı push aralığında. Notlar kazanıyor: `/ship`'in göreceği aralıkta
`.metal` gerçekten değişti, yani A bölümünde `make shader` `[~]` değil
**gerçek, koşmuş ve geçmiş bir adım**.

### B.1 `/ship` — dallanmamış `main` push'u `[oto]`

> **Koşuldu: 2026-09-11, `a50f846..dd77945`, 16 commit.** Doğrulama push'tan
> önce yeniden koşuldu ve dördü de tetiklendi: `make hepsi` 0 (79 sınama),
> `make shader` 0 (`.metal` bu aralıkta değişti), `make test-yaris` 0
> (`frame()` gövdesi `Term` kilidi altında değişti), `make duman` 0 →
> `kare=1 hucre=8 glif=6 kural=15 pipeline=ok`. `Cargo.lock` oynamadı.

Sayıyı `/ship` adım 2 sayar (`git log origin/main..HEAD`) — buraya sabit
yazılan bir sayı, teslimden önce düşen her düzeltmeyle bayatlar. Bu
belge yazılırken hiçbiri push edilmemişti; omurga sırayla:
`9311e1b` (set kurulumu) → `9b9d63f`, `b0e2e38` (planlama damgaları) →
`bb04da7` (phase dosyaları) → `92607aa` (phase-1) → `004c61a` (damga) →
`1dbb084` → `643c8c6` (phase-2) → `271c670`, `b648d56` (damgalar) →
`c3d7359` (phase-3) → `7659cc1` (damga) → `d8ad544`.

İki commit phase dışı ama yayın etkisi taşıyor, `/ship` bunları da gönderir:

- **`1dbb084`** — 270 tanımlayıcıyı İngilizceye çevirdi, 15 kaynak dosyada;
  `CLAUDE.md`, `.claude/is-akisi/proje.md` ve `.claude/skills/audit/SKILL.md`
  lens 10'daki dil kuralını daralttı. `Makefile`'ın yarış süzgeci
  `--ignored yaris_` → `race_` düzeltildi — eski süzgeç 0 test koşup yeşil
  dönüyordu (yukarıdaki "Beklenen çıktı" notu).
- **`d8ad544`** — `cell.metal`'de `float kapsama` → `coverage`, `curl()`'ün
  doc'u kodla (`.min(h as f32)`) uyumlandı. `make shader` koşuldu, exit 0.

### B.2 Göz kontrolü `[elle]`

> **Sonuç: geçti (kullanıcı, 2026-09-11, `cd97728` ağacı).** Dört madde
> `~/g.sh` betiğiyle koşuldu — betik kaçış dizilerini kendisi bastı, yani
> `4:3`'ün iki noktası elle yazılmadı ve yanlış yazılma riski (`4;3` →
> altı çizili + eğik) hiç doğmadı. Uygulama `BT_RUN_SECONDS`'sız açıldı,
> yani kullanıcının kendi `$SHELL`'iyle; duman koşusunun sabit shell'i
> değil.

Bu set ekranda görünen değişiklik üretiyor; hiçbir otomatik kapı "doğru
görünüyor mu"yu göremez. Reçete:

```sh
printf '\033[1mkalın\033[0m \033[3meğik\033[0m \033[4:3mkıvrım\033[0m \033[4:3;58;5;196mkırmızı\033[0m \033[9müstü\033[0m'
```

Bakılacak:

1. **Kalın** gerçekten kalın **yüz** mü (sentetik kalınlaştırma değil —
   yüzün kendi kalın gövdesi).
2. **Kıvrım** düz çizgi değil **dalga** mı.
3. SGR 58'li kıvrım (**kırmızı** sözcüğü) kırmızı mı, ön plan renginde değil.
4. İmleci altı çizili bir harfe getirince çizgi imleç bloğunun **üstünde**
   kalıyor mu, altında kaybolmuyor mu — reçetenin kendisi imleci metnin
   sonunda bırakıyor, bunun için ayrı bir satır gerekiyor:

   ```sh
   printf '\033[4:3;58;5;196mkırmızı\033[0m\033[3D'; sleep 5
   ```

   `\033[3D` imleci `kırmızı`'nın ikinci `ı`'sının üstüne 3 hücre geri alıyor,
   `sleep 5` orada tutuyor (koşulmadı — teyit gözle yapılır).

### B.3 Ölçüm bekleyen beş iddia `[komut]`

`docs/OLCUMLER.md` bu depoda **yok** ve ölçüm kancaları (`BT_FRAME_LOG`,
`BT_SCROLL_TEST`, `BT_STARTUP_TRACE`) da yok — 002 ve 003'ün bekleyen
ölçümleriyle aynı durumdu. **005 sonrası güncellendi:** kanca seti geldi (`BT_FRAME_STATS`, `BT_SCROLL_TEST`, açılış damgası) ve `/measure` artık sayı üretebiliyor. Tarihli kayıtlar o gün doğruydu, dokunulmadı. O gün `/measure` sayı değil **"ölçüm aracı yok"**
döndürür (003 `teslim.md` B.1'de kanıtlandı: `cargo bench --workspace --
--list` → `0 benchmarks`). Beşi de kanca seti gelene kadar eylemsiz kalıyor,
sayı uydurulmadı:

1. **phase-1** — dört yüzün atlas kurulumundaki ana thread bedeli (`Atlas::new`
   artık bir yerine dört `CTFont` açıyor, `ensure()` her ölçek değişiminde
   `*self = Self::new(...)` yapıyor) — 003 B.1 **#4**'ün genişlemesi.
2. **phase-1** — yüz başına ayrı yuva tutulmasının atlas doluluğuna etkisi —
   003 B.1 **#3**'ün genişlemesi. `Faces::effective` tek yüzlü ailelerde
   (Monaco gibi) yuvaları tek mekanizmaya indiriyor ama çok yüzlü bir ailede
   (Menlo) dört yüz dört ayrı yuva kaplıyor; aritmetik plana yazıldı
   (`(1024/w)×(1024/h)` kapasite, karakter × yüz), sayı yok.
3. **phase-2** — sınır `Cell`'inin 5 alandan 10'a çıkmasının kare süresine
   etkisi — 003 B.1 **#5**'in genişlemesi. `size_of` bir gerçek (`24 bayt`
   grid hücresine bağlı, sınır `Cell`'i ayrı), **kare süresine etkisi**
   ölçüm bekliyor.
4. **phase-3** — kare başına kural instance'larının ve `Atlas::slot`'un
   ikinci çağrı sınıfının kare süresine etkisi — 003 B.1 **#2** ve **#5**'in
   genişlemesi. **Aday çözüm** kayıtlı (`/simplify` → Efficiency 1, ölçümden
   önce uygulanmadı): kural uv'lerini kare başına altı yuvalık bir memoya
   almak; bedeli `RuleKind`'ın varyant sayısını `bt-gpu`'ya yazmak.
5. **phase-3** — altı çizili hücrenin artık iki tam hücre dörtlüsünü
   (glyph + kural) alfa pipeline'ından geçirmesinin fragment maliyeti —
   "aynı tampon, aynı draw call" kararının (`discussion.md` Karar 4)
   bilinen bedeli, `cell_rule` pipeline'ı kapsam dışı bırakılırken kabul
   edildi.

```sh
/measure 004-yazi-bicimleri
```

### B.4 Bilinen borç — kayıt `[oto]`

Phase-3'ün `/code-review`'u 8 bulgu devretmişti; orkestratör ikisini aynı
sette kapattı (`d8ad544`: `cell.metal`'in dil ihlali ve `curl()`'ün bayat
doc'u — ikisi de depo kuralı ihlaliydi, ertelenmeleri kuralın kendisini
aşındırırdı). Kalan **6** borç:

1. **`DIM`'in `underline_color`'a uygulanmaması** — doğru davranış
   doğrulanamıyor: alacritty'nin çizicisi bağımlılık değil, `plan.md` R3.5
   çözüm yolunu dim'siz tarif ediyor; tahminle düzeltmek asıl hata olurdu.
2. **`dividing_period`'in asal hücre genişliklerinde `Dotted` ile `Dashed`'i
   aynı desene indirmesi** — önerilen "en yakın bölen" düzeltmesi `w=13`'te
   `Dotted`'ı düz çizgiye çeviriyor; tasarım kararı ister, hata düzeltmesi
   değil.
3. **`RULE_RESERVE = 6`'nın `RuleKind`'ın varyant sayısına derleme zamanında
   bağlı olmaması** — canlı hata yok.
4. **`curl_is_continuous_across_cell_edges`'in uzunluk bekçisi eksikliği.**
5. **`Metrics`'in iki yeni alanının (`underline_px`, `strikeout_px`) `pub`
   olması** — `pub(crate)` yeterdi.
6. **`Cargo.toml`'daki gerekçe yorumunun `features` dizisinin içinde
   durması** — dışarı, satırın üstüne taşınmalı.

Kapsam dışı ama görünürlüğü korunsun diye ayrıca not: `plan.md`'nin
"Kapsam Dışı" bölümü 003'ten devreden ölçek borusu borcunu
(`Surface::set_size` ve `cell_metrics`'in `bt-gpu`'ya iki ayrı kapısı)
**yeniden erteliyor** — bu 6'nın dışında, yazı biçimiyle ilgisiz.

### Yayın Checklist

<!-- `/ship` bekleyen manuel adımları BU başlık altında arar. -->

- [x] B.1 `/ship` push `[oto]` — `a50f846..dd77945`, 16 commit, 2026-09-11
- [x] B.4 Bilinen borç kaydı `[oto]` — 6 kalem `teslim.md`'de, gerekçeleri
      phase-3'ün `## Uygulama Notları`'nda
- [x] B.2 Göz kontrolü `[elle]` — **kullanıcı 2026-09-11'de yaptı, geçti.**
      Doğrulayan kullanıcıdır; bu satır onun raporudur, otomatik bir kapı değil
- [ ] B.3 `/measure 004-yazi-bicimleri` `[komut]` — beş iddia bekliyor.
      **005 kanca setini getirdi**, komut artık koşulabilir. Kanca adları
      değişti: `BT_FRAME_LOG` ve `BT_STARTUP_TRACE` yok, ikisinin yerine
      `BT_FRAME_STATS` (açılış aynı bayrağın altında). `#4`'ün bench yarısı
      açık kalıyor

## Geri Alma

Sıra **tersten**: `c3d7359` → `643c8c6` → `92607aa` (her biri kurulduğu
katmana bağımlı olanı önce sökülür).

- **`c3d7359` (phase-3) geri alınırsa:** `kural=` jetonu çıktıdan **ve**
  `CLAUDE.md`/`Makefile`/`proje.md`'den aynı anda kaybolur (aynı commit'te
  girdiler) — tutarlı, ama jetonu arayan bir okuyucu "artık yok" sinyalini
  yalnız kodun kendisinden alır, ayrı bir uyarı yayınlanmaz. `d8ad544`
  **bağımsız geri alınamaz**: `cell.metal`'in `coverage`'ını `kapsama`'ya
  döndürür, yani dil kuralını yeniden ihlal eder — `c3d7359` ile birlikte ya
  da hiç.
- **`643c8c6` (phase-2) geri alınırsa:** `c3d7359`'un `Frame::push`'u
  `643c8c6`'nın eklediği beş `Cell` alanını (`bold`, `italic`, `underline`,
  `underline_color`, `strikeout`) **okuyor**; `643c8c6` önce sökülürse
  `bt-gpu` derlenmez. Sıra bu yüzden `c3d7359` → `643c8c6` (altı sınama
  literali ve `impl Default for Cell` zaten `643c8c6`'nın içinde, birlikte
  gider).
- **`92607aa` (phase-1) geri alınırsa:** `git revert` **temiz uygulanmaz** —
  `1dbb084` bu commit'in tanımlayıcılarını (`Yuzler`, `yuz_turet`,
  `ciz_kural` → `Faces`, `derive_face`, ...) yeniden adlandırdı. Elle çakışma
  çözümü gerekir. `1dbb084`'ün kendisi geri alma **seçeneği değil**: hem
  270 tanımlayıcıyı yeniden Türkçeye çevirir hem de düzelttiği boş yarış
  süzgecini (0 test koşup yeşil dönen hâl) geri getirir.
- **Ayar şeması, tema, terminfo:** `plan.md`'nin `## Göç`'ü net —
  hiçbiri değişmedi, doğrulanacak bir geri düşüş yok.
- **`Cargo.lock`:** hiçbir revert'te oynamaz; üç phase boyunca zaten
  oynamadı.
