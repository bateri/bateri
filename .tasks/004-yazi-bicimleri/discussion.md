# Yazı biçimleri — Tartışma

<!-- Biçim B (karar-listesi): birbirinden bağımsız birden çok karar noktası. -->

## Karar 1: Kapsam — emoji ve kutu çizim bu sete girsin mi?

Konu dört şeyi birden anıyor ama dördü **mimari olarak aynı iş değil**:

| iş | nerede yaşar | 003'ün bir öncülünü kırar mı |
|---|---|---|
| `BOLD` / `ITALIC` | atlas anahtarı + 4 `CTFont` | **hayır** — hücre boyutunda, `R8Unorm` |
| `UNDERLINE` / `STRIKEOUT` | yeni geometri + font metriği | **hayır** |
| emoji | BGRA doku, ikinci örnekleme yolu, **iki hücrelik glyph** | **evet, iki birden**: "tüm glyph'ler hücre boyutunda" ve tek `R8Unorm` doku |
| kutu çizim | yordamsal raster (Metalterm'de `boxdraw`) | hayır, ama yazı biçiminden tamamen bağımsız |

Emoji, 003 `teslim.md` B.3'teki **"geniş karakterin glyph'i tek yuvaya
kırpılıyor"** sınırıyla aynı problem: ikisi de "bir glyph bir hücreden geniş
olabilir" demek. Tek bir geniş-glyph seti ikisini birden çözer; emojiyi buraya
almak o seti yarım yapar.

Kutu çizim ise yordamsal çizim işi — fonttan hiç glyph almıyor, kendi
çiziyor. Yazı yüzleriyle tek ortak noktası atlasta yuva tüketmesi.

**Öneri: 004 = yüzler + kurallar.** Emoji ve geniş glyph ayrı bir sete, kutu
çizim ayrı bir sete.

> Bu seçilirse **iki** belge yanlışa düşer ve ikisi de seti kuran commit'te
> düzelir (aynı commit kuralı): `CLAUDE.md`'nin *"kalanı 004'ün işi
> (…emoji, kutu çizim)"* cümlesi **ve** `crates/bt-atlas/src/lib.rs:10-11`
> modül doc'unun aynı iddiası.

## Karar 2: Bayraklar sınırdan nasıl geçecek? → **iki tip, iki crate**

İlk taslak `Face` enum'ını `bt-core`'a koyup `bt-atlas`'ın önbellek anahtarı
yapıyordu. **Bu çalışmaz ve panel üç mercekten de yakaladı.**

`crates/bt-atlas/Cargo.toml` bağımlılıkları yalnız `objc2-core-foundation`,
`objc2-core-graphics`, `objc2-core-text` — **`bt-core` yok ve olmamalı**.
`bt-atlas`'a o kenarı eklemek, dört varyantlı bir enum uğruna
`alacritty_terminal`'i saf-CoreText crate'ine çekerdi; `CLAUDE.md` bunu
"bağımlılık mimari karardır" diye kapatıyor.

Doğru dikiş yeri **crate sınırı**:

```rust
// bt-core — terminal semantiği. Bayrak taşır, yüz değil.
pub struct Cell {
    pub col: u16, pub row: u16,
    pub ch: Option<char>,
    pub fg: LinearRgba, pub bg: Option<LinearRgba>,
    pub bold: bool, pub italic: bool,
    pub underline: UnderlineStyle,               // Karar 3
    pub underline_color: Option<LinearRgba>,     // SGR 58; None → ön plan
    pub strikeout: bool,
}

// bt-atlas — tipografi. CoreText trait'inin karşılığı.
pub enum Face { Regular, Bold, Italic, BoldItalic }

// bt-gpu — ikisini birden gören TEK katman; çeviri burada, dört kollu match.
let face = match (cell.bold, cell.italic) {
    (false, false) => Face::Regular, (true, false) => Face::Bold,
    (false, true) => Face::Italic,   (true, true)  => Face::BoldItalic,
};
```

**Bu ikilik plana gerekçesiyle yazılır, yoksa sonradan "birleştirilir".**
İki enum yan yana görüldüğünde refleks onları tek tipe indirmektir ve o
hamle katman yönünü ters çevirir. `bt_core::bold/italic` bir SGR bayrağıdır,
`bt_atlas::Face` bir font yüzüdür; dört varyantları aynı, **sebepleri ayrı**.

Ayrı bir `Rules` sarmalayıcı tipi yok: alanlar doğrudan `Cell`'de. (1. tur bunu "dört `bool`" diye yazmıştı; SGR 58 geri gelince alt çizgi bir enum'a ve bir renge dönüştü, gruplama kararı yeniden türetilmedi — gerekçe aynı: sarmalayıcı tip çağıran tarafta kod kazandırmıyor.)

**Bu bir "hücreye alan eklemek" değil.** `CLAUDE.md`'nin 24 baytlık `const`
assert'i alacritty'nin **grid hücresine** bağlı (10 000 satır scrollback ×
sekme). Buradaki `Cell` `frame()` sınırının **geçici** kare-başı tipi;
scrollback'te yaşamıyor, assert'e dokunulmuyor.

> Uygulama notu: `Cell`'e alan eklemek `bt-gpu`'daki **altı** `Cell { … }`
> sınama literalini kırar. `Default` + `..Default::default()` gerekir;
> `#[non_exhaustive]` **çözüm değil** — `bt-gpu` o zaman `Cell`'i hiç kuramaz.

## Karar 3: Hangi alt çizgi çeşitleri? → **beşi de + SGR 58 rengi**

<!-- 2026-09-11: kullanıcı kıvrımlı çizgiyi kapsama aldı; bu bölüm ve Karar 4
     o karara göre yeniden yazıldı. 1. tur muhakemesi bu yönü görmedi. -->

Alacritty beş çeşit alt çizgi taşıyor ve hepsi tek bayrak kümesinde
(`cell.rs:29-35`): `UNDERLINE`, `DOUBLE_UNDERLINE`, `UNDERCURL`,
`DOTTED_UNDERLINE`, `DASHED_UNDERLINE` — ayrıca `STRIKEOUT` ve
`cell.underline_color()`.

**Kıvrımlı çizgi kapsama alınınca beşini birden almak setin şeklini
bozmuyor, düzeltiyor.** Gerekçe: kıvrımlı çizgi zaten bir fragment shader'ı
zorunlu kılıyor (düz bir dikdörtgen değil, dalga). O shader var olduğunda
çift / noktalı / kesikli **aynı shader'ın desen dalları**: her biri birkaç
satır. "Yalnız kıvrımlı" demek, beş kardeşten dördünü keyfî olarak dışarıda
bırakmak ve `bt-core` sınırına yarım bir enum koymak olurdu.

SGR 58 (ayrı alt çizgi rengi) da **birlikte gelir**: kıvrımlı çizginin tek
gerçek tüketicisi dil sunucularıdır ve onlar rengi anlam taşımak için
kullanır — hata kırmızı, uyarı sarı. Renksiz undercurl yarım özelliktir.

```rust
// bt-core sınırı
pub enum UnderlineStyle { None, Single, Double, Curl, Dotted, Dashed }
// Cell: underline: UnderlineStyle,
//       underline_color: Option<LinearRgba>,   // SGR 58; None → ön plan
//       strikeout: bool,
```

**Kapsamda:** beş alt çizgi çeşidi + üstü çizili + SGR 58 rengi.
**Kapsam dışı, kayda geçer:** üstü çizili için ayrı renk (SGR'de yok).

## Karar 4: Kural çizgisi nasıl çizilir? → **atlasta sprite, var olan `cell` pipeline'ı**

<!-- 2026-09-11, 2. tur: iki kez yeniden yazıldı. (a) "cell_bg'yi yeniden
     kullan" → dalga fragment hesabı ister, düştü. (b) "yeni cell_rule.metal +
     üçüncü pipeline" → panel üçüncü bir yol gösterdi, o da düştü. -->

**Yeni shader yok, üçüncü pipeline yok.** `cell` pipeline'ı zaten genel bir
**kapsama maskesi çizicisi** ve bunu kodun kendi yorumu söylüyor
(`cell.metal:55-56`): *"Renk instance'tan gelir, dokudan değil — atlas glyph
başına bir maske tutuyor, bir görüntü değil."* Kıvrımlı bir alt çizgi de
hücre boyunda bir kapsama maskesinden ibaret.

`GlyphInstance`'ın `size`'ı yok (`frame.rs:41-48`), dörtlü **tam bir hücre** —
kural yuvası da tam bir hücre. Renk instance'tan geldiği için SGR 58 bedava.

Yani kural çizgileri **atlasta yuva tutan sprite'lar** olur:

```rust
// bt-atlas
pub enum RuleKind { Single, Double, Curl, Dotted, Dashed, Strike }
pub enum Sprite { Char(char), Rule(RuleKind) }
yuvalar: HashMap<(Sprite, Face), u16>   // kurallar Face::Regular'da yaşar
```

**Bu makine zaten kurulu ve sınanmış.** `raster.rs` alfa-only bir
`CGBitmapContext` kuruyor (renk uzayı yok, kapsama doğrudan alfa baytı) ve
`tofu_tamponu` **fontu hiç kullanmadan** yuvaya elle bir kutu çiziyor. Kural
sprite'ları o iki mekanizmanın ikinci müşterisi; yeni bir şey icat etmiyor.
`CLAUDE.md`'nin katman tablosu da `bt-atlas`'ı zaten *"glyph rasterizasyonu,
atlas paketleme, **kutu çizim karakterleri**"* ile yükümlü kılıyor — yordamsal
raster oraya yazılı, ve Karar 1'de ayrılan kutu çizim seti aynı makineyi
isteyecek.

**Bu rotanın düşürdükleri**, tek tek:

| pipeline rotası isterdi | sprite rotasında |
|---|---|
| `cell_rule.metal` + üçüncü pipeline | **yok** |
| `RuleInstance` + iki taraflı assert çifti | **yok** — `GlyphInstance` |
| `Frame::clear(cell_px, **rule_px**)` | **yok** — kural metriği `Frame`'e hiç girmiyor |
| `make shader` zorunlu kapı | **yok** — `.metal` el değmiyor, `[~]` |
| imleç sırası gerekçesi | **bedava** — glyph geçişi zaten arka planlardan ve imleçten sonra kodlanıyor (`renderer.rs:409-412`) |

Metrik sorunu da kökünden kalkıyor: kırpma `font.rs:127`'de, `cell_px`'i
bilen **tek** yerde yapılır ve `slot_bytes`'ın "yuva geometrisinin tek sahibi"
gerekçesine (`font.rs:42-45`) birebir oturur.

**Kısıt — plana yazılır:** dalganın (ve noktalı/kesikli desenin) periyodu
hücre genişliğini **tam bölmeli**, yoksa hücre sınırında faz kırılır ve
çok hücreli bir alt çizgi kesintili görünür. Kendiliğinden gelen bir özellik
değil, kurulurken sağlanacak bir kısıt.

**Reddedilen alternatif — `cell_rule` pipeline'ı:** `docs/ARASTIRMA.md:31`
Metalterm'de böyle bir pipeline'ın **var olduğunu** söylüyor ama **nasıl
çizdiğini** söylemiyor; aynı envanterde `mt-atlas/raster` ve `boxdraw` da var.
İki okuma da referansa uygun, sadelik sprite'ı seçtiriyor. Pipeline rotası
gerekirse geri gelebilir — asıl kazancı mutlak ekran x'inden türeyen
**kesintisiz** dalga olurdu, yani yukarıdaki periyot kısıtından kurtulmak.
Bugün o kısıt ucuz, o yüzden ertelendi.

## Karar 5: Atlas anahtarı ve font yüzleri

`yuvalar: HashMap<char, u16>` → `HashMap<(char, Face), u16>`; `slot(ch, face)`.
İmza değişimi `renderer.rs`'in ödünç düzenini kırmıyor: `face` `Copy`,
`Upload<'a>` yine yalnız `&self.tampon`'u ödünç alıyor.

Dört yüz `CTFontCreateCopyWithSymbolicTraits` ile düz yüzden türetilir.

**Denetim `font.rs`'inkiyle aynı sınıf ama aynı değil.** İlk taslak dönen
**aile adını** karşılaştırmayı öneriyordu; bu totoloji: API'nin belgelenmiş
sözleşmesi zaten "aynı ailede yeni bir font, yoksa NULL" ve `Menlo-Bold`'un
ailesi `Menlo`. Doğru sorular ikisi:

1. **`nil` mi?** — `objc2-core-text` bunu `Option<CFRetained<CTFont>>` olarak
   veriyor, yani kapı tipte.
2. **İstenen trait'i gerçekten edindi mi?** — `symbolic_traits()` ile maske
   kesiştirilir. Fontun kalın yüzü yoksa CoreText düz yüzü geri verebilir ve
   *o sessiz ikame `font.rs`'in yaşadığı hatanın ta kendisidir.*

Bulunamayan yüz **düz yüze geri düşer**; uyarı atlas kurulurken **bir kez**
basılır, kare başına değil.

**Manifest'e dokunuyor.** `copy_with_symbolic_traits` ve `CTFontSymbolicTraits`
`CTFontTraits` feature'ının arkasında ve workspace bugün onu açmıyor
(`Cargo.toml:35-37` yalnız `"std", "CTFont", "CTFontDescriptor",
"objc2-core-graphics"`). Yeni **crate** değil, ama yeni bir feature satırı →
`## Yayın Etkisi`'ne düşer ve `Cargo.lock` oynarsa bilinçli karardır.

Sentetik kalın (glyph'i kaydırıp üst üste basmak) ve sentetik eğik (shear
matrisi) **kapsam dışı**, kayda geçer.

**Yuva tüketimi** — aritmetik, ölçüm değil: kapasite `(1024/w) × (1024/h)`
ve aynı karakter **yüz başına ayrı yuva** tutar. "Dörde katlanır" yanlış
olur (yalnız birden çok yüzle geçen karakterler için). Tahliye yok; dolan
atlas `TOFU`'ya düşer, yani belirti **görünür** kutu, sessiz kayıp değil.
LRU zaten 00X'e kayıtlı.

## Karar 6: Metrik hangi yüzden? → **yalnız düz yüzden**

Hücre genişliği düz yüzün boşluk advance'i olarak kalır; kalın glyph aynı
yuvaya rasterize olur ve bir piksel kırpılabilir. Her terminal böyle yapar —
hücre ızgarası yüze göre oynayamaz. Bekçisi: kalın `M`'in yuvaya sığdığını
doğrulayan bir sınama.

`Metrics` iki alan kazanır:

```rust
pub underline_px: (u16, u16),  // (üstten konum, kalınlık)
pub strikeout_px: (u16, u16),
```

`CTFontGetUnderlinePosition` (negatif = taban çizgisinin altında) ve
`CTFontGetUnderlineThickness`'ten türer. Üstü çizili için CoreText API'si
**yok**: `CTFontGetXHeight`/2 kadar taban çizgisinin üstü kullanılır.
İkisi de **hücrenin içine kırpılır** — küçük descent'li bir fontta alt çizgi
`cell_px.1`'in dışına düşer ve sessizce komşu satıra taşar.

Alan eklemek kırıcı değil: `Metrics` yalnız `font.rs:127`'de kuruluyor,
`bt-gpu` onu yalnız okuyor, `bt-shell` hiç görmüyor.

Bunlar ölçüm değil, fonttan okunan değerler: "ölçülmemiş sayı yazılmaz"
kuralı bunlara işlemez.

## Karar 7: Atlama koşulu — `then_some` mı, koşulun kendisi mi?

003 koda şunu not düşmüş: *"altı çizili bir boşluk `Some(' ')` olacak ve tek
dokunulacak satır aşağıdaki `then_some`"*. **Karar bunun tersi.**

Altı çizili boşluk bir **kural** istiyor, rasterize edilmiş boş bir yuva
değil. `Some(' ')` yapmak atlasa bir boşluk glyph'i yükletir: yuva harcar,
hiçbir piksel boyamaz. Doğrusu `ch`'yi `None` bırakıp koşulu genişletmek:

```rust
if bg.is_none() && ch.is_none()
    && underline == UnderlineStyle::None && !strikeout { continue; }
```

003'ün o yorumu aynı phase'de düzeltilir (kodla çelişen cümle kuralı).

## Karar 8: Küçükler

- **`HIDDEN` kuralları da düşürür.** `\e[8m` "mürekkep yok" demek; altı
  çizili gizli metin çizgiyi gösterirse gizleme delinir.
- **Geniş karakterin ikinci hücresi bayrakları taşır** (alacritty şablondan
  kopyalıyor), yani kural iki hücreye kendiliğinden yayılır. Bedava, ama
  kayda geçer — sonradan "neden çalışıyor" sorusunun cevabı.
- **Kalın ≠ parlak renk.** Alacritty'nin `draw_bold_text_with_bright_colors`
  davranışı ikili tarafta ve varsayılanı kapalı. **Öneri: yapmayalım** —
  kalın yüz zaten var, rengi de oynatmak iki mekanizmayı birbirine bağlar.
- **003'ten devredilen borç.** `003/teslim.md` B.3 *"ölçeğin `bt-gpu`'ya iki
  kapısı var (`Surface::set_size` ve `cell_metrics`)"* maddesini 004'e
  devretmiş. **Öneri: gerekçesiyle yeniden ertelensin** — ölçek borusuyla
  ilgili, yazı biçimiyle değil, ve bu seti genişletir. Sessizce düşmesin
  diye `## Kapsam Dışı`'na açıkça yazılır.

## Karar 9: Duman sözleşmesine `kural=R` jetonu ve reçetesi

`glif=` ile aynı sınıf kapı: renderer kuralları hiç çizmese de
`kare=N hucre=8 glif=6 pipeline=ok` **hepsi > 0** basılır ve `make duman`
yeşil geçer.

**İlk reçete yetersizdi.** `\033[4m` yalnız **düz** çizgidir
(`vte/src/ansi.rs:1843` → `[4, ..] => Attr::Underline`); beş stilin dördünü
hiç sınamaz. Kıvrım dalını düz çizgiye düşüren bir kod tıpatıp aynı `kural=R`
sayısını basardı — yani setin **varlık sebebi** kapının kör noktasında kalırdı.

**Reçete** — sabitleri kırmıyor, çünkü eklenen hücrelerin hepsi `bg: None,
ch: None`, yani `hucre=8` ve `glif=6` bit bit duruyor ve hepsi Karar 7'nin
yeni yan tümcesinden geçiyor:

```sh
printf '\033[41;1;4m bateri \033[0m\033[4m \033[0;4:2m \033[0;4:3m \033[0;4:4m \033[0;4:5m \033[0;9m \033[0;4:3;58;5;196m \033[0m\n'
```

Aradaki `\033[0;` **zorunlu**: `Attr::Strike` `ALL_UNDERLINES`'ı kaldırmıyor,
sıfırlanmazsa o hücre kesikli **artı** üstü çizili olur.

> **İki nokta yük taşıyor.** `\033[4;3m` ≠ `\033[4:3m`. Noktalı virgüllü hâl
> `Underline + Italic`'tir (`[3] => Attr::Italic`); iki noktalı hâl undercurl.
> Reçeteyi "sadeleştiren" biri `:`'yı `;` yaparsa sınama sessizce
> düz-altı-çizili-eğik'e iner ve duman yeşil kalır.

**Kapı üçe ayrılır** — üçü de var olan örüntünün kopyası:

| kapı | ne kanıtlar |
|---|---|
| `make duman` → `kural=R > 0` | sınır kural üretti (CPU sayacı, `hucre=`/`glif=` sınıfı) |
| `bt-core` sınaması — **stil dizisini** assert eder | beş stilin ayırt edildiği; `hucre=8`/`glif=6` sahipliğinin aynısı |
| offscreen sınama — kural bandının **x boyunca tekdüze olmadığı** | kıvrımın gerçekten dalga olduğu; `glif_hucrenin_icini_arka_planindan_ayirir`'ın analoğu, tam bayt assert etmeden |

**Reçete ile jeton aynı phase'e girmez.** Reçete + `bt-core` stil sınaması
sınır phase'ine girer (o phase'de `Frame::push` o hücreler için hiçbir şey
push etmez: `bg: None` → `bg_count` oynamaz, `ch: None` → glyph yok, yani
`make duman` yeşil kalır). Yalnız `kural=` **jetonu** çizen phase'i bekler —
erken girerse `R=0` olur ve set ortasında duman kırmızıya düşer.

Sahiplik `hucre=`/`glif=` ile aynı: `bt_core::smoke_shell` **ve** kendi
`bt-core` sınaması. Jeton kuralı korunur: **eklenir, silinmez.**

## Karar 10: Bayrak eşlemesinin tuzağı — `UNDERCURL`, `UNDERLINE` demek değil

Bu setin **en sessiz** hata kaynağı ve refleks tam ters yönde çalışıyor.

`Attr::Undercurl` önce `ALL_UNDERLINES`'ı **siliyor**, sonra yalnız
`UNDERCURL` ekliyor (`alacritty_terminal/src/term/mod.rs:1910-1913`):

```rust
Attr::Undercurl => {
    cursor.template.flags.remove(Flags::ALL_UNDERLINES);
    cursor.template.flags.insert(Flags::UNDERCURL);
},
```

Yani **kıvrımlı metinde `Flags::UNDERLINE` kapalıdır.** `bt-core`'un eşlemesi
refleksle `cell.flags.contains(Flags::UNDERLINE)` diye yazılırsa kullanıcının
istediği dalgalı çizgi **hiç çizilmez** ve hiçbir sayaç bunu görmez.

Doğru eşleme beş bayrağı **ayrı ayrı** sorar ve sırası önemlidir (beşi birbirini
dışlıyor, ama savunmacı sıra tek bir doğru cevap verir):

```rust
let underline = if f.contains(Flags::UNDERCURL)         { UnderlineStyle::Curl }
    else if f.contains(Flags::DOUBLE_UNDERLINE)         { UnderlineStyle::Double }
    else if f.contains(Flags::DOTTED_UNDERLINE)         { UnderlineStyle::Dotted }
    else if f.contains(Flags::DASHED_UNDERLINE)         { UnderlineStyle::Dashed }
    else if f.contains(Flags::UNDERLINE)                { UnderlineStyle::Single }
    else                                                { UnderlineStyle::None };
```

Bekçisi Karar 9'un `bt-core` stil sınaması: beş hücrenin **beş ayrı** stil
vermesini assert eder. `contains(UNDERLINE)` refleksi o sınamada anında kırmızı
düşer.

## Karar Noktaları

1. **Kapsam** — 004 = yüzler + kurallar mı, yoksa emoji/kutu çizim de mi? (Karar 1)
2. **Alt çizgi çeşitleri** — düz + üstü çizili yeterli mi, ayrı renk gerçekten çıksın mı? (Karar 3)
3. **Duman kapısı** — `kural=` jetonu eklensin mi? (Karar 9)

Geri kalanların (2, 4, 5, 6, 7, 8) tek makul yolu var; kayıt için yazıldılar.

## Muhakeme (2026-09-11)

| Mercek | Verdict |
|---|---|
| Sadelik / YAGNI | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Hiçbir mercek `KIRMIZI` vermedi: yaklaşım ayakta, itirazlar giderildi.

**Kabul edilen itirazlar → plan değişikliği:**

- **`Face` `bt-core`'da yaşayamaz** (üç mercek birden; ana döngüde
  `Cargo.toml`'lardan doğrulandı: `bt-atlas`'ın `bt-core` bağımlılığı yok,
  `bt-gpu` ikisini birden gören tek katman) → Karar 2 baştan yazıldı: iki
  tip, iki crate, çeviri `bt-gpu`'da. Ayrıca "sonradan birleştirilmesin"
  gerekçesi plana yazılacak bir kısıt olarak eklendi.
- **`Rules` tipi vaat ettiği rengi taşıyamıyor** (Sadelik + Codebase-fit;
  taslak kendi içinde çelişiyordu) → SGR 58 kapsam dışına alındı, `Rules`
  tipi tümden düştü, dört `bool` kaldı. Karar 3 yeniden yazıldı.
  **(2026-09-11 tarihinde geçersiz: kullanıcı kıvrımlıyı isteyince SGR 58
  geri geldi; `Rules` tipinin düşmesi kararı korundu, gerekçesi Karar 2'de
  yeniden türetildi.)**
- **`Frame::push` kural metriğine erişemez** (Codebase-fit; `CellMetrics`'in
  font metriği taşımayı reddi ve atlas ödüncünün `encode_glyphs`'e hapsi
  kanıtıyla) → `Frame::clear(cell_px, rule_px)` parametresi, var olan
  `cell_px` örüntüsünün aynısı.
- **Aile adı denetimi totoloji** (Codebase-fit; API sözleşmesi "aynı ailede
  font, yoksa NULL") → denetim `symbolic_traits()` kesişimine çevrildi.
- **`CTFontTraits` feature'ı kapalı** (Codebase-fit; ana döngüde
  `Cargo.toml:35-37`'den doğrulandı) → manifest satırı `## Yayın Etkisi`'ne.
- **003'ün devrettiği "ölçeğin iki kapısı" borcu anılmamış** (İşletme) →
  Karar 8'e açık yeniden erteleme olarak girdi.
- **Duman betiğini serbestçe değiştirmek üç sınamayı kırar** (İşletme) →
  Karar 9 `hucre=8`/`glif=6`'yı koruyan **ve** yeni atlama koşulunu sınayan
  reçeteyle yazıldı.
- **Atlas yuva tüketimi ve dört yüzün kurulum bedeli anılmamış** (İşletme) →
  yuva tüketimi Karar 5'e aritmetik olarak, kurulum bedeli `## Yayın
  Etkisi`'ne "ölçüm bekliyor" olarak (003 B.1 #4'ün devamı).
- **İkinci belge çelişkisi** (`bt-atlas/src/lib.rs:10-11`) (İşletme) →
  Karar 1'in notuna eklendi.

**Reddedilenler:**

- **"Kurallar `bg` listesine yazılsın, üçüncü liste gereksiz"** (Sadelik) —
  merceğin sıra analizi glyph için doğru (`sa·fg + (1−sa)·fg = fg`) ama
  **imleci atlıyor**: `push_cursor` imleç bloğunu `bg`'nin sonuna ekliyor
  (`frame.rs:150-158`) ve o blok opak, yani imlecin altındaki hücrenin alt
  çizgisi örtülürdü. Üçüncü liste korundu — ama gerekçesi merceğin
  tartıştığı glyph sırası değil, imleç sırası; Karar 4 buna göre yazıldı.
- **"`kural=` jetonu eklenmesin, yeni bir şey kanıtlamıyor"** (Sadelik) —
  gerekçe "kural aynı listeden aynı pipeline'a gidiyor" varsayımına
  dayanıyordu; üçüncü liste korunduğu için varsayım düştü. İşletme merceğinin
  `glif=` paralelliği ve kırmayan reçetesi kabul edildi. Yine de karar
  kullanıcıya soruluyor (Karar Noktası 3).
- **"İki yüz (düz + kalın) yeterli"** — Sadelik merceğinin kendisi de
  reddetti: mekanizma iki yüzde de dört yüzde de aynı, eğik yüzün bedeli
  sıfıra yakın, kaybı gerçek (`bat`, `delta`, markdown, nvim).
- **`Metrics`'in iki kalınlık alanını tek alanda birleştirmek** (Sadelik,
  kendi de "itiraz değil" dedi) — bir `u16` kazandırır, okunurluk kaybettirir.

## Karar (2026-09-11, kullanıcı onayı)

- **Kapsam: yalnız yazı biçimleri** — kalın, eğik, alt çizgi ailesi, üstü
  çizili. Gerekçe: emoji tek hücreye sığmıyor ve 003'ün iki öncülünü birden
  kırıyor (tek `R8Unorm` doku, hücre boyutunda glyph); aynı problemi 003'ten
  devreden "geniş karakter yarım çiziliyor" borcuyla tek sette çözmek daha
  temiz. Kutu çizim ise fonttan glyph almayan yordamsal bir iş, konu birliği
  yok.
- **Reddedilen: emoji bu sete** — geniş-glyph setini yarım bırakırdı.
- **Reddedilen: kutu çizim bu sete** — yazı biçimleriyle ortak parçası yok.
- **Alt çizgi: kıvrımlı dâhil, beş çeşit + SGR 58 rengi.** Kullanıcı
  kıvrımlı çizgiyi (nvim / dil sunucusu tanı çizgileri) açıkça istedi.
  Panelin 1. turu bu yönü **görmedi** — Karar 3 ve Karar 4 bu karara göre
  yeniden yazıldı ve muhakeme 2. tura sokuldu.
- **Reddedilen (öneri geri alındı): "yalnız düz + üstü çizili, ayrı renk
  yok"** — 1. turun önerisiydi; kullanıcı kıvrımlıyı isteyince gerekçesi
  düştü. Kıvrımlı bir shader zorunlu kılıyor, o shader varken beş stil aynı
  shader'ın desen dallarına iniyor ve SGR 58 rengi olmadan undercurl yarım
  özellik kalıyor.
- **`kural=` duman jetonu: eklenecek** (ana döngü kararı, Karar 9). İki
  mercek çelişti; tie-break kodda yapıldı — üçüncü liste korunduğu için
  jeton gerçekten yeni bir yolu kanıtlıyor.

## Muhakeme — 2. tur (2026-09-11)

Kullanıcının kıvrımlı çizgi kararı tasarımı 1. turun hiç görmediği bir yöne
taşıdı; panel yalnız **değişen yüzey** için tekrarlandı.

| Mercek | Verdict |
|---|---|
| Sadelik / YAGNI | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Yine `KIRMIZI` yok. Ama tur, tasarımın **rotasını** değiştirdi.

**Rota değişikliği — kabul (Sadelik):** *"dalga çizilemez"* öncülü yanlıştı;
yalnız iki seçenek (`cell_bg` ya da yeni shader) varsayıyordu. Üçüncüsü
zaten kurulu: `cell` pipeline'ı bir **kapsama maskesi** çizicisi
(`cell.metal:55-56`, ana döngüde doğrulandı) ve `bt-atlas`'ta yordamsal
raster hem var hem sınanmış (`raster.rs` alfa-only CG bağlamı, `tofu_tamponu`
fontsuz çizim — ana döngüde doğrulandı). Karar 4 sprite rotasına çevrildi.

**Rota değişikliğinin konusuz bıraktıkları** (itiraz geçerliydi, tasarım
değişince ortadan kalktı):

- `rule_px`'in `Frame::clear`'a ulaşma yolu yok (Codebase-fit + İşletme, iki
  mercek bağımsız buldu; `CellMetrics`'in font metriğini reddeden doc'u
  kanıtıyla) → sprite rotasında kural metriği `Frame`'e hiç girmiyor.
- Kıvrım genliğinin kırpması sahipsiz ve `Metrics`'in `(u16,u16)` şekli onu
  ifade edemiyor (Codebase-fit) → kırpma `font.rs:127`'ye, `cell_px`'i bilen
  tek yere iniyor.
- `RuleInstance` 48 bayt / 3 dolgu (Sadelik + Codebase-fit) → tip tümden yok,
  `GlyphInstance` kullanılıyor.

**Kabul edilen, rotadan bağımsız itirazlar:**

- **`UNDERCURL`, `UNDERLINE`'ı içermiyor** (İşletme; ana döngüde
  `term/mod.rs:1910-1913`'ten doğrulandı) → **Karar 10** olarak yazıldı.
  Setin varlık sebebini sessizce boşa çıkaracak tek satırlık hata.
- **`\033[4m` yalnız düz çizgi** (İşletme; `vte/src/ansi.rs:1838-1843`) →
  Karar 9'un reçetesi beş stili + üstü çiziliyi + SGR 58'i sınayan hâle
  getirildi, `hucre=8`/`glif=6` bit bit korunarak.
- **`\033[4;3m` ≠ `\033[4:3m`** (İşletme) → Karar 9'a tuzak notu.
- **Reçete ile jeton aynı phase'e girmemeli** (İşletme) → Karar 9'a yazıldı.
- **Revizyon kalıntıları** (Codebase-fit): Karar 2'nin `underline: bool`
  taslağı, Karar 7'nin enum'la derlenmeyen koşulu, 1. tur kaydının bayat
  "SGR 58 kapsam dışı" cümlesi → üçü de düzeltildi; kayıt silinmedi,
  tarihlenerek geçersiz işaretlendi.
- **Phase bölmesi** (İşletme): yüz yarısı ile kural yarısı birbirine bağlı
  değil → `plan.md → ## Yaklaşım`'da üç phase, gerekçesiyle.
- **Ölçüm dili** (İşletme): yeni bağımsız liste açılmaz, 003 B.1'in
  #2/#3/#4/#5'ini **genişlettiği** yazılır ve kancası hâlâ olmadığı için
  `/measure` yine "ölçüm aracı yok" der.
- **`Cell` alan sayısı** (Sadelik): 5 → 10. Kopyalama maliyeti sorun değil
  (sink jenerik ve satır içine alınıyor), ama "geçici kare-başı tip"
  savunması ilk kez zorlanıyor — `plan.md`'ye açık not.

**Reddedilenler:**

- **`STRIKEOUT` ayrı mekanizmadan çizilsin** — Sadelik merceğinin kendisi de
  reddetti: `cell_bg`'ye ayırmak imleç sırası problemini ikinci kez doğurur
  ve iki kod yolu açar.
- **Rengi `u8x4`'e paketlemek** (Sadelik, kendi reddetti) — `CLAUDE.md` renk
  uzayı kuralı: 8-bit **lineer** koyu tonlarda bantlanır.
- **Stil başına ayrı draw call** (Sadelik, kendi reddetti) — `instans_tamponu`
  draw başına kare başına tampon ayırıyor; beş liste beş ayırma demek.
- **`cell_rule` pipeline'ı** — bkz. Karar 4'ün son paragrafı: referans
  envanteri iki okumaya da açık, kazancı (kesintisiz dalga) bugünkü periyot
  kısıtının bedelinden küçük. Gerekirse geri gelir.
