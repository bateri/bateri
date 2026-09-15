# .claude — iş akışı

Dört skill, tek zincir:

```
/rfc  →  /plan-review  →  /implement  →  /ship
üretir    sınar            yürütür        teslim eder
```

| skill | ne yapar | ne YAPMAZ |
|---|---|---|
| `/rfc` | planlama seti üretir (`.tasks/NNN-slug/`) | kod yazmaz |
| `/plan-review` | tasarımı 3 mercekli jüriyle sınar | plan değiştirmez, önerir |
| `/implement` | phase başına kodlar, doğrular, tek commit atar; set sonunda kalite kapısını koşar | push etmez |
| `/ship` | doğrular, commit'ler, `main`'e gönderir | manuel adımları kendiliğinden koşmaz |

Zinciri baştan sona koşturan bir sürücü vardır ve kendisi iş yapmaz:

| skill | ne yapar | ne YAPMAZ |
|---|---|---|
| `/akis` | konuyu alır, seti ürettirir, vetletir, kararı kaydeder, `--auto` ile kodlatır | aşamaların işini kendi yapmaz; teslim/push etmez |

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

## Düzen

```
.claude/
├── is-akisi/
│   ├── duzen.md            ← iş seti düzeni: klasör adı, numaralandırma,
│   │                          dosya rolleri, durum tablosu, indeks  [TEK SAHİP]
│   ├── proje.md            ← PROJEYE ÖZGÜ: doğrulama komutları, kalite kapısı (phase / set),
│   │                          yayın etkisi, branch akışı, tuzaklar   [TEK SAHİP]
│   └── sablonlar/          ← belge biçimleri: context, discussion, plan,
│                              phase, teslim                          [TEK SAHİP]
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
