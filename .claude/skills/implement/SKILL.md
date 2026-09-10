---
name: implement
description: Bir planlama setinin (.tasks/NNN-slug/) phase dosyalarını sırayla hayata geçirir — kodu yazar, testleri koşar, kalite kapısından geçirir, phase başına commit atar ve kapanışta teslim.md derler. Kullanıcı planlanmış bir işi uygulamak, "phase'lere geç", "hadi yapalım" ya da yarım kalmış bir setten devam etmek istediğinde kullanılır. Push/teslim etmez.
allowed-tools: Read, Write, Edit, Glob, Grep, Bash(make:*), Bash(cargo:*), Bash(git:*), Bash(ls:*), TodoWrite, Agent, Skill
---

Kullanıcı `/rfc` ile üretilmiş bir planlama setini **phase phase hayata
geçirmek** istiyor. Phase dosyaları kodun birebir kılavuzudur; bu skill o
kılavuzu yürütür.

**Zincir:** `/rfc` üretir → `/plan-review` sınar → `/implement` yürütür (bu
skill) → `/ship` teslim eder. **Bu skill push etmez.** Kapanışta yalnız
`teslim.md`'yi derler; commit'leri `main`'e göndermek `/ship` kararıdır.

Akış varsayılan olarak **insan-döngüdedir**: her phase sonunda durulur, onay
alınır, devam edilir. `--auto` verildiyse bkz.
[references/otonom-serit.md](references/otonom-serit.md).

Okunacak sözleşmeler:

- `.claude/is-akisi/duzen.md` — set düzeni, phase sıralaması, durum tablosu, indeks
- `.claude/is-akisi/proje.md` — **doğrulama komutları, kalite kapısı, yayın etkisi**
- `.claude/is-akisi/sablonlar/teslim.md` — kapanışta derlenecek biçim

## Girdi

`$ARGUMENTS` = iş klasörü (`003-ek-fiil-indeksi`, ya da yalnız numarası/slug'ı)
+ opsiyonel `--auto`. Verilmemişse `.tasks/` altındaki setleri durumlarıyla
listele ve sor.

Sette hiç `phase-*.md` yoksa (yalnız context/plan var): bu **planlama-only**
settir. "`/rfc` ile önce phase dosyalarını üret" de ve dur.

## 0. Ön uçuş (bir kez)

**0.1 Yükle.** `plan.md`'yi ve bütün phase dosyalarını oku. `plan.md` yoksa
çökme, doğrudan phase dosyalarından çalış.

**0.2 Sırala.** `phase-*.md` glob'la ve **doğal** sırala (`duzen.md` → Phase
numaraları). Leksik sıralama `phase-10`'u `phase-2`'nin önüne atar.

**0.3 Traceability (hafif).** `plan.md`'de `## Gereksinimler` varsa: her
gereksinim en az bir phase'in `_Requirements:_` satırında geçiyor mu (alt
gereksinim karşılandıysa üstü de kapsanmış sayılır), `_Requirements:_` satırı
olmayan phase var mı. İhlal varsa **raporla ve devam/düzelt onayı al** —
sessizce geçme. Bölüm yoksa bu adımı atla, uyarma.

**0.4 Resume noktası.** Birincil sinyal `plan.md → ## Durum` tablosudur:
resume noktası ilk `✅` olmayan phase. Tablo yoksa sinyalleri çaprazla ve her
birinin tuzağını bil:

- **Checklist `[x]` oranı** — tek başına güvenilmez, aynı set içinde karışık olabilir.
- **Phase'in commit'i** — commit'i phase'in **dokunduğu dosyalarla** eşle,
  `"Phase N"` metin grep'iyle değil; o etiket setler arası çakışır.
- **`## Uygulama Notları` doluluğu** — zayıf ikincil ipucu; salt refactor
  phase'inde bölüm boş kalabilir, boşluk "bitmedi" demek değildir.

Seçili phase'in commit'i yoksa "devam ediyor" say. Kısmen işaretliyse maddeleri
`git status`/`git diff` ile tek tek doğrula. Belirsizsen sor: "phase-N'den
devam ediyorum, doğru mu?"

**Tüm phase'ler bitmişse** döngüyü atla, doğrudan Kapanış'a geç — `teslim.md`
eksikse orada derlenir.

**0.5 Muhakeme kontrolü** (yalnız hiç başlanmamış sette). Set `discussion.md`
içeriyor ama hiçbir dosyada `## Muhakeme` bölümü yoksa, koda dökmeden önce
`/plan-review {set}` öner. Kullanıcı istemezse zorlamadan devam et. Set
yarıda/bitmişse bu kontrolü **atla** — iş ortasında muhakeme önerisi gürültüdür.

**0.6 Todo kur.** `TodoWrite` ile resume noktasından itibaren phase başına bir
todo. Kalıcı kayıt `plan.md ## Durum` + phase checklist'leridir; todo yalnız
oturum-içi göstergedir.

## Phase döngüsü (sırayla, resume noktasından)

**1. Uygula.** YALNIZ `plan.md` + bu phase dosyasını referans al — geçmiş
phase'lerin ayrıntısını değil; onların özeti kendi `## Uygulama Notları`
bölümlerinde yazılıdır. Kılavuzu birebir izle.

**2. Test-first (opt-in).** Phase doğrulanabilir bir davranış üretiyorsa
(checklist'te `Test: {senaryo}` maddesi varsa): önce testi yaz, **fail ettiğini
doğrula**, sonra implementasyona geç. Salt refactor phase'lerinde atla.

**3. İşaretle ve sapmaları kaydet.** Biten her checklist maddesini `[ ]`→`[x]`
yap, todo'yu güncelle. İlk varsayımdan **sapan her şeyi** phase dosyasının
`## Uygulama Notları` bölümüne yaz — teslim.md ve gelecekteki okuyucu oradan
okur.

**4. Doğrula.** `.claude/is-akisi/proje.md` → "Doğrulama" bölümündeki komutları
koş; **geçmeli**. Hangi komutun ne zaman gerektiği orada tanımlıdır (shader
derlemesi, terminfo ve yarış sınaması gibi koşullu komutlar dahil) ve burada tekrarlanmaz.

**Ölçüm bir kapı değildir** — kare süresi, gecikme ve bench ölçümleri phase'i bloke etmez.
Değişiklik bir kare/gecikme/bellek iddiası taşıyorsa sayı **uydurma**: phase'in
`## Yayın Etkisi` bloğuna "ölçüm bekliyor: {ne}" yaz ve devam et. Kullanıcı
`/measure` ile koşturur.

**5. Kalite kapısı.** `proje.md` → "Kalite kapısı": `/simplify` → `/code-review`
→ `/audit`, sıra önemli. **Kapıyı sen koşturursun, `Skill` aracıyla** —
bunlar kullanıcının yazması gereken komutlar değildir. Çağrı düşerse
`proje.md`'deki iniş sırasını izle (subagent → kullanıcıdan iste → en son
waive); kendi elinle yapıp "yapıldı" yazma.

Gideremediğin ya da kapsam dışı bir **bulguyu** phase dosyasına gerekçesiyle
waive olarak yaz, sonra ilerle. Kapının **kendisi** koşmadıysa bu bir waive
değildir; `[~]` işaretlenir ve adım 7'de kullanıcıya söylenir.

Checklist'te kutuları işaretle: koşan kapı `[x]`, atlanan kapı `[~]` +
gerekçe. **Kutuları silme** — silinen kutu adım 8'in taramasında görünmez:
kapı hiç var olmamış gibi olur ve atlandığı hiçbir yerde iz bırakmaz. Subagent fan-out'larında model seçimi için
[references/otonom-serit.md](references/otonom-serit.md) → "Model katmanlaması".

**6. Commit (phase = tek commit).** Türkçe, emir kipinde tek satırlık özet;
gövdede ne değişti ve yayın etkisi. Hash'i **iki yere** yaz: phase
checklist'inin `Commit:` satırı **ve** `plan.md ## Durum` tablosunda phase'i
`✅` yap. Bir sonraki `/implement` çağrısının resume sinyali budur.
**Push etme** — push `/ship` kararıdır.

İlk phase'in commit'inden sonra `.tasks/README.md`'de setin satırını
**🔨 devam** yap.

**7. Onay kapısı.** Sonraki phase'e geçmeden dur: değişen dosyalar, commit
hash'i ve sapmaların özetini ver, onay bekle. Özet **her seferinde** ayrı bir
"atlanan kapılar" satırı taşır (`[~]` işaretli her madde, gerekçesiyle) —
"geçen phase'de söylemiştim" geçerli değil; bir kez söylenip sessizce
tekrarlanan atlama, atlamanın gizlenmesidir. Son phase'de bu kapı yerine
Kapanış'a geç.

Onay sonrası iki yol: aynı oturumda devam (küçük setlerde pratik) ya da
**taze bağlam (önerilen)**: `/clear`, sonra yeniden `/implement {set}`. Bu
güvenlidir çünkü adım 6 her şeyi diske yazdı; ön uçuş `## Durum`'dan devam
noktasını bulur. `/compact` **kullanma** — kayıplı özet, dosyalardaki kanonik
kayıtla yarışan ikinci bir kaynak olur ve düzeltilmiş varsayımların eski hâlini
taşıyabilir. (Yalnız phase **ortasında** kesinti gerekiyorsa anlamlıdır.)

## Kapanış (son phase sonrası, bir kez)

**8. Eksik-checklist kapısı.** Bütün phase'lerin checklist'lerini tara. İki
şeye birden bak — işaretsiz kutu **ve eksik kutu**:

- İşaretlenmemiş (`[ ]`) kutu varsa ya tamamla ya da kullanıcıyla **bilinçli
  waive** olarak onayla (nedenini phase dosyasına yaz, `[~]` işaretle).
- `[~]` işaretli kutuları **say ve raporla** — bunlar bitmiş iş değil,
  bilerek atlanmış kapıdır. Devir özetinde phase'iyle ve gerekçesiyle
  listelenir; `[x]` gibi sessizce geçilmez.
- `## Checklist` bölümü olan bir phase'de kalite kapısı satırları
  (`/simplify`, `/code-review`, doğrulama, commit) **hiç yoksa** bu bir waive
  değil, bir kayıptır: kapı ya koşmamıştır ya da izi silinmiştir. Şablondan
  eksik olan satırları geri koy ve o phase için kapının koşup koşmadığını
  kullanıcıya sor — varsayma. Başka bir phase'e devredilmiş
kutu (`→ phase-2b'de takip`) zaten bilinçli waive'dir, tamamlamaya kalkma.

**9. teslim.md derle.** `.tasks/{set}/teslim.md` varsa güncelle, yoksa
şablondan üret. Phase'lerin `## Yayın Etkisi` bloklarını topla:

- "yok" bloklarını **atla**. Hiç dolu blok yoksa **no-op teslim.md** üret:
  "türetilmiş dosya/ölçüm/belge etkisi yok; doğrulama + `/ship` yeterli".
- **Çelişki kuralı:** bir Yayın Etkisi bloğu aynı phase'in `## Uygulama
  Notları` sapma notlarıyla çelişirse **Uygulama Notları kazanır** — bloklar
  phase yazılırken tahmin edilmiş olabilir, notlar implementasyondan gelir.
  Dosya adlarını, sayıları, ölçümleri çapraz oku.
- `git log` ile tara: Yayın Etkisi'ne düşmemiş ama etkisi olan phase-dışı
  commit varsa ekle.
- Her B adımını şeritle etiketle (`[oto]` / `[komut]` / `[elle]` — biçim
  şablonda).
- Set **zaten teslim edilmişse** (commit'ler push'lu) teslim.md'yi "yapıldı"
  diye belgele, "yapılacak" gibi değil.

**10. İndeks.** `.tasks/README.md`'de setin notunu güncelle ("N phase tamam;
teslim bekliyor"). 🟢 işareti `/ship`'in işidir, burada verme.

**11. Devir.** Kapanış özeti: hayata geçirilen phase'ler + commit hash'leri,
`## Uygulama Notları`'na düşen kayda değer sapmalar, waive'ler, ölçüm
değiştiyse yeni değer ve `docs/OLCUMLER.md`'ye işlenip işlenmediği. Net
yönlendirme: **"Teslim için `/ship` çalıştır"**. teslim.md'de bekleyen
`[komut]`/`[elle]` adımı varsa birlikte yürütmeyi teklif et.
