# Kutu çizim — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md)

Blok elemanları (U+2580–U+259F), Braille (U+2800–U+28FF) ve çizgi çizim
(U+2500–U+257F, **köşegenler `╱╲╳` hariç**) artık fonttan gelmiyor: kapı
`Atlas::slot`'ta, `Sprite::Rule` kolunun ikizi olarak ve **yedekten önce**
duruyor, geometri hücre ölçüsünden hesaplanıyor. Kullanıcının bildirdiği iki
kusur kapandı (Claude Code'un maskotu ve spinner'ı) ve TUI çerçeveleri
komşusuyla döşüyor. `bt-gpu` ile `bt-core` **hiç değişmedi** — `Sprite::Char(ch)`
çağrısı aynı, yuva aritmetiği aynı; sınır da ayar dosyası da genişlemedi.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make duman
```

### Beklenen çıktı

- `make hepsi`: `denetim: temiz`, clippy uyarısız, `bt-atlas` 57 sınama yeşil.
- `make duman`: jetonlar **birebir** phase-1'in satırı — `hucre=8 glif=6
  kural=15 yuva=13/1984`. Duman betiği donmuş ve bu setin karakterlerine hiç
  dokunmuyor, yani sayılar oynarsa sebebi bu set değildir. (`kare` ile `istek`
  koşudan koşuya oynar, sözleşmenin sabit kısmı değiller.)
- Ölçüm değişmedi: bu set `docs/OLCUMLER.md`'ye sayı yazmıyor.

### Doğrulama Checklist

- [x] `make hepsi` yeşil (phase-1, phase-2 ve kapı commit'lerinde ayrı ayrı)
- [x] `make duman` yeşil, jetonlar birebir aynı
- [x] Gözle kontrol: örnek sayfa, Claude Code'un maskotu ve spinner'ı
      (phase-1), 125 çizgi karakteri + altı çerçeve + kesikli aile (phase-2)
      — kullanıcı onayladı (2026-09-21)
- [x] Set kapısı: `/code-review` + `/audit` koştu, bulgular tek commit'te

## B. Yayın (doğrulamadan SONRA)

Belge etkisinin tamamı **kod commit'lerinin içinde** indi (`CLAUDE.md`'nin
kutu çizim paragrafı, `bt-atlas` başlık yorumu, `Atlas` ve `RuleKind`
doc'ları, `docs/YOL-HARITASI.md`'nin borç maddesi). Ayar şeması, tema biçimi,
shell entegrasyonu, `.metal`, `Cargo.lock` ve app bundle **değişmedi**; yeni
bağımlılık yok. Geriye tek bir adım kalıyor.

### B.1 Yuva ayak izini ve doyma eşiğini ölç `[komut]`

```
/measure
```

İki iddia bekliyor ve ikisi de bu setin doğurduğu:

- **Yuva ayak izi.** Yordamsal aile **413** yuva tutuyor (32 blok + 125 çizgi
  + 256 Braille); Braille'in 256'sı bu setten önce **sıfır** harcıyordu
  (genişlik kapısından dönüp negatif önbellekte `TOFU`'ya bağlanıyordu).
  Ölçüsü `Atlas::occupancy`, panelin aynı yöntemle bulduğu taban 160.
  **2026-09-21'de sayı 421 oldu:** setin dışında, tek commit'lik bir
  düzeltme terminalin grafik kümesini (U+23B8–U+23BF, 8 sprite) aynı kapıya
  aldı — `⎿` kutu çıkıyordu. Altısı zaten fonttan yuva harcıyordu, yani
  tavana gerçek katkı 2. Ölçüm bu sayıyla koşar.
- **Puntoya göre doyma eşiği.** Kapasite `floor(1024/w) * floor(1024/h)` ve
  puntoyla düşüyor (13pt@2x'te 1984, 32pt@2x'te 338). Hangi puntoda ailenin
  kendisi kapasiteyi aştığı ölçülmeli — tahliye borcunun önceliğini o sayı
  belirler (`docs/YOL-HARITASI.md` → "Atlas dolunca geri dönüşü yok").

**Kare süresi iddiası yok** ve bu bilerek: çizim yuva başına ömürde bir kez
koşuyor (`draw_rule` emsali), kare başına yeni iş doğmuyor.

### Yayın Checklist

- [ ] `/measure` — yuva ayak izi (421 vs 160) ve puntoya göre doyma eşiği
      `docs/OLCUMLER.md`'ye işlendi

## Açık kalemler

Set kapısının kayda geçip **düzeltmediği** bulgular; hiçbiri bu setin
kapsamında değil ve üçü de gerekçeli:

- **Atlas dolunca geri dönüşü yok.** 021 eşiği yaklaştırdı: Retina'da ~27pt
  civarında kullanılabilir kapasite ailenin kendi boyuna iniyor ve dolan
  atlasta **her** yeni karakter kalıcı olarak kutu çıkıyor. Kapsam kararı
  bilinçliydi (`discussion.md` → Kapsam dışı: yuva rezervi), çaresi tahliye
  ve borç yol haritasına yazıldı.
- **Yuva rasterizasyonu bütün hücreyi tarıyor.** `rect` boş satırı atlıyor
  ama her satırda bütün sütunları yürüyor; kazanç mekanik (döngü sınırlarını
  dikdörtgene kırpmak). Yuva başına ömürde bir kez koşuyor ve **ölçülmedi**,
  o yüzden bir iddia değil bir not. (Kapının "yay bütün hücrede `hypot`
  hesaplıyor" iddiası yanlış: çeyrek kısıtı `hypot`'tan önce eliyor.)
- **Köşegen deliği bir sınamayı taşıyor.** `face_fallback_is_cached_under_the_
  requested_face`'in fikstürü (`╱`) kapsamın dışında bırakılan üç köşegenin
  içinde yaşıyor, yani köşegenleri yordamsal çizmeye karar veren gelecek bir
  set önce o sınamaya yeni bir prob bulmak zorunda — ya da sentetik bir yüz
  kurup fikstürü fonttan koparmak. Bağ iki yerde adıyla yazılı
  (`raster::family`'nin köşegen kolu, `the_diagonals_stay_out_of_scope`).
- **Gölge kolu tamponu iki kez yazıyor.** `draw_procedural` sıfırlıyor,
  `block`'un `░▒▓` kolu hemen üstüne düz değer basıyor. Düzeltilmedi:
  sıfırlama bütün kolların **ortak** ön koşulu ve onu kollara dağıtmak tek
  bir memset için kuralın dört kopyasını doğururdu.

## Geri Alma

Tek bileşen, tek yön: `raster::is_procedural` kapsamdan çıkarılırsa bütün
aile bugünkü font yoluna geri döner (blok ve çizgi Menlo'dan gelir ve
döşemez, Braille genişlik kapısından dönüp tofu olur) — yani commit
revert'ünden başka bir adım yok.

- Kod: `git revert` (phase-1, phase-2 ve kapı commit'leri; `bt-gpu`/`bt-core`
  dokunulmadığı için sınırda değişiklik yok).
- Ayar şeması: **yok** — geri alınacak anahtar, korunacak eski değer yok.
- Belge: aynı commit'lerin içinde; revert `CLAUDE.md` paragrafını ve yol
  haritasının borç maddesini de eski hâline döndürür. Yol haritasına eklenen
  **atlas doyması** borcu revert'te kalmalı: o madde bu setin kodundan değil
  ölçümünden doğdu ve kod geri alınsa da doğru kalır.
