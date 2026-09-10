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
    pub underline: bool, pub strikeout: bool,
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

Ayrı bir `Rules` tipi yok — dört `bool` yeterli ve çağıran tarafta daha az kod.

**Bu bir "hücreye alan eklemek" değil.** `CLAUDE.md`'nin 24 baytlık `const`
assert'i alacritty'nin **grid hücresine** bağlı (10 000 satır scrollback ×
sekme). Buradaki `Cell` `frame()` sınırının **geçici** kare-başı tipi;
scrollback'te yaşamıyor, assert'e dokunulmuyor.

> Uygulama notu: `Cell`'e alan eklemek `bt-gpu`'daki **altı** `Cell { … }`
> sınama literalini kırar. `Default` + `..Default::default()` gerekir;
> `#[non_exhaustive]` **çözüm değil** — `bt-gpu` o zaman `Cell`'i hiç kuramaz.

## Karar 3: Hangi alt çizgi çeşitleri? → **düz + üstü çizili, ayrı renk yok**

Alacritty beş çeşit alt çizgi (düz, çift, kıvrımlı, noktalı, kesikli) ve
`cell.underline_color()` ile ayrı bir alt çizgi rengi taşıyor.

İlk taslak ayrı rengi (SGR 58) kapsama alıyordu. **Geri alındı** — taslak
kendi içinde çelişiyordu: sınır tipinde renk alanı yoktu ama Karar 4 ayrı
geometriyi tam da "ayrı alt çizgi rengi mümkün olsun" diye seçiyordu.

Ayrı renk çıkarıldı çünkü kazancı yok: SGR 58'in gerçek tüketicisi (nvim
tanı çizgileri) onu **kıvrımlı** çizgiyle birlikte kullanıyor ve kıvrımlı
zaten kapsam dışı (ayrı shader ister — Metalterm'in `cell_rule` pipeline'ının
asıl sebebi odur). Düz **ve** renkli alt çizgi tek başına neredeyse hiç
üretilmiyor. Artakalan görsel fark tartılabilir ve küçük: renkli bir alt
çizginin bir descender'ın (`g j p q y`) kenar yumuşatma bandını kestiği
birkaç piksel.

**Kapsamda:** düz alt çizgi + üstü çizili, rengi **ön plan**.
**Kapsam dışı, kayda geçer:** çift / kıvrımlı / noktalı / kesikli, SGR 58.

## Karar 4: Kural çizgisi glyph mi, ayrı geometri mi? → **ayrı geometri, üçüncü liste**

Düz bir kural çizgisi renkli bir dikdörtgen; `cell_bg` pipeline'ının
`Instance { pos, size, rgba }` yapısı zaten tam olarak onu çiziyor ve
`pos`/`size` piksel cinsinden (`frame.rs:128-133`). Yani **`.metal`
dosyalarına hiç dokunulmuyor**; `make shader`'ın koşulu doğmuyor.

Atlasta glyph olarak tutmak üç yerde kötü: yuva harcar, kalınlık ve konum
font metriğinden değil raster'dan gelir, rengi glyph'in rengine bağlanır.

**Kurallar `bg` listesine yazılamaz, üçüncü bir liste gerekir — ve gerekçe
imleçtir.** `push_cursor` imleç bloğunu `self.bg`'nin **sonuna** ekliyor
(`frame.rs:150-158`) ve o blok opak. Kural `bg`'ye girseydi imlecin altındaki
hücrenin alt çizgisi imleç tarafından örtülürdü. Üçüncü liste glyph'lerden
**sonra** kodlanır, yani imlecin de üstüne çizilir — ve orada rengi
`bt-core`'un imleç için zaten tersine çevirdiği ön plandır, yani görünür
kalması bedava (003'ün glyph için kurduğu mekanizmanın aynısı).

Yan kazanç: kural glyph'ten sonra çizildiği için descender'ları keser, ama
rengi ön planla aynı olduğundan piksel piksel aynı sonucu verir.

`bg_count` **el değmez** → `hucre=K` jetonunun anlamı bit bit korunur ve
`push`'taki `debug_assert_eq!(bg.len(), bg_count)` bekçisi olduğu gibi kalır.

> Geometri sorunu: `Frame::push` kural dikdörtgenini kurmak için alt çizginin
> konum ve kalınlığını bilmeli, ama tek bildiği ölçü `clear()`'dan gelen
> `cell_px`. `bt_atlas::Metrics`'i `Frame`'e taşımak katman tablosunu
> bulanıklaştırır — `CellMetrics` bunu zaten açıkça reddediyor
> (`renderer.rs:94-97`), atlas ödüncü de `encode_glyphs`'e hapsedilmiş.
> **Çözüm var olan örüntü:** kural metriği kare boyunca sabittir, tıpkı
> `cell_px` gibi → `Frame::clear(cell_px, rule_px)` parametresi olur, alanı
> değil. `link.rs` değeri `Renderer`'ın **ödünç değil değer** döndüren bir
> erişimcisinden alır, `atlasi_esitle` ile aynı şekilde.

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
if bg.is_none() && ch.is_none() && !underline && !strikeout { continue; }
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

## Karar 9: Duman sözleşmesine `kural=R` jetonu

`glif=` ile birebir aynı sınıf kapı: phase-2 durumunda (sınır bayrak taşıyor,
renderer yoksayıyor) `kare=N hucre=8 glif=6 pipeline=ok` **hepsi > 0** basılır
ve `make duman` yeşil geçer. Kuralların hiç çizilmediğini gören başka otomatik
kapı yok.

Betiği serbestçe değiştirmek `hucre=8`/`glif=6` sabitlerini ve üç sınamayı
oynatır. **Kırmayan reçete:**

```sh
printf '\033[41;1;4m bateri \033[0m\033[4m \033[0m\n'
```

- `hucre=8` **aynen durur** — 9. hücrenin arka planı varsayılan, sayılmaz.
- `glif=6` **aynen durur** — 9. hücre boşluk, `ch=None`.
- 9. hücre **tam olarak Karar 7'nin vakası**: arka plan yok, mürekkep yok,
  ama altı çizili. Yani atlama koşulunun yeni yan tümcesi duman kapısından
  geçer. (Arka planlı bir varyant bunu **sınamaz**: sekiz hücre zaten
  `bg.is_some()` ile geçiyor, yeni yan tümce hiç karar veren dal olmaz.)
- İlk sekiz hücre `BOLD` taşır, yani `Face` yolu da betikte var.

**Sınır: `kural=` setin yalnız kural yarısını kapatır.** `Face` her zaman
`Regular` dönen bir yapı da aynı `kural=9`'u basar; yüz yarısının kapısı ayrı
ve birim düzeyinde (`slot('M', Bold) != slot('M', Regular)` + offscreen
sınama). Jeton, renderer'ın çizdiği phase ile **aynı commit'te** girer —
erken girerse `R=0` olur ve set ortasında `make duman` kırmızıya düşer.

Sahiplik `hucre=`/`glif=` ile aynı: `bt_core::smoke_shell` **ve** kendi
`bt-core` sınaması. Jeton kuralı korunur: **eklenir, silinmez.**

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
