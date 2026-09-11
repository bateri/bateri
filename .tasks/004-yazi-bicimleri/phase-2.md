# Phase 2 — `bt-core`: biçim sınırdan geçer

## Özet

`frame()` sınırı kalın/eğik bayraklarını, beş alt çizgi çeşidini, üstü
çiziliyi ve SGR 58 rengini taşır; atlama koşulu kuralları da sorar. **Ekranda
hiçbir değişiklik olmaz** — `Frame::push` yeni alanları henüz okumuyor.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4, R3.5, R5.1, R5.2_

**Dikiş 003'ün kanıtlanmış deseni:** phase-4 sink'i genişletip renderer'a
glyph verisini yoksatarak almıştı ("görünmez, yeşil"). Aynısı: yeni alanlar
sınırdan geçer, `bt-gpu` onları phase-3'e kadar okumaz, `hucre=8 glif=6`
bit bit aynı kalır.

---

## 1. `UnderlineStyle` ve `Cell`'in yeni alanları

`crates/bt-core/src/session.rs`

```rust
/// Alt çizgi çeşidi — beşi birbirini dışlıyor.
///
/// Alacritty'nin `Flags`'i **yeniden ihraç edilmiyor** (003 R2.3): `bt-core`
/// alacritty'yi kapsüller, `pub` API'de alacritty tipi görünmez.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curl,
    Dotted,
    Dashed,
}

pub struct Cell {
    pub col: u16,
    pub row: u16,
    pub ch: Option<char>,
    pub fg: LinearRgba,
    pub bg: Option<LinearRgba>,
    /// Font yüzünü **`bt-gpu` türetir**; burada yalnız SGR bayrağı var.
    pub bold: bool,
    pub italic: bool,
    pub underline: UnderlineStyle,
    /// SGR 58; `None` → ön plan. `bg` ile birebir aynı örüntü.
    pub underline_color: Option<LinearRgba>,
    pub strikeout: bool,
}
```

`Cell` 5 alandan 10'a çıkıyor. Kopyalama maliyeti sorun değil — sink jenerik
(`impl FnMut(Cell)`) ve satır içine alınıyor. Ama "geçici kare-başı tip"
savunması ilk kez zorlanıyor: **`CLAUDE.md`'nin 24 baytlık `const` assert'i
bu tipe değil, alacritty'nin grid hücresine bağlı** (10 000 satır scrollback ×
sekme). Bu cümle koda yorum olarak girer, yoksa `/audit` haklı olarak "hücreye
alan eklendi, yan tablo neden olmadı" der.

> **`bt-gpu`'nun altı sınama literali kırılır.** `Cell { … }` kuran altı
> yer var (`renderer.rs` üç, `frame.rs` üç, hepsi `#[cfg(test)]`).
> `#[derive(Default)]` + `..Default::default()` gerekir.
> `#[non_exhaustive]` **çözüm değil**: `bt-gpu` o zaman `Cell`'i hiç kuramaz
> ve altı sınama birden ölür. Yani bu phase `bt-gpu` test koduna dokunur —
> katman ihlali değil, envanter kalemi.

---

## 2. Bayrak eşlemesi — **bu setin en sessiz tuzağı**

`crates/bt-core/src/session.rs`, `frame()` döngüsü

`Attr::Undercurl` önce `ALL_UNDERLINES`'ı **siliyor**, sonra yalnız
`UNDERCURL` ekliyor (`alacritty_terminal/src/term/mod.rs:1910-1913`). Yani
**kıvrımlı metinde `Flags::UNDERLINE` kapalıdır.** Refleksle
`contains(Flags::UNDERLINE)` yazılırsa bu setin varlık sebebi olan dalgalı
çizgi **hiç çizilmez** ve hiçbir sayaç bunu görmez.

```rust
// Beş bayrak AYRI AYRI sorulur ve kıvrımlı ÖNCE gelir. `UNDERCURL`
// `UNDERLINE`'ı İÇERMEZ — `Attr::Undercurl` ALL_UNDERLINES'ı silip yalnız
// kendini ekliyor. Tek bir `contains(UNDERLINE)` bu satırların hepsini
// sessizce düz çizgiye indirirdi.
let f = cell.flags;
let underline = if f.contains(Flags::UNDERCURL) {
    UnderlineStyle::Curl
} else if f.contains(Flags::DOUBLE_UNDERLINE) {
    UnderlineStyle::Double
} else if f.contains(Flags::DOTTED_UNDERLINE) {
    UnderlineStyle::Dotted
} else if f.contains(Flags::DASHED_UNDERLINE) {
    UnderlineStyle::Dashed
} else if f.contains(Flags::UNDERLINE) {
    UnderlineStyle::Single
} else {
    UnderlineStyle::None
};
```

`HIDDEN` kuralları da düşürür — `\e[8m` "mürekkep yok" demek; altı çizili
gizli metin çizgiyi gösterirse gizleme delinir:

```rust
let gizli = f.contains(Flags::HIDDEN);
let underline = if gizli { UnderlineStyle::None } else { underline };
let strikeout = !gizli && f.contains(Flags::STRIKEOUT);
```

> **Geniş karakterin ikinci hücresi bayrakları taşıyor** (alacritty şablondan
> kopyalıyor), yani kural iki hücreye kendiliğinden yayılıyor. Bedava, ama
> yoruma yazılır: sonradan "neden çalışıyor" sorusunun cevabı burası.

---

## 3. SGR 58 rengi

`crates/bt-core/src/session.rs`

```rust
// `underline_color()` `CellExtra`'da yaşıyor ve `Option<Color>` dönüyor;
// çözümü `bg`/`fg` ile aynı yoldan geçiyor, `pub` API'ye alacritty tipi
// sızmıyor. `None` → çizen taraf ön planı kullanır.
let underline_color = cell
    .underline_color()
    .map(|c| color::lineer_rgba(color::resolve(c, colors)));
```

---

## 4. Atlama koşulu genişler

`crates/bt-core/src/session.rs`

Boşluk `ch: None` **kalır**. 003 koda *"altı çizili bir boşluk `Some(' ')`
olacak ve tek dokunulacak satır aşağıdaki `then_some`"* diye not düşmüştü;
**karar bunun tersi** — `Some(' ')` atlasa bir boşluk glyph'i yükletir: yuva
harcar, hiçbir piksel boyamaz. Altı çizili boşluğun istediği bir kural.

```rust
if bg.is_none()
    && ch.is_none()
    && underline == UnderlineStyle::None
    && !strikeout
{
    continue;
}
```

**`MUREKKEPSIZ` bloğunun yorumu aynı commit'te düzeltilir** (kodla çelişen
cümle kuralı): "004'te değişecek olan tam burası … tek dokunulacak satır
aşağıdaki `then_some`" cümlesi artık yanlış.

---

## 5. Duman reçetesi ve stil sınaması

`crates/bt-core/src/session.rs` → `smoke_shell()`

`\033[4m` yalnız **düz** çizgidir (`vte/src/ansi.rs:1843` →
`[4, ..] => Attr::Underline`). Beş stili sınamayan bir reçete, kıvrım dalını
düz çizgiye düşüren bir kodu göremez.

```rust
"printf '\\033[41;1;4m bateri \\033[0m\\033[4m \\033[0;4:2m \\033[0;4:3m \\033[0;4:4m \\033[0;4:5m \\033[0;9m \\033[0;4:3;58;5;196m \\033[0m\\n'; sleep 10"
```

- Eklenen yedi hücrenin hepsi `bg: None, ch: None` → **`hucre=8` ve
  `glif=6` bit bit durur**, `sabit_shell_arka_plan_hucreleri_verir` ve
  `sabit_shell_alti_glif_verir` **oynamaz**.
- Yedisi de madde 4'ün yeni yan tümcesinden geçer — yani atlama koşulunun
  yeni hâli duman kapısından geçmiş olur.
- İlk sekiz hücre `BOLD` taşır: `Face` yolu da betikte var.
- Aradaki `\033[0;` **zorunlu**: `Attr::Strike` `ALL_UNDERLINES`'ı
  kaldırmıyor, sıfırlanmazsa o hücre kesikli **artı** üstü çizili olur.

> **İki nokta yük taşıyor.** `\033[4;3m` ≠ `\033[4:3m`. Noktalı virgüllü hâl
> `Underline + Italic`'tir (`[3] => Attr::Italic`); iki noktalı hâl undercurl.
> Reçeteyi "sadeleştiren" biri `:`'yı `;` yaparsa sınama sessizce
> düz-altı-çizili-eğik'e iner ve duman yeşil kalır. Bu uyarı `smoke_shell`'in
> doc'una girer.

`smoke_shell`'in doc'undaki sayı sözleşmesi (`hucre=8`, `glif=6`) korunur ve
yanına `kural=` sayısı eklenir — sahibi bu fonksiyon **ve** aşağıdaki sınama.

---

## Uygulama Notları

**0. Ad ve dil düzeltmesi (`SAPMA` değil).** Phase dosyası `bb04da7`'de, yani
`1dbb084`'ün dil daraltmasından önce yazıldı. Kod tanımlayıcılarının tamamı
İngilizce: `color::lineer_rgba` → `color::linear_rgba`, `let gizli` →
`let hidden`, ve checklist'in Türkçe sınama adları İngilizceye taşındı —
`kivrimli_metin_curl_verir` → `undercurl_text_yields_curl`,
`sabit_shell_bes_stili_ayirt_eder` → `smoke_shell_distinguishes_five_styles`,
`gizli_metnin_kurallari_da_duser` → `hidden_text_drops_rules_too`,
`alti_cizili_bosluk_hucresi_gecer` → `underlined_space_cell_passes_sink`.
"Değişmeden geçsin" denen ikisi zaten İngilizceydi
(`smoke_shell_yields_background_cells`, `smoke_shell_yields_six_glyphs`).

**1. `#[derive(Default)]` derlenmiyor; `Cell`'in `Default`'u elle yazıldı.**
`LinearRgba` `Default` taşımıyor ve taşımamalı: tek kurucusunun `from_srgb`
olması renk uzayını tipe bağlayan şeyin ta kendisi (`color.rs`'in newtype
gerekçesi). `impl Default for Cell` `fg`'yi **siyah** veriyor — anlamlı bir
varsayılan ön plan yok ve makul görünen bir varsayılan, unutulan bir alanı
ekranda sessizce yanlış yapardı. Doc'u kapsamı söylüyor: yalnız
`..Default::default()` ile kurulan sınama literalleri için; üretim yolunda
tek kurucu `frame()` ve orada on alanın onu da koşulsuz yazılıyor.

**2. `UnderlineStyle` `lib.rs`'ten yeniden ihraç edilmek zorundaydı.**
Checklist'te yoktu: `pub` bir alanın tipi, ihraç edilmezse
"private type in public interface". `lib.rs`'in modül doc'undaki görünür tip
listesi de aynı commit'te güncellendi.

**3. Eşleme zinciri atlama kapısının ÜSTÜNDE değil ALTINDA.** Phase metni
(madde 2 + madde 4) beş bayrağı kapıdan önce çözüp kapıda
`underline == None && !strikeout` soruyordu. İlk hâli öyle yazıldı;
`/simplify`'ın Altitude ve Efficiency mercekleri bağımsız olarak aynı şeyi
söyledi ve haklılar: `frame()`'in kendi disiplini "kapıda ucuz test, çözüm
kapıdan sonra" (aynı gerekçe `fg` ve `underline_color` için iki ayrı yorumda
yazılı), oysa zincir kapının üstünde **her görünür hücrede** yedi bayrak
okuyordu — kapının ihtiyacı tek soru: "hiç kural var mı".
Son hâl: `const RULES = ALL_UNDERLINES | STRIKEOUT`, kapıda
`let ruled = !hidden && flags.intersects(RULES)`, beş kollu zincir ve
`strikeout` kapıdan sonra. Davranış aynı, sınamalar aynı.
`!hidden` maskenin **içinde**: dışarıda kalsaydı gizli+altı çizili hücre
kapıdan geçip `sink`'e çizilecek hiçbir şeyi olmadan varırdı (mutasyonla
doğrulandı: `hidden_text_drops_rules_too` kırmızı düşüyor).
Aynı hamlede `NO_INK` `SPACERS`'a indi ve `HIDDEN` **tek bir `let`** oldu:
gizlilik hem mürekkebi hem kuralları düşürüyor, iki ifadede yaşarsa
ayrışabilirler.

**4. Sıra mutasyonu kırmızı düşmüyor — bekçi *sırayı* değil *düşürmeyi*
tutuyor.** Ölçüldü: `contains(UNDERLINE)` kolunu zincirin **başına** almak
üç sınamanın hiçbirini kırmıyor, çünkü alacritty beş bayrağı gerçekten
birbirini dışlar tutuyor (`Attr::*Underline`'ın beşi de önce
`ALL_UNDERLINES`'ı siliyor). Kırmızı düşen mutasyon asıl refleks: zinciri
tek bir `contains(UNDERLINE)`'a indirmek →
`undercurl_text_yields_curl` **ve** `smoke_shell_distinguishes_five_styles`
düşüyor. Kıvrımlı-önce sırası yine de korundu: bedava ve dışlayıcılık
alacritty'nin bize vermediği bir garanti. `hidden` kolunu silmek de
`hidden_text_drops_rules_too`'yu kırıyor (mutasyonla doğrulandı) — o sınama
uygulamadan önce yeşildi, çünkü atıl alanlar zaten `None`/`false`'tu; kanıtı
mutasyon, kırmızı-önce değil.

**5. Üç duman sınaması ortak `spawn_smoke()` kurucusuna geçti.**
Üçüncü kopya `SessionOptions` literalini üçe çıkarıyordu. İki var olan
sınamanın **iddiaları bit bit aynı** (8 ve 6 korundu); değişen yalnız
oturumun nasıl açıldığı. `spawn_session` da aynı `spawn_with_command`
gövdesine indi.

**6. Reçete Rust satır devamıyla yazıldı.** Tek satır 100 sütunu aşıyor ve
rustfmt dizgi literalini bölemiyor; `\` + satır sonu kaçışı sonundaki boşluğu
koruyup baştaki girintiyi yiyor, yani kabuğa giden komut tek satır.
Doğrulaması `smoke_shell_distinguishes_five_styles`'ın `cells.len() == 15`
iddiası: reçete bozulsa hücre sayısı tutmaz.

**7. `underline_color` atlama kapısından SONRA çözülüyor.** Kapının kural
yan tümcesi yalnız bayrak okuyor; `CellExtra`'ya inip palete bakmak
çizilmeyen hücreler için `Term` kilidi altında ödenirdi — ön planın kapı
sonrasına alınmasıyla birebir aynı gerekçe.

**8. Kodla çelişen dört yorum aynı commit'te düzeltildi.** `NO_INK` bloğunun
"004'te `Some(' ')` olacak" cümlesi (karar tersine çıktı), `Cell`'in
"biçim bayrakları geçmez … 004'ün işi" başlığı, ve `fg`'nin gelecek zamanlı
"004'ün kural çizgisi isteyecek" cümlesi, ve `Cell::ch`'in `Option<char>`
gerekçesindeki "bedeli 004'te ödenirdi: altı çizili bir boşluk **mürekkep
ister**" öngörüsü (dördüncüsünü `/simplify` buldu; karar tersine çıktığı için
gerekçe artık dört ayrık durumun *farklı şey istemesine* dayanıyor).
`smoke_shell`'in doc'u yedi kural hücresini, stil sırasını ve iki nokta
uyarısını kazandı; `fore`'un yorumundaki gelecek zaman da düzeldi.

### `/simplify` kaydı

Dört mercek (reuse, simplification, efficiency, altitude) paralel koştu.

**Uygulanan:** eşleme zinciri kapının altına indi ve kapı tek maske testine
düştü (not 3 — Altitude 1 + Efficiency 1, aynı bulgu iki mercekten);
`underline_color` da `ruled` kapısının arkasına alındı (Efficiency 3:
`extra` sıfır genişlikli birleşik karakter ve hyperlink için de dolu, yani
kuralsız hücrede `Arc` deref + palet çözümü boşa gidiyordu);
`spawn_with_command`'ın `Option` parametresi düştü — iki çağıranın ikisi de
`Some` veriyordu (üç mercek birden); beklenen renk vektörü
`vec![Option::None; 6].into_iter().chain([red])` yerine düz `vec!` literali
(iki kardeş assert zaten öyle); `Cell::ch`'in ölü 004 öngörüsü (not 8);
alacritty'nin "`Undercurl` `ALL_UNDERLINES`'ı siler" olgusunun üç kopyası
bire indi — tek yük taşıyan yer eşleme sitesi, `UnderlineStyle`'ın doc'u
tasarım sonucunu, sınama yorumu yakaladığı mutasyonu söylüyor.

**Reddedilen (uygulanmadı, waive değil — bulgu değil tercih):**
`undercurl_text_yields_curl` ve
`underlined_space_cell_passes_sink`'i silip duman sınamasına bırakmak
(Simplification 3) — ikisi de `plan.md`'nin onaylı checklist'inde ve
odaklı sınama duman reçetesi yeniden yazıldığında da ayakta kalıyor;
üç duman sınamasını tek PTY spawn'ına toplamak (Efficiency 4) — hata
yalıtımı sınama başına bir süreçten daha değerli; `underline_color`'ı
`Option` yerine düz `LinearRgba` yapıp yokluğunda `fg` yazmak
(Efficiency 2) — `plan.md` R3.5 "`None` → ön plan" diyor ve phase-3'ün
`underline_color ?? fg` satırı buna bağlı, `/simplify` bir plan kararını
çeviremez; `bt-gpu`'nun yeni alanları okumaması (Efficiency 2) — phase-3'ün
işi, bu setin dikişi tam olarak bu.

### `/code-review` kaydı

**2 bulgu, ikisi de düşük, ikisi de uygulandı; waive yok.** Bulguların
ikisi de `/simplify`'ın açtığı yüzeyde:

- **SGR 58 kapısı `ruled`'a bağlıydı ve yalnız üstü çizili bir hücreye renk
  taşıyordu.** `\e[9;58;5;196m` → `underline: None, strikeout: true,
  underline_color: Some(kırmızı)`; `plan.md` ve `phase-3.md` ise "üstü
  çizili **hep** `fg`" diyor, yani phase-3'ün çizicisi bu rengi okusa üstü
  çiziliyi kırmızıya boyardı. Kapı `underline != None`'a daraltıldı
  (adı "alt çizgi rengi" ve SGR'de üstü çizilinin ayrı rengi yok) ve
  `Cell::fg`'nin doc'u aynı ayrımı söyleyecek şekilde düzeltildi. Duman
  reçetesi bunu göremiyor — 9 ile 58 aynı hücrede buluşmuyor — bu yüzden
  `strikeout_only_cell_carries_no_underline_color` eklendi; mutasyonla
  doğrulandı (kapı `ruled`'a döndüğünde kırmızı).
- **`smoke_shell_distinguishes_five_styles`'ın bekleme ölçütü yapısal
  olarak kırılgandı.** `wait_cells(.., 8)` arka planlı hücre sayar, ama
  yedi kural hücresi sekiz arka planlı hücreden **sonra** geliyor: PTY
  okuması ikisinin arasında bölünürse sınama 8'i gören karede dönüp
  `cells.len() == 15`'te düşerdi. 15 koşuda görülmedi, ama kardeşi
  (`underlined_space_cell_passes_sink`) çapasını tam da bu yüzden sona
  koyuyor. `wait_cells` genel `wait_frame(session, wake, ready)` üstüne
  oturtuldu ve stil sınaması ölçütünü karenin tamamına bağladı.

Kayda değer ikinci yarısı: `/code-review` alacritty ve vte kaynaklarından
dört olguyu bağımsız doğruladı (beş alt çizgi bayrağının gerçekten birbirini
dışladığı, `BOLD_ITALIC`'in `BOLD|ITALIC` birleşimi olduğu için `contains`'in
doğru yanıtladığı, `4:2/4:3/4:4/4:5` eşlemesi, `58;5;196` → `Indexed(196)`)
ve iki yeni PTY sınamasını 15 kez koşturdu — flake yok.

### `/audit` kaydı

**İlgisiz mercekler (elendi, kayda geçiyor):** 4 (ayar/tema şeması — model
yok), 5 (shell üçlüsü — `assets/shell/` el değmedi), 7 (thread ve blokaj —
yeni thread, kilit, `sleep` ya da G/Ç yok; diff'teki `sleep`'ler sınama
betiklerinin kabuk komutu), 8 (boşta sıfır kare — yeni animasyon ya da
zamanlayıcı yok, `dirty` kapısı el değmedi; `frame()`'in gövdesi değişti ama
kare **talebi** değişmedi).

**Koşan mercekler:**

- **1 katman yönü ve platformsuzluk — temiz.** `cargo tree -p bt-core` yalnız
  `alacritty_terminal` ve onun ağacını veriyor; `objc2`/`core-text`/`metal`
  yok, kaynakta da grep boş. `bt-gpu` ağacında `bt-shell` yok.
  `UnderlineStyle` `bt-core`'un kendi tipi ve alacritty'nin `Flags`'i ile
  `Color`'ı `pub` API'ye çıkmıyor; `(bold, italic) → Face` çevirisi
  bilerek yazılmadı, phase-3'ün işi.
- **2 yeni bağımlılık — temiz.** `Cargo.toml` ve `Cargo.lock` el değmedi
  (`git diff HEAD --name-only` ikisini de göstermiyor).
- **3 panik yolu — temiz.** `bt-core`'a giren tek `expect` sınama
  modülünde (`underlined_space_cell_passes_sink`'in çapası); üretim yolunda
  yeni `unwrap`/`expect`/`panic!`/indeksleme yok.
- **6 ölçüm sahipliği — temiz.** Diff'te fps/gecikme/bellek iddiası yok.
  Geçen sayılar (`24 bayt`, `4 bayt`, `8`/`6`/`7`/`15` hücre) `size_of`
  gerçeği ve iş sayımı; `docs/OLCUMLER.md` yok ve oluşturulmadı. Sınır
  `Cell`'inin büyümesinin kare süresine etkisi "ölçüm bekliyor" olarak
  duruyor ve bu phase de sayı yazmadı.
- **9 hücre boyutu ve shader/Rust düzeni — bir düşük bulgu, devredildi.**
  24 baytlık `const` assert güncel ve hâlâ alacritty'nin **grid** hücresine
  bağlı; `.metal` el değmedi, `Instance`/`GlyphInstance` assert'leri
  oynamadı, `Frame::push` yeni alanları okumuyor. Bulgu: `CLAUDE.md`'nin
  hücre maddesi grid hücresi ile sınır hücresini ayıran yan tümceyi
  taşımıyor — **phase-3'e devredildi** ve oranın checklist'ine yazıldı
  (phase-3 zaten `CLAUDE.md`'ye dokunuyor).
- **10 belge ve üslup borcu — üç bulgu, üçü de uygulandı.** (a)
  `Frame::push`'un "burada sorulacak bir bayrak yok" yorumu phase-2'den
  sonra eksik kalıyordu: sorulacak alan **var**, yalnız phase-3'e ertelendi
  — dikiş notu kodun yanına yazıldı. (b) `Cell`'in doc'undaki "`BOLD`/
  `ITALIC` **ikişer** `bool`" üleştirme sayısıydı, yani dört `bool` diyordu;
  "iki ayrı `bool`" oldu. (c) Bu dosyanın "Yayın Etkisi → belge" listesi
  "dördü" deyip beş kalem sayıyordu ve not 8'in listesiyle örtüşmüyordu;
  ikisi tek sayıda buluşturuldu. Tanımlayıcıların tamamı İngilizce (beş yeni
  sınama adı dahil), yorumlar ve `assert!` gerekçeleri Türkçe, `#[allow]`
  yok, yeni UI dizgisi ya da ayar anahtarı yok.

## Yayın Etkisi

- **shader** — yok.
- **terminfo / `TERM`** — yok.
- **ayar şeması** — yok. SGR 58 var olan `color::resolve` yolundan geçiyor,
  yeni kullanıcı ayarı doğmuyor (`docs/AYARLAR.md` ve ayar modeli henüz yok).
- **tema / materyal** — yok.
- **shell entegrasyonu** — yok. `smoke_shell` bir **sınama** betiği,
  `assets/shell/` altındaki entegrasyon üçlüsü değil.
- **app bundle** — yok.
- **yeni bağımlılık** — yok; `Cargo.toml`/`Cargo.lock` el değmedi (doğrulandı:
  `git status` ikisini de göstermiyor).
- **belge** — öngörülenden geniş çıktı. **Kodla çelişen beş cümle**
  (not 8'in listesi) aynı commit'te düzeldi: `session.rs`'te `NO_INK`
  yorumu (`MUREKKEPSIZ`'in bugünkü adı; 003'ün "`Some(' ')` olacak"
  öngörüsü tersine çıktı), `Cell`'in başlık doc'u, `Cell::fg`'nin gelecek
  zamanlı cümlesi, `Cell::ch`'in ölü 004 öngörüsü, `fore`'un yorumundaki
  gelecek zaman. Ayrıca **üç ekleme**: `smoke_shell`'in doc'u (yedi kural
  hücresi, stil sırası, iki nokta uyarısı), `bt-core/src/lib.rs`'in modül
  doc'undaki görünür tip listesi (`UnderlineStyle`), ve `bt-gpu`'nun
  `Frame::push`'una "yeni alanlar bu phase'de okunmuyor" dikiş notu
  (`/audit` mercek 10'un bulgusu).
  Duman jetonu **bu phase'de eklenmiyor** (phase-3), o yüzden `CLAUDE.md`,
  `Makefile` ve `proje.md` el değmiyor — doğrulandı, `make duman`
  `kare=1 hucre=8 glif=6 pipeline=ok` ile bit bit aynı satırı basıyor.
  `CLAUDE.md`'nin hücre maddesine "sınır `Cell`'i ayrı kayıttır" yan
  tümcesi **phase-3'e devredildi** (`/audit` mercek 9; phase-3 zaten
  `CLAUDE.md`'ye dokunuyor, iki commit'te iki kez açmanın anlamı yok).
- **ölçüm bekliyor:** sınır `Cell`'inin büyümesinin kare süresine etkisi —
  003 `teslim.md` B.1 **#5**'in genişlemesi. `size_of` bir ölçüm değil
  gerçektir ve istenirse `const` assert'le bağlanır; **kare süresine
  etkisi** ölçüm bekler ve kancası (`BT_FRAME_LOG`) hâlâ yok.

---

## Checklist

- [x] `UnderlineStyle` enum'ı + `Cell`'in beş yeni alanı + `Default` (elle, bkz. not 1) + `lib.rs`'ten yeniden ihraç (not 2)
- [x] `Cell`'in doc'una "24 baytlık assert bu tipe değil, grid hücresine bağlı" notu
- [x] Bayrak eşlemesi: beş bayrak ayrı ayrı, **kıvrımlı önce** (madde 2)
- [x] `HIDDEN` kuralları da düşürüyor (zincirin ilk kolu, not 3)
- [x] SGR 58 → `Option<LinearRgba>` (kapıdan sonra çözülüyor, not 7)
- [x] Atlama koşulu genişledi; `NO_INK` yorumu düzeltildi (`MUREKKEPSIZ`'in bugünkü adı)
- [x] `smoke_shell` reçetesi + doc'una iki nokta uyarısı
- [x] `bt-gpu`'nun altı `Cell { … }` sınama literali `..Default::default()`'a geçti
- [x] Test: `undercurl_text_yields_curl` — `\033[4:3m` `UnderlineStyle::Curl` üretir (**`contains(UNDERLINE)` refleksi burada kırmızı düşer**; mutasyonla doğrulandı, not 4)
- [x] Test: `smoke_shell_distinguishes_five_styles` — 15 hücre, stil dizisi `[Single, Double, Curl, Dotted, Dashed]` + üstü çizili + SGR 58 rengi
- [x] Test: `hidden_text_drops_rules_too` — `\033[8;4:3;9m` kural üretmez
- [x] Test: `underlined_space_cell_passes_sink` — `bg: None, ch: None`, kural var → sink çağrılır
- [x] Test: `smoke_shell_yields_background_cells` ve `smoke_shell_yields_six_glyphs` **iddiaları bit bit aynı** geçiyor (8 ve 6 korundu; ortak kurucuya geçtiler, not 5)
- [x] Doğrulama geçti — **kapıdan sonra yeniden koşuldu** (kapı kodu değiştirdi, kapı öncesi yeşil geçersizdi):
  - `cargo fmt --all -- --check` → exit 0, `make hepsi` → exit 0
  - `make test-yaris` → exit 0 (**zorunluydu**: `frame()` gövdesi `Term` kilidi altında değişti)
  - `make duman` → exit 0, `kare=1 hucre=8 glif=6 pipeline=ok` — jeton bu phase'de eklenmiyor, sayılar bit bit korundu
  - `make shader` `[~]` — `.metal` ve `build.rs` el değmedi, tetiklenmedi
  - `make terminfo` `[~]` — `assets/terminfo` el değmedi (ve hedefin girdisi henüz yok)
  - `git status`: `Cargo.lock` **oynamadı**
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (kaydı yukarıda)
- [x] `/code-review` çalıştırıldı, bulgular giderildi (kaydı yukarıda; +1 sınama: `strikeout_only_cell_carries_no_underline_color`)
- [x] `/audit` çalıştırıldı, bulgular giderildi (kaydı yukarıda; mercek 9'un bulgusu phase-3'e devredildi ve oranın checklist'ine yazıldı)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı (öngörü doğrulandı: `Cargo.lock` oynamadı, `.metal`/`build.rs`/`assets` el değmedi)
- [x] Commit: 643c8c6
