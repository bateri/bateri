---
name: audit
description: Değişen kodu bateri'ye özgü mercekler için denetler — katman yönü ve bt-core'un platformsuzluğu, PTY/ayrıştırma yolunda panik, boşta sıfır kare ve animasyon durma koşulu, hücre boyutu ve shader/Rust düzen uyumu, ayar şeması, shell entegrasyon üçlüsü, yeni bağımlılık, ölçüm sahipliği. Kullanıcı "denetle", "kurallara uyuyor mu", "mimari bozuldu mu" dediğinde ve /implement'in kalite kapısında /simplify ve /code-review'dan SONRA koşar. Genel hata avı değildir.
allowed-tools: Read, Glob, Grep, Agent, Bash(git:*), Bash(cargo:*), Bash(make:*), Bash(ls:*), Bash(grep:*)
---

Değişen kodu **bu depoya özgü** kurallar için denetle.

Sınır nettir: `/simplify` genel kod kalitesine bakar (reuse, sadeleştirme,
verimlilik), `/code-review` hata avlar. **İkisi de projeye özgü kural
okumaz.** Bu skill yalnız o boşluğa bakar — genel bug ya da stil arama, o
işler yapıldı.

Kuralların gerekçesi `CLAUDE.md`'dedir ve burada tekrarlanmaz; aşağıdakiler o
kuralların **kontrol edilebilir hâlleridir**.

## Girdi

`$ARGUMENTS` bir hedef veriyorsa (commit, dosya, `.tasks/{set}` phase'i) onu
al. Vermiyorsa çalışma ağacındaki değişiklik: `git diff HEAD` + staged. Hiç
değişiklik yoksa son commit'i denetle.

## Kurgu: önce ele, sonra dağıt

On merceğin çoğu her diff'te ilgisizdir; hepsini her seferinde koşturmak
israftır. Sıra şu:

**1. Eleme (inline, saniyeler).** `git diff --name-only` ile hangi merceğin
ilgili olduğunu belirle. Değişmemiş alana bakan mercek düşer — `Cargo.toml`
değişmediyse 2 sorulmaz, `assets/shell/` el değmediyse 5 sorulmaz, `.metal`
ve `bt-gpu` değişmediyse 8 ve 9 sorulmaz.

**2. Mekanik mercekler inline koşar.** 1, 2, 3, 4, 5, 6 esasen grep, `cargo
tree` ve dosya varlığıdır; çıkarım az, aktarım az. Bunlar için ajan kurma.

**3. Yargı mercekleri fan-out.** 7, 8, 9, 10 akıl yürütme ister (bir
animasyonun gerçekten durup durmadığını izlemek, bir thread'in render yolunu
bloklayıp bloklamadığını görmek, `#[repr(C)]` yapıyla `.metal` struct'ını alan
alan eşlemek). İlgili çıkanların sayısı **ikiden azsa inline** koş — tek mercek
için ajan kurmak da israftır. İkiden çoksa her mercek için bir `Agent` başlat,
paralel, `model: 'opus'` (yargı içerir;
`.claude/skills/implement/references/otonom-serit.md` → Model katmanlaması).
Her ajana yalnız kendi merceğini, ilgili diff'i ve `CLAUDE.md`'yi ver; aramayı
repoyla sınırla.

**4. Sentez ana döngüde — devredilmez.** Hangi bulgu gerçek, hangisi gürültü;
ajanlar spekülatif bulgu üretebilir. Ayıklamayı sen yap.

Elenen mercekleri raporda **"ilgisiz"** diye say: neyin bakılmadığı da
bilgidir, sessizce düşürülen mercek "denetlendi" gibi okunur.

## Mercekler

Her mercek için: bulgu varsa `dosya:satır` + neden ihlal + ne yapılmalı.
Bulgu yoksa tek satır "temiz" — mercek başına paragraf yazma.

Mekanik olanlar **1–6**; yargı isteyenler **7–10**.

**1. Katman yönü ve platformsuzluk.** Bağımlılık yukarı gitmemeli:
`bateri → bt-shell → bt-gpu → {bt-atlas, bt-core}`. `bt-core` hiçbir platform
kütüphanesi görmez; `bt-atlas` yalnız `core-text`/`core-graphics`.

```sh
cargo tree -p bt-core  -e normal | grep -E "objc2|core-text|core-graphics|metal"
cargo tree -p bt-atlas -e normal | grep -E "objc2"
cargo tree -p bt-gpu   -e normal | grep -E "bt-shell"
grep -rn "objc2\|core_text\|core_graphics" crates/bt-core/src | grep -v ":[[:space:]]*//"
```
(Son süzgeç yorum satırlarını düşürür: `bt-core`'un kendi başlık yorumu
"objc2 yok" der ve grep'i yanlış pozitife düşürür — 001 phase-1'de oldu.)
Hepsi boş dönmeli. `cargo tree` `Cargo.toml`'daki sözleşmeyi, `grep` kaynak
içindeki kaçağı görür — ikisi birden sorulur; `[cfg(target_os)]` arkasına
saklanmış bir `objc2` çağrısı `cargo tree`'de görünmeyebilir.

**2. Yeni bağımlılık.** `Cargo.toml` ya da `Cargo.lock` değişti mi? Dış
bağımlılık **mimari karardır**, kendiliğinden yapılmaz — bulgu olarak yaz ve
kullanıcıya sor. `Cargo.lock`'un tek başına değişmesi de bulgudur: ya bir
sürüm oynadı ya da bir feature bayrağı yeni crate çekti.

**3. Panik yolu.** PTY okuma ve ayrıştırma yolunda `unwrap`/`expect`/
`panic!`/indeksleme paniği olmamalı; bilinmeyen dizi yoksayılır ve loglanır.

```sh
git diff HEAD -U0 -- crates/bt-core/src | grep -E "^\+" | grep -E "\.unwrap\(\)|\.expect\(|panic!|unreachable!|\[[a-z_]+\]" | grep -v "// audit: "
```
Sınama modülleri (`#[cfg(test)]`) hariç. Bilinçli bir `unwrap` varsa yanına
`// audit: {neden güvenli}` yazılır; yorumsuz olan bulgudur.

**4. Ayar ve tema şeması.** `settings.rs` ya da tema modeli değişti mi?
Sorular: yeni anahtarın varsayılanı var mı; eski anahtar silindi mi (silinmez —
okunup uyarı verilir); `docs/AYARLAR.md` güncellendi mi; yeniden yazma yolu
**bilinmeyen anahtarı koruyor** mu (round-trip sınaması var mı). Anahtar
adları İngilizce ve `snake_case` mi.

**5. Shell entegrasyon üçlüsü.** `assets/shell/` altında bir kabuk dosyası
değiştiyse üçü de (zsh, bash, fish) diff'te mi? Değilse ya gerekçesi
`## Uygulama Notları`'nda yazar ya da bulgudur. Ayrıca: entegrasyon
kullanıcının rc dosyasına **yazıyor mu** (`>>`, `sed -i`, `~/.zshrc`) — yazıyorsa
kırmızı.

**6. Ölçüm sahipliği.** Diff'te ölçüm sayısı taşıyan belge satırı var mı?
Tek sahip `docs/OLCUMLER.md`. Başka belge (`CLAUDE.md`, `MIMARI.md`,
`README.md`, `.tasks/*`) sayıyı **tekrar etmez**, niteliksel anlatıp bağlanır.
Ayrıca: **ölçülmemiş iddia** var mı ("120 fps tutar", "gecikme düşer", "daha
az bellek")? Ölçülmediyse iddia edilmez — `/measure` ile ölçülür ya da cümle
düşer. `docs/ARASTIRMA.md` istisnadır: Metalterm'in kendi sayılarını aktarır,
bizim ölçümümüz değildir ve bilerek eskir.

**7. Thread ve blokaj.** Render yolunda (`bt-gpu`'nun frame üreten kodu,
display link callback'i) bloklayan çağrı var mı — PTY `read`, kilit bekleme,
`std::thread::sleep`, dosya G/Ç? AppKit çağrıları `MainThreadMarker` taşıyor
mu, yoksa "zaten ana thread'deyiz" varsayımı mı? PTY okuyucu ile renderer
arasındaki paylaşılan durum tek bir kilit altında mı, yoksa iki kilit sırası
kilitlenme (deadlock) üretebilir mi?

**8. Boşta sıfır kare ve animasyon durma.** Diff bir animasyon ya da zamanlayıcı
ekliyorsa **durma koşulu** nerede? İmleç yerleşince, decay bitince, sayaç
hedefe varınca frame talebi kesiliyor mu? "Her frame'de yeniden çiz" yolu
açık mı kaldı? Kirli satır olmadan `setNeedsDisplay`/display link talebi var
mı? Belirti sessizdir: uygulama çalışır, pil gider.

**9. Hücre boyutu ve shader/Rust düzen uyumu.** `Cell` yapısı değiştiyse
`const` boyut assert'i güncel mi ve gerekçesi yazıyor mu; yeni alan yan
tabloya mı gitmeliydi? `.metal` içindeki bir struct ya da uniform değiştiyse
Rust tarafındaki `#[repr(C)]` karşılığı **alan sırası, tip ve hizalama** ile
aynı mı (`float3`'ün 16 bayt hizası klasik tuzaktır)? Vertex/pipeline
descriptor'daki attribute indeksleri shader'la eşleşiyor mu?

**10. Belge ve üslup borcu.** Yeni crate `lib.rs` başında sözleşmesini anlatan
bir yorum aldı mı? Yeni yorumlar "ne" değil **"neden"** anlatıyor mu
(çevredeki yoğunlukta)? Yorumlar ve sınama iletileri Türkçe, UI dizgileri ve
ayar anahtarları İngilizce mi? `clippy` bastırmaları (`#[allow]`) gerekçeli mi?

## Çıktı

Bulguları önem sırasıyla ver. Her biri: `dosya:satır` · hangi mercek · tek
cümle ihlal · önerilen düzeltme. Bulgu yoksa hangi merceklerin temiz çıktığını
tek satırda say — "denetlendi" demek yetmez, neyin denetlendiği görünmeli.

Bulgu **uydurma**: mercek uygulanamıyorsa (ilgili dosya değişmemiş) o merceği
"ilgisiz" diye geç. Zorlama bulgu, gerçek bulguyu gömer.
