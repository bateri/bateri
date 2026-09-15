---
name: audit
description: Değişen kodu bateri'ye özgü mercekler için denetler — make denetim'in mekanik yarısının üstüne bağımlılık kararının kaydı, ayar şeması, ölçüm sahipliği, thread/blokaj, boşta sıfır kare ve animasyon durma koşulu, hücre boyutu ve shader/Rust düzen uyumu, belge ve dil kuralı. Kullanıcı "denetle", "kurallara uyuyor mu", "mimari bozuldu mu" dediğinde ve /implement'in set sonundaki kalite kapısında /code-review'dan SONRA koşar. Genel hata avı değildir.
allowed-tools: Read, Glob, Grep, Agent, Bash(git:*), Bash(cargo:*), Bash(make:*), Bash(ls:*), Bash(grep:*)
---

Değişen kodu **bu depoya özgü** kurallar için denetle. `/code-review` hata
avlar ve projeye özgü kural okumaz; bu skill yalnız o boşluğa bakar.

Kuralların gerekçesi `CLAUDE.md`'dedir ve burada tekrarlanmaz; aşağıdakiler o
kuralların **kontrol edilebilir hâlleridir**.

## Girdi

`$ARGUMENTS` bir hedef veriyorsa (commit aralığı, dosya, `.tasks/{set}`) onu
al; set verildiyse aralık `duzen.md` → Set aralığı. Vermiyorsa çalışma
ağacındaki değişiklik (`git diff HEAD`); o da yoksa son commit.

## Kurgu

**1. Mekanik yarı: `make denetim`.** Katman yönü ve platformsuzluk, `bt-core`'da
gerekçesiz panik yolu, rc dosyasına yazan shell entegrasyonu ve bağımlılık
uyarısı `Makefile`'dadır ve `make hepsi` onu her phase'de zaten koşar. Burada
yeniden grep'leme; `make denetim`'i bir kez koş ve sonucunu rapora al.

**2. Eleme.** `git diff --name-only {hedef}` ile aşağıdaki merceklerden hangisinin
ilgili olduğunu belirle; değişmemiş alana bakan mercek düşer ve raporda
**"ilgisiz"** diye sayılır — sessizce düşen mercek "denetlendi" gibi okunur.

**3. İnline koş.** Mercekleri kendin koş. Fan-out yalnız üç ya da daha fazla
yargı merceği (4–7) ilgiliyse **ve** diff büyükse (birkaç yüz satırı aşıyorsa)
anlamlıdır: her mercek için bir `Agent`, paralel, `model: 'opus'`, istemde
otonom şeridin Ajan kuralları; ajana yalnız kendi merceğini ve ilgili diff'i
ver. Sentez ana döngüdedir, devredilmez.

## Mercekler

Her mercek için: bulgu varsa `dosya:satır` + neden ihlal + ne yapılmalı. Bulgu
yoksa tek satır "temiz".

**1. Bağımlılık kararı.** `make denetim` `Cargo.toml`/`Cargo.lock` uyarısı
verdiyse: kararın kaydı (`discussion.md` → `## Karar` ya da phase notu) var mı?
Yoksa bulgudur ve kullanıcıya sorulur — dış bağımlılık mimari karardır.

**2. Ayar ve tema şeması.** `settings.rs` ya da tema modeli değiştiyse: yeni
anahtarın varsayılanı, eski anahtarın akıbeti (silinmez), `docs/AYARLAR.md`,
yeniden yazma yolunun **bilinmeyen anahtarı koruduğu** round-trip sınaması,
İngilizce `snake_case` adlar. Shell dosyası değiştiyse üç kabuk da (zsh, bash,
fish) diff'te mi; değilse gerekçesi Uygulama Notları'nda mı.

**3. Ölçüm sahipliği.** Diff'te ölçüm sayısı taşıyan belge satırı ya da
**ölçülmemiş iddia** ("120 fps tutar", "gecikme düşer") var mı? Tek sahip
`docs/OLCUMLER.md`. İstisnalar: `docs/ARASTIRMA.md` (Metalterm'in sayıları) ve
bir `const`'un doc'undaki türetme (koşu tablosu değil). `Measured`'ın doc'undaki
sayılar istisna değil **emanettir**: kare süresi ve açılışın ilk ölçümü onları
`docs/OLCUMLER.md`'ye taşır.

**4. Thread ve blokaj.** Render yolunda (kare üreten kod, display link
callback'i) bloklayan çağrı var mı — PTY `read`, kilit bekleme, `sleep`, dosya
G/Ç? AppKit çağrıları `MainThreadMarker` taşıyor mu? PTY okuyucu ile renderer
arasındaki paylaşılan durumda iki kilit sırası kilitlenme üretebilir mi?

**5. Boşta sıfır kare ve animasyon durma.** Yeni animasyon ya da zamanlayıcının
**durma koşulu** nerede? Kirli satır olmadan kare talebi var mı? Belirti
sessizdir: uygulama çalışır, pil gider.

**6. Hücre boyutu ve shader/Rust düzen uyumu.** `Cell` değiştiyse `const`
assert güncel ve gerekçeli mi, alan yan tabloya mı gitmeliydi? `.metal`
struct'ı değiştiyse Rust `#[repr(C)]` karşılığı alan sırası, tip ve hizalama
ile aynı mı (`float3`'ün 16 bayt hizası)? Attribute indeksleri eşleşiyor mu?

**7. Belge ve dil.** Yeni crate'in `lib.rs` başlık yorumu var mı? Yorumlar
"neden"i mi anlatıyor? Yorumlar Türkçe, **kod tanımlayıcılarının tamamı
İngilizce** mi (`build.rs` dahil)? Türkçe kalan üç öbek yerinde mi (tanı metni,
`Makefile` hedefleri, jeton satırının anahtarları) ve jeton **değerleri**
İngilizce mi (`CLAUDE.md` → Dil)? `#[allow]` gerekçeli mi?

## Çıktı

Önce `make denetim` sonucu, sonra bulgular önem sırasıyla: `dosya:satır` ·
mercek · tek cümle ihlal · önerilen düzeltme. Bulgu yoksa hangi merceklerin
temiz, hangilerinin ilgisiz olduğunu tek satırda say.

Bulgu **uydurma**: zorlama bulgu, gerçek bulguyu gömer.
