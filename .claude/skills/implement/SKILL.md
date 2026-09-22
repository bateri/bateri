---
name: implement
description: Bir planlama setinin (.tasks/NNN-slug/) phase dosyalarını sırayla hayata geçirir — kodu yazar, testleri koşar, phase başına tek commit atar, set sonunda kalite kapısından geçirir. Kullanıcı planlanmış bir işi uygulamak, "phase'lere geç", "hadi yapalım" ya da yarım kalmış bir setten devam etmek istediğinde kullanılır. Push/teslim etmez.
allowed-tools: Read, Write, Edit, Glob, Grep, Bash(git:*), Bash(ls:*), TodoWrite, Agent, Skill
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

- `.claude/is-akisi/duzen.md` — set düzeni, phase sıralaması, durum tablosu, set aralığı, **kalite kapısı, commit biçimi**
- `.claude/is-akisi/proje.md` — **kapı komutu, koşullu komutlar, riskli phase tetikleyicileri**

## Girdi

`$ARGUMENTS` = iş klasörü (`{NNN-slug}`, ya da yalnız numarası/slug'ı)
+ opsiyonel `--auto`. Verilmemişse `.tasks/` altındaki setleri durumlarıyla
listele ve sor.

Sette hiç `phase-*.md` yoksa bu **planlama-only** settir: "`/rfc` ile önce
phase dosyalarını üret" de ve dur.

## 0. Ön uçuş (bir kez)

**0.1 Yükle.** `plan.md`'yi oku. Phase dosyalarının gövdesini yalnız sırası
gelince oku (insan-döngüde bu oturum, otonom şeritte implementer).

**0.2 Sırala.** `phase-*.md` glob'la ve **doğal** sırala (`duzen.md` → Phase
numaraları).

**0.3 Traceability (hafif).** Yalnız **çok phase'li** sette: her gereksinim en
az bir phase'in `_Requirements:_` satırında geçiyor mu, satırı olmayan phase
var mı — grep'le bak, gövde açma. İhlal varsa **raporla ve devam/düzelt onayı
al**. Tek phase'li sette atla: tek phase her şeyi kapsar, kontrol hiçbir şey
bulamaz (`duzen.md` → Tek phase'li set).

**0.4 Resume noktası.** `plan.md → ## Durum`: ilk `✅` olmayan satır. Tablo
yoksa ya da şüpheliyse phase'in commit'ini gövdedeki `{set} phase-{N}`
satırından ara (`git log --grep`); bulunamazsa checklist'i `git status`/`git
diff` ile doğrula ve sor: "phase-N'den devam ediyorum, doğru mu?"

Bütün phase'ler ✅ ama `kapı` satırı boşsa (akış kapıdan önce kesilmiş):
adım 8'i koş ve sonucunu tek `{set} kapı` commit'iyle kapat — ayrı kapı
commit'inin tek meşru hâli bu. İkisi de doluysa set bitmiştir; `/ship`'e yönlendir.

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

**4. Doğrula.** `proje.md` → Doğrulama: kapı komutu ve tetiklenen koşullu
komutlar; **geçmeli**.

Ölçüm bir kapı değildir ve iddiası yazılmaz (`duzen.md` → Kalite kapısı →
Ölçüm).

**5. Riskli phase kapısı.** Phase `proje.md` → Riskli phase
tetikleyicilerinden birini tetiklediyse `/code-review`'u `Skill` aracıyla, ön planda
koştur; kod değiştiyse adım 4'ü yeniden koş. Tetiklemediyse bu adım yoktur —
tam kapı set sonunda koşar (adım 8). **Son phase'de** bu adım adım 8'in içinde
erir: set kapısı zaten o phase'in commit'inden önce koşuyor. Gideremediğin bulguyu phase dosyasına
gerekçesiyle waive olarak yaz; kapının **kendisi** koşmadıysa kutusu `[~]`
olur ve adım 7'de söylenir.

**6. Commit (phase = tek commit).** Checklist'i işaretle, `plan.md ## Durum`'da
phase'i `✅` yap, ilk phase'se `.tasks/README.md`'de setin satırını **🔨**
yap — hepsi kodla **aynı commit'e** girer. **Son phase'de** önce adım 8'i koş;
kapının düzeltmeleri, `kapı` satırının ✅'ü ve indeksin 🟢'si de bu commit'e
girer. Ayrı kapı ya da damga commit'i yoktur. İleti biçimi `duzen.md` → Teslim
(gövdenin ilk satırı `{set} phase-{N}`). Hash dosyaya yazılmaz, defter için
ayrı commit atılmaz. **Push etme.**

**7. Onay kapısı.** Sonraki phase'e geçmeden dur: değişen dosyalar, commit
hash'i ve sapmaların özeti; onay bekle. Özet **her seferinde** ayrı bir
"atlanan kapılar" satırı taşır (`[~]` işaretli her madde, gerekçesiyle). Son
phase'de bu kapı yoktur; adım 9'a geç.

Onay sonrası aynı oturumda devam edilebilir ya da **taze bağlam (önerilen)**:
`/clear`, sonra yeniden `/implement {set}`. `/compact` kullanma — kayıplı
özet, dosyalardaki kanonik kayıtla yarışan ikinci kaynak olur.

## Kapanış (son phase'in commit'inden önce, bir kez)

**8. Set kapısı.** Son phase'in kodu yazılıp doğrulandıktan sonra, **commit'ten
önce**: `duzen.md` → Kalite kapısı → "Set sonunda". `/code-review` setin
aralığında (`duzen.md` → Set aralığı; son phase henüz commit'lenmediği için
aralık + çalışma ağacı), ardından `/audit`. Bulgu düzeltildiyse doğrulamayı
yeniden koş. Sonra `## Durum`'un `kapı` satırını ✅, indeks satırını **🟢**
yap ve notu tek cümleye getir ("N phase + kapı tamam") — hepsi son phase'in
commit'ine girer (adım 6). Kapı koşamadıysa `kapı` satırına `[~] {gerekçe}`
yazılır (🟢 yine konur, `duzen.md` → İndeks) ve adım 9'da söylenir. Otonom şeritte bunu son phase'in implementer'ı yapar (otonom
şerit §5).

Eksik-checklist taraması **yok**: kapı komutu yeşil ve `## Durum` ✅ ise
phase bitmiştir; kutu `[ ]` kaldıysa phase commit'inde işaretlenmemiştir, o
kadar. (Bir dönem waive sayımı ve "kutuyu geri koy" ritüeli vardı; 49 `[~]`
üretti ve hiçbirini kod okumadı.)

**9. Devir.** Son phase'in commit'inden sonra kapanış özeti: phase'ler + commit hash'leri (git log'dan), kayda
değer sapmalar, waive'ler ve **gözle kontrol satırı** (`duzen.md` → Kalite
kapısı 4: kullanıcının gördüğü davranış değiştiyse neye bakılacak, tek satır). Net
yönlendirme: **"Teslim için `/ship`; sonraki set için `/clear`."**
