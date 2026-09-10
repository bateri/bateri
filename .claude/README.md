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
| `/implement` | phase'leri kodlar, test eder, commit atar | push etmez |
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
| `/audit` | bateri'ye özgü mercekler (katman yönü ve `bt-core`'un platformsuzluğu, panik yolu, boşta sıfır kare, hücre boyutu ve shader düzeni, ayar şeması, shell üçlüsü, bağımlılık, ölçüm sahipliği) | kalite kapısının üçüncü adımı; `/simplify` ve `/code-review` projeye özgü kural okumaz |
| `/measure` | kare süresi, giriş gecikmesi, bellek, açılış ve bench ölçer, `docs/OLCUMLER.md`'ye işler | **yalnız kullanıcı isteyince** — ölçüm kapı değildir, gerçek pencere ve sessiz makine ister |

## Ne zaman kullanılmaz

Zincir, **birden çok oturuma yayılan ve tasarım kararı içeren** işler içindir.
Şunlar için doğrudan çalış, set açma: tek dosyalık düzeltme, hata giderme,
belge güncellemesi, ölçüm yenileme, yeniden adlandırma. Bir işin planlama
maliyeti işin kendisini geçiyorsa akış yanlış araçtır.

Ara durum da meşrudur: tek `phase-1.md`'lik set (discussion yok, panel yok) —
`/rfc` bunu destekler, her iş çok fazlı olmak zorunda değildir.

## Düzen

```
.claude/
├── is-akisi/
│   ├── duzen.md            ← iş seti düzeni: klasör adı, numaralandırma,
│   │                          dosya rolleri, durum tablosu, indeks  [TEK SAHİP]
│   ├── proje.md            ← PROJEYE ÖZGÜ: doğrulama komutları, kalite kapısı,
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
