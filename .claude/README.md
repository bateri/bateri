# .claude — iş akışı

Dört skill, tek zincir:

```
/rfc  →  /plan-review  →  /implement  →  /ship
üretir    sınar            yürütür        teslim eder
```

| skill | ne yapar | ne YAPMAZ |
|---|---|---|
| `/rfc` | planlama seti üretir (`.tasks/NNN-slug/`) | kod yazmaz |
| `/plan-review` | tasarımı 3 mercekli jüriyle sınar — **yalnız pahalı kararda** (`/rfc` adım 6) | plan değiştirmez, önerir |
| `/implement` | phase başına kodlar, doğrular, tek commit atar; set kapısını son phase'in commit'inden önce koşar ve 🟢'yi o commit'e koyar | push etmez |
| `/ship` | doğrular (kod değişmediyse son yeşil koşuyu kullanır), gönderir; 🟢'yi yalnız eksikse tamamlar | set defteri tutmaz |

Zinciri baştan sona koşturan bir sürücü vardır ve kendisi iş yapmaz:

| skill | ne yapar | ne YAPMAZ |
|---|---|---|
| `/akis` | konuyu alır, seti ürettirir, gerekirse vetletir, kararı kaydeder, `--auto` ile kodlatır | aşamaların işini kendi yapmaz; teslim/push etmez |

`/akis`'in insandan devraldığı tek nokta `/rfc` adım 7'nin onayıdır; karar
kaydı yine yazılır (damga biçimi şablonda). Aşamaların kuralları
değişmez — sürücü yalnız sırayı, kapıları ve eskalasyonu yönetir.

İki yardımcı skill zincirin dışında, kendi başına da çağrılır:

| skill | ne yapar | ne zaman |
|---|---|---|
| `/audit` | bateri'ye özgü yargı mercekleri (bağımlılık kararı, ayar şeması, ölçüm sahipliği, thread, boşta sıfır kare, hücre ve shader düzeni, dil); mekanik yarısı `make denetim`'de | set sonundaki kalite kapısında `/code-review`'dan sonra; `/code-review` projeye özgü kural okumaz |
| `/measure` | kare süresi, giriş gecikmesi, bellek, açılış, boşta kare sınırı ve bench ölçer, `docs/OLCUMLER.md`'ye işler | **yalnız kullanıcı isteyince** — ölçüm kapı değildir, gerçek pencere ve sessiz makine ister |

## Ne zaman kullanılmaz

Zincir, **birden çok oturuma yayılan ve tasarım kararı içeren** işler içindir.
Şunlar için doğrudan çalış, set açma: tek dosyalık düzeltme, hata giderme,
belge güncellemesi, ölçüm yenileme, yeniden adlandırma. Bir işin planlama
maliyeti işin kendisini geçiyorsa akış yanlış araçtır.

Ara durum da meşrudur: tek `phase-1.md`'lik set (discussion yok, panel yok) —
`/rfc` bunu destekler, her iş çok fazlı olmak zorunda değildir. Set yürürken
çıkan tek commit'lik düzeltme de phase açmaz (`duzen.md` → Ek phase eşiği).

## Hafifletme (2026-09-15)

001–006'nın oturum dökümleri ve git geçmişi okunarak zincir hafifletildi.
Değişen kural ve sahibi:

| ne vardı | ne oldu | neden | sahibi |
|---|---|---|---|
| her phase'de `/simplify` → `/code-review` → `/audit` | her phase'de yalnız doğrulama (`make hepsi`, içinde `make denetim`); riskli phase'de ayrıca `/code-review`; set sonunda bir kez `/code-review` + `/audit` | kapı 17 phase'de 243 ajan açtı ve implementer'ın bağlamını şişirdi; mekanik mercekler grep'ti | `proje.md` → Kalite kapısı |
| hash'i checklist'e ve `## Durum`'a yaz | ✅ kod commit'inin içinde, hash yok; commit gövdesi `{set} phase-{N}` | hash commit'ten sonra belli olduğu için 121 commit'in 57'si defterdi | `duzen.md` → Durum |
| sadakat kontrolü | kaldırıldı | 17 phase'de 17 "makas yok"; kuralın kendi günbatımı şartı | `otonom-serit.md` §4 |
| kapıyı arka planda başlat, yokla | ön planda koş; bekleme döngüsü yasak | üç implementer'ın harcamasının büyük kısmı `echo tick`/`sleep` turuydu | `otonom-serit.md` → Ajan kuralları |
| bütün setler tek oturumda | bir oturum, bir set; devir `/clear` önerir | ana oturum 22 kez compact'landı | `otonom-serit.md` §1 |
| phase dosyasında kod örnekleri | kod örneği yok, hedef 3–5 KB | `.tasks/` üretilen kodu geçti | `sablonlar/phase.md` |
| `/ship` push'tan sonra damgalar | damga push'tan önce aynı commit'te | damga commit'i gönderilmeden kalıyordu | `ship` adım 4 |

Kalan, repo dışı kalem: uzun koşan ajanlar prompt önbelleğinin çalıştığı
yoldan koşmalı (`otonom-serit.md` → Önbellek ön koşulu).

## Sadeleştirme (2026-09-22)

007–022'nin git geçmişi ve `.tasks/` içeriği sayılarak ikinci tur. Hafifletme
defteri öldürmemişti, taşımıştı: 007'den sonraki 207 commit'in 71'i yalnız
`.tasks/`/`docs/`'a dokunuyordu ve sekiz set, kodu `main`'de olduğu hâlde
"ölçüm bekliyor" yüzünden süresiz 🔨'daydı. Her satır bir çıkarma ya da
daraltma; yeni kapı, defter ya da ajan yok.

| ne vardı | ne oldu | neden | sahibi |
|---|---|---|---|
| kare/gecikme iddiası taşıyan phase "ölçüm bekliyor" yazar, `/ship` bekleyen adımı 🔨 sayar | iddia yazılmaz; 🟢 = kod `main`'de + `make hepsi` yeşil + `kapı` satırı kapalı; bekleyen iddiaların tek yeri `docs/OLCUMLER.md` | bir phase'in iddiası setin durumuna dönüşüyordu; 12 iddianın kancası ya da yükü yoktu, bench'inki reddedilmiş bir bağımlılığı bekliyordu | `proje.md` → Doğrulama, `duzen.md` → İndeks |
| `teslim.md` (doğrulama + yayın checklist'i + geri alma) | yok; kapanış = kapı commit'i + indeks satırı + devir mesajı | 21 sette 21 kez "revert et", 61 `[komut]`/`[elle]` adımı, 49 `[~]`; hash yazan defter commit'leri `teslim.md`'de yeniden doğmuştu (017'de üç kez) | `proje.md` → Teslim; şablon silindi |
| phase'de `## Yayın Etkisi`, plan'da `## Göç`, context'te `## Kanıt` ve `## Mevcut Mimari` | hepsi yok | 94 phase'in 93'ü konusu olmayan başlığı doldurdu (tek branch, terminfo yok, bash/fish yok); gerçek yayın etkisi zaten `/audit` mercekleri ve eskalasyon listesi | `sablonlar/` |
| panel her discussion.md'de varsayılan açık, 3 × `opus` | yalnız birden çok yaklaşım **ve** pahalı sınıf (bağımlılık, katman, `Cell`, `TERM`, shell, her karede CPU) | 71 verdiktin 65'i SORUNLU, 1'i TEMİZ: %99 "sorunlu" diyen kapı ayırt etmiyor; gerçek KIRMIZI üç sette | `rfc` adım 6, `plan-review` → Ne zaman koşulur |
| `/implement` 0.5 muhakeme nag'i, adım 9 eksik-checklist kapısı, `/ship` adım 7 kalan dilim | yok | panel isteğe bağlıyken nag gürültü; waive sayımı hiç kod okumadı; kalan dilim teslim.md'nin adımlarıydı | `implement`, `ship` |
| phase hedefi 3–5 KB | cümle kalktı | 65 phase'in 59'u aştı (ort. 9,1 KB): ölü kural | `sablonlar/phase.md`, `rfc` adım 9 |
| gözle kontrol adsız | set kapısının 4. adımı: devir mesajında tek satır "neye bakılır" | 017'de üç kapı koştu, teslimden sonra beş kusuru kullanıcının gözü buldu — ürünü sınayan adım süreçte adsızdı; bu bir defter satırı değil, 🟢'yi bekletmez | `proje.md` → Kalite kapısı |

## Üçüncü tur (2026-09-23)

025 tek phase'li bir setti ve süreç onun etrafında yedi commit, beş yere
kopyalanmış bir sınır listesi ve koddan büyük bir belge üretti (706'ya 426
satır). Kullanıcı sordu; her satır yine bir çıkarma.

| ne vardı | ne oldu | neden | sahibi |
|---|---|---|---|
| `/ship` kapıyı "her zaman" koşar | aynı oturumda yeşil koştuysa ve arada yalnız belge değiştiyse koşmaz | kapı-defteri commit'inden sonra aynı sonuç bir buçuk dakikaya ikinci kez alındı | `ship` adım 1 |
| set kapısı son phase'den **sonra**, ayrı `{set} kapı` commit'i; 🟢 `/ship`'in ayrı commit'i | kapı son phase'in commit'inden **önce**; düzeltme, `kapı` ✅ ve 🟢 o commit'e girer | bir kod commit'inin etrafında altı defter commit'i | `proje.md` → Kalite kapısı, `implement` adım 6/8, `duzen.md` → İndeks |
| son phase'de riskli phase `/code-review`'u + set `/code-review`'u | yalnız set kapısı | aynı diff iki kez incelendi | `proje.md`, `otonom-serit.md` §5 |
| tek phase'li sette `plan.md` Yaklaşım/Akış + `R1.1` numaraları + `_Requirements:_` | yok; `phase-1.md` taşır | kapsama kontrolü tek phase'te hiçbir şey bulamaz; iki kopya biri eskir | `duzen.md` → Tek phase'li set |
| yol haritası açılmamış işlere numara verir | numara set açılınca | araya giren her set kaydırıyordu: on beş kayma notu | `docs/YOL-HARITASI.md` başı, `rfc` adım 3 |
| `/rfc` adım 7 her kararı kullanıcıya sorar | yalnız ürün kararı; teknik karar gerekçesiyle `## Karar`'a | kullanıcı teknik seçimi değerlendiremiyor ("bilmiyorum") | `rfc` adım 7, `plan-review` adım 4 |
| panel sınıfı konu adıyla ("shell entegrasyonu") | değişecek dosyayla (`assets/shell/`) | 025 betiğe dokunmadan panel açtı | `rfc` adım 6 |
| aynı bilgi birden çok set dosyasında ve `CLAUDE.md`'de | her bilgi tek yerde; `CLAUDE.md` kural + tek cümle + işaretçi | `CLAUDE.md` her oturumda okunuyor ve her set bir paragraf ekliyordu | `proje.md` → Teslim |

## Düzen

```
.claude/
├── is-akisi/
│   ├── duzen.md            ← iş seti düzeni: klasör adı, numaralandırma,
│   │                          dosya rolleri, durum tablosu, indeks  [TEK SAHİP]
│   ├── proje.md            ← PROJEYE ÖZGÜ: doğrulama komutları, kalite kapısı (phase / set),
│   │                          teslim kuralı, tuzaklar                [TEK SAHİP]
│   └── sablonlar/          ← belge biçimleri: context, discussion, plan,
│                              phase                                  [TEK SAHİP]
└── skills/
    ├── akis/SKILL.md        ← zincirin sürücüsü (konudan koda)
    ├── rfc/SKILL.md
    ├── plan-review/SKILL.md
    ├── implement/SKILL.md + references/otonom-serit.md
    ├── ship/SKILL.md
    ├── audit/SKILL.md       ← PROJEYE ÖZGÜ mercekler
    └── measure/SKILL.md     ← PROJEYE ÖZGÜ ölçüm türleri
```

Skill'ler kuralları gövdelerinde **tekrar etmez**, `is-akisi/` altına bağlanır.
Sebebi CLAUDE.md'deki ilkenin aynısı: aynı kural iki yerde dururken biri
düzeltilirse öteki sessizce eskir.

## Kökeni ve başka projeye taşıma

Bu yapı `odunluk` deposundan taşındı. Taşırken **yalnız şunlar** yeniden
yazıldı, gerisi olduğu gibi: `is-akisi/proje.md`, `skills/audit`,
`skills/measure`, `plan-review`'ın "bu projeye özgü mercek notları" bölümü,
`sablonlar/` içindeki örnek komutlar, skill'lerin `allowed-tools` satırları
(`go` → `cargo`) ve `settings.json` izinleri. Bir sonraki projeye taşırken
aynı liste geçerlidir; skill gövdeleri ve `duzen.md` projeden bağımsızdır.

## Çağırma

Skill'ler `/rfc`, `/plan-review`, `/implement`, `/ship` diye çağrılır; işi
komple devretmek için `/akis {konu}`. Ayrıca
`description` alanları sayesinde uygun bağlamda kendiliğinden de devreye
girebilirler — komutlarda olmayan tek fark budur.
