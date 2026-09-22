---
name: implement
description: Bir planlama setinin (.tasks/NNN-slug/) phase dosyalarını sırayla hayata geçirir — kodu yazar, testleri koşar, phase başına tek commit atar, set sonunda kalite kapısından geçirir. Kullanıcı planlanmış bir işi uygulamak, "phase'lere geç", "hadi yapalım" ya da yarım kalmış bir setten devam etmek istediğinde kullanılır. Push/teslim etmez.
allowed-tools: Read, Write, Edit, Glob, Grep, Bash(make:*), Bash(cargo:*), Bash(git:*), Bash(ls:*), TodoWrite, Agent, Skill
---

Kullanıcı `/rfc` ile üretilmiş bir planlama setini **phase phase hayata
geçirmek** istiyor. Phase dosyaları kodun kılavuzudur; bu skill o kılavuzu
yürütür.

**Zincir:** `/rfc` üretir → `/plan-review` sınar → `/implement` yürütür (bu
skill) → `/ship` teslim eder. **Bu skill push etmez.**

Akış varsayılan olarak **insan-döngüdedir**: her phase sonunda durulur, onay
alınır, devam edilir. `--auto` verildiyse bkz.
[references/otonom-serit.md](references/otonom-serit.md). Subagent başlatılan
her durumda o dosyanın **Ajan kuralları** isteme yazılır.

Okunacak sözleşmeler:

- `.claude/is-akisi/duzen.md` — set düzeni, phase sıralaması, durum tablosu, set aralığı
- `.claude/is-akisi/proje.md` — **doğrulama komutları, kalite kapısı, commit biçimi**

## Girdi

`$ARGUMENTS` = iş klasörü (`003-glyph-atlas`, ya da yalnız numarası/slug'ı)
+ opsiyonel `--auto`. Verilmemişse `.tasks/` altındaki setleri durumlarıyla
listele ve sor.

Sette hiç `phase-*.md` yoksa bu **planlama-only** settir: "`/rfc` ile önce
phase dosyalarını üret" de ve dur.

## 0. Ön uçuş (bir kez)

**0.1 Yükle.** `plan.md`'yi oku. Phase dosyalarının gövdesini yalnız sırası
gelince oku (insan-döngüde bu oturum, otonom şeritte implementer).

**0.2 Sırala.** `phase-*.md` glob'la ve **doğal** sırala (`duzen.md` → Phase
numaraları).

**0.3 Traceability (hafif).** `plan.md`'de `## Gereksinimler` varsa her
gereksinim en az bir phase'in `_Requirements:_` satırında geçiyor mu, satırı
olmayan phase var mı — grep'le bak, gövde açma. İhlal varsa **raporla ve
devam/düzelt onayı al**. Bölüm yoksa atla.

**0.4 Resume noktası.** `plan.md → ## Durum`: ilk `✅` olmayan satır. Tablo
yoksa ya da şüpheliyse phase'in commit'ini gövdedeki `{set} phase-{N}`
satırından ara (`git log --grep`); bulunamazsa checklist'i `git status`/`git
diff` ile doğrula ve sor: "phase-N'den devam ediyorum, doğru mu?"

Bütün phase'ler ✅ ise doğrudan Kapanış'a geç.

**0.5 Todo kur.** `TodoWrite` ile resume noktasından itibaren phase başına bir
todo. Kalıcı kayıt `## Durum` + checklist'lerdir.

## Phase döngüsü (sırayla, resume noktasından)

**1. Uygula.** YALNIZ `plan.md` + bu phase dosyasını referans al; geçmiş
phase'lerin özeti kendi `## Uygulama Notları`'ndadır.

**2. Test-first (opt-in).** Checklist'te `Test: {senaryo}` varsa önce testi
yaz, **fail ettiğini doğrula**, sonra implementasyona geç.

**3. İşaretle ve sapmaları kaydet.** Biten maddeyi `[x]` yap. İlk varsayımdan
**sapan** her şeyi `## Uygulama Notları`'na yaz — madde başına bir-iki satır,
yalnız sapma. Planın tekrarı, keşif günlüğü ya da "şunu da okudum" notu girmez.

**4. Doğrula.** `proje.md` → Doğrulama; **geçmeli**. Koşullu komutlar (shader,
yarış, duman, kur) orada tanımlıdır.

Ölçüm bir kapı değildir ve iddiası yazılmaz (`proje.md` → Doğrulama).

**5. Riskli phase kapısı.** Phase `proje.md` → Kalite kapısı → "Riskli
phase" koşulunu tetiklediyse `/code-review`'u `Skill` aracıyla, ön planda
koştur; kod değiştiyse adım 4'ü yeniden koş. Tetiklemediyse bu adım yoktur —
tam kapı set sonunda koşar (adım 8). Gideremediğin bulguyu phase dosyasına
gerekçesiyle waive olarak yaz; kapının **kendisi** koşmadıysa kutusu `[~]`
olur ve adım 7'de söylenir.

**6. Commit (phase = tek commit).** Checklist'i işaretle, `plan.md ## Durum`'da
phase'i `✅` yap, ilk phase'se `.tasks/README.md`'de setin satırını **🔨**
yap — hepsi kodla **aynı commit'e** girer. İleti biçimi `proje.md` → Teslim
(gövdenin ilk satırı `{set} phase-{N}`). Hash dosyaya yazılmaz, defter için
ayrı commit atılmaz. **Push etme.**

**7. Onay kapısı.** Sonraki phase'e geçmeden dur: değişen dosyalar, commit
hash'i ve sapmaların özeti; onay bekle. Özet **her seferinde** ayrı bir
"atlanan kapılar" satırı taşır (`[~]` işaretli her madde, gerekçesiyle). Son
phase'de bu kapı yerine Kapanış'a geç.

Onay sonrası aynı oturumda devam edilebilir ya da **taze bağlam (önerilen)**:
`/clear`, sonra yeniden `/implement {set}`. `/compact` kullanma — kayıplı
özet, dosyalardaki kanonik kayıtla yarışan ikinci kaynak olur.

## Kapanış (son phase sonrası, bir kez)

**8. Set kapısı.** `proje.md` → Kalite kapısı → "Set sonunda": `/code-review`
setin aralığında (`duzen.md` → Set aralığı), ardından `/audit`. Bulgu
düzeltildiyse `make hepsi` ve tek commit (`{set} kapı`); o commit `## Durum`'un
`kapı` satırını ✅ yapar ve indeks notunu tek cümleye getirir ("N phase +
kapı tamam"). Düzeltme yoksa ikisi de aynı **tek** defter commit'ine girer —
setin son commit'i budur, başka defter yok. Kapı koşamadıysa `[~] {gerekçe}`
ve adım 9'da söylenir. Otonom şeritte bunu tek bir kapı ajanı yapar (otonom
şerit §5). 🟢 `/ship`'in işidir.

Eksik-checklist taraması **yok**: `make hepsi` yeşil ve `## Durum` ✅ ise
phase bitmiştir; kutu `[ ]` kaldıysa phase commit'inde işaretlenmemiştir, o
kadar. (Bir dönem waive sayımı ve "kutuyu geri koy" ritüeli vardı; 49 `[~]`
üretti ve hiçbirini kod okumadı.)

**9. Devir.** Kapanış özeti: phase'ler + commit hash'leri (git log'dan), kayda
değer sapmalar, waive'ler ve **gözle kontrol satırı** (`proje.md` → Kalite
kapısı 4: pencereyi açan davranış değiştiyse neye bakılacak, tek satır). Net
yönlendirme: **"Teslim için `/ship`; sonraki set için `/clear`."**
