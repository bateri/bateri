# Phase 1 — OSC 133 tarayıcısı ve oturum durumu

## Özet

Kabuğun bastığı işaretleri tanıyan **saf** bir tarayıcı ve onun doldurduğu
oturum durumu; henüz kimse beslemiyor, kimse okumuyor.

_Requirements: R2.1, R2.2, R2.3, R2.4_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** (ya da yeni bir `bt-core` modülü) — üç
  parça:
  - **Tarayıcı**: bayt dilimleri alır, aralarında durum taşır, OSC 133'ün
    `A`/`B`/`C`/`D` işaretlerini çıkarır. `ESC ]` ile başlar, `BEL` ya da
    `ESC \` ile biter; **bölünmüş dizi** iki `read()` arasında hayatta kalır.
    Taşıma tamponunun bir **üst sınırı** var: sınırı aşan dizi düşürülür ve
    tarayıcı boşa döner — bozuk ya da kötü niyetli bir akış belleği büyütemez.
    Tanımadığı OSC'yi ve bozuk yükü **yoksayar**; `bt-core`'da panik yok.
  - **Durum tipi** (`ShellState`): prompt'ta mıyız, komut mu koşuyor, sonuncusu
    hangi kodla bitti. **Kabuk adı geçmez** — tip zsh'i de bash'i de bilmez
    (R2.4). Seviye `enum`'u **yok**: durumun yokluğu "entegrasyon yok" demek
    (`discussion.md` → Karar 3).
  - **Yuva ve sorgu**: durum `Adapter`'ın yaprak kilidinde durur,
    `Session::shell_state()` onu kopyalayarak verir. Emsal `Session::theme()`:
    `Term` kilidine girmez, `frame()` imzasına dokunmaz.

## Kabul

- Tarayıcı birim sınamalarıyla kapanıyor: dört işaret, `D`'nin çıkış kodu,
  **chunk sınırında ikiye bölünmüş** dizi, iki sonlandırıcı (`BEL` / `ESC \`),
  tanınmayan alt-işaret, bozuk yük, üst sınırı aşan dizi.
- Tarayıcı `Session`'a geri girmiyor ve **hiçbir kare istemiyor** — bu bir
  kural değil, tipin şekli: tarayıcının elinde ne `Wake` ne `Session` var.
- `Session::shell_state()` bugün her koşuda "durum yok" diyor (besleyen yok) ve
  bunu bir sınama çiviliyor.
- `frame()` imzası değişmedi; `bt-gpu`'nun kare sınamaları dokunulmadan geçiyor.

## Yayın Etkisi

shader yok · terminfo yok · ayar şeması yok · tema yok · shell entegrasyonu
yok (betik sonraki phase'lerde) · app bundle yok · yeni bağımlılık yok.

`bt-core`'un platformsuzluğu korunuyor: tarayıcı saf bayt işi, `libc` bile
görmüyor.

## Uygulama Notları

- **Sonlandırıcı iki değil dört.** `vte-0.15.0`'ın `advance_osc_string`'i
  diziyi `BEL`, `CAN` (0x18), `SUB` (0x1A) **ve çıplak `ESC`** ile bitiriyor;
  sonuncusunda `ESC \`'in `\`'ini beklemeden dağıtıyor. Aynı kaynaktan ikinci
  bir parite kuralı: dizinin içindeki C0 baytları (0x00–0x06, 0x08–0x17, 0x19,
  0x1C–0x1F) yüke **girmiyor**. İkisi de tarayıcıya girdi ve sınandı —
  çerçeveleme ızgarayla aynı olmasaydı iki taraf aynı akıştan iki farklı hikâye
  okurdu.
- **Üst sınır yalnız `133` gövdesine uygulanıyor.** Numara kararı `;`'da
  veriliyor; `133` olmayan dizi tampona hiç dokunmadan atlanıyor. Aksi hâlde
  her meşru OSC 52 kopyası (kilobayt, megabayt) "sınırı aşan dizi" yoluna
  düşerdi ve sınırın ayırt ettiği bir şey kalmazdı. Sınır 256 bayt ve
  **tasarım sabiti**, ölçüm değil (gerekçesi `PAYLOAD_LIMIT`'in doc'unda).
- **`D;abc` işareti düşürmüyor, yalnız kodu bilinmiyor.** Plan "bozuk yük
  yoksayılır" diyor; yoksayılan, tanınmayan **işaret** (`133;Z`, `133;AB`,
  boş yük). Okunamayan bir parametre komutun bittiği bilgisini çürütmez ve
  düşürseydik durum sonsuza kadar "çalışıyor"da asılı kalırdı.
- **Yuva `AdapterInner`'da değil `Session`'da.** Phase "`Adapter`'ın yaprak
  kilidinde" diyordu; `Adapter` alacritty'nin **olaylarını** karşılıyor ve bu
  duruma hiç dokunmuyor — işaretler olaydan değil ham bayttan geliyor. Kilit
  yine yaprak, `Arc` çünkü phase-2 onu okuyucu thread'ine taşıyacak.
- **`ShellPhase` dört değerli** (`Prompt`/`Input`/`Running`/`Finished`), yani
  `A` ile `B` ayrı: "prompt çiziliyor" ile "kullanıcı yazıyor" sınırı Input
  Dock'un ilk sorusu ve ayrımı şimdi tutmak bedava.
- **`#![allow(dead_code)]`** modülün başında: tarayıcıyı besleyen taraf
  phase-2'de iniyor, bugün tek çağıranı sınamalar. **Satır phase-2'de kalkar.**
- **Test-first sırası bozuldu:** sınamalar ve gövde birlikte yazıldı. Yerine
  daha güçlü bir kanıt kondu — üç mutasyon (çıplak `ESC` sonlandırıcısı
  kaldırıldı, üst sınır kaldırıldı, `133` kararı erken verilmedi) tek tek
  denendi ve her biri kendi sınamasını kırmızıya düşürdü.

## Checklist

- [x] Tarayıcı: durum makinesi, bölünmüş dizi, üst sınır
- [x] `ShellState` + yaprak kilitteki yuva + `Session::shell_state()`
- [x] Test: dört işaret, çıkış kodu, bölünmüş dizi (her bayt sınırında), iki
      sonlandırıcı, bozuk yük, üst sınır aşımı, "besleyen yokken durum yok"
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı
