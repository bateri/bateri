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

<!-- /implement doldurur. -->

## Yayın Etkisi

- **shader** — yok.
- **terminfo / `TERM`** — yok.
- **ayar şeması** — yok. SGR 58 var olan `color::resolve` yolundan geçiyor,
  yeni kullanıcı ayarı doğmuyor (`docs/AYARLAR.md` ve ayar modeli henüz yok).
- **tema / materyal** — yok.
- **shell entegrasyonu** — yok. `smoke_shell` bir **sınama** betiği,
  `assets/shell/` altındaki entegrasyon üçlüsü değil.
- **app bundle** — yok.
- **yeni bağımlılık** — yok; `Cargo.toml`/`Cargo.lock` el değmiyor.
- **belge** — `session.rs`'in `MUREKKEPSIZ` yorumu ve `smoke_shell` doc'u.
  Duman jetonu **bu phase'de eklenmiyor** (phase-3), o yüzden `CLAUDE.md`,
  `Makefile` ve `proje.md` el değmiyor.
- **ölçüm bekliyor:** sınır `Cell`'inin büyümesinin kare süresine etkisi —
  003 `teslim.md` B.1 **#5**'in genişlemesi. `size_of` bir ölçüm değil
  gerçektir ve istenirse `const` assert'le bağlanır; **kare süresine
  etkisi** ölçüm bekler ve kancası (`BT_FRAME_LOG`) hâlâ yok.

---

## Checklist

- [ ] `UnderlineStyle` enum'ı + `Cell`'in beş yeni alanı + `Default` türetimi
- [ ] `Cell`'in doc'una "24 baytlık assert bu tipe değil, grid hücresine bağlı" notu
- [ ] Bayrak eşlemesi: beş bayrak ayrı ayrı, **kıvrımlı önce** (madde 2)
- [ ] `HIDDEN` kuralları da düşürüyor
- [ ] SGR 58 → `Option<LinearRgba>`
- [ ] Atlama koşulu genişledi; `MUREKKEPSIZ` yorumu düzeltildi
- [ ] `smoke_shell` reçetesi + doc'una iki nokta uyarısı
- [ ] `bt-gpu`'nun altı `Cell { … }` sınama literali `..Default::default()`'a geçti
- [ ] Test: `kivrimli_metin_curl_verir` — `\033[4:3m` `UnderlineStyle::Curl` üretir (**`contains(UNDERLINE)` refleksi burada kırmızı düşer**)
- [ ] Test: `sabit_shell_bes_stili_ayirt_eder` — reçetenin stil dizisi `[Single, Double, Curl, Dotted, Dashed]` + üstü çizili + SGR 58 rengi
- [ ] Test: `gizli_metnin_kurallari_da_duser` — `\033[8;4m` kural üretmez
- [ ] Test: `alti_cizili_bosluk_hucresi_gecer` — `bg: None, ch: None`, kural var → sink çağrılır
- [ ] Test: `sabit_shell_arka_plan_hucreleri_verir` ve `sabit_shell_alti_glif_verir` **değişmeden** geçiyor (8 ve 6 korundu)
- [ ] Doğrulama geçti (`make hepsi`; `make shader` `[~]`; `make duman` → `kare=N hucre=8 glif=6 pipeline=ok` — jeton henüz yok; `make test-yaris` **zorunlu**, `frame()` gövdesi `Term` kilidi altında değişti)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
