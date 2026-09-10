# Yazı biçimleri — Bağlam

## Mevcut Durum

003 pencereye harfleri getirdi ama **yalnız tek yüzü ve yalnız düz metni**.
Zincirin her halkası bugün biçimi düşürüyor:

- **`bt-core` sınırı bayrak taşımıyor.** `Session::frame()` alacritty'nin
  `Flags`'inden yalnız `INVERSE` ve `DIM`'i okuyor, o ikisini de **renge
  çözüp** atıyor (`session.rs`, ön plan/arka plan hesabı). `BOLD`, `ITALIC`,
  `UNDERLINE`, `STRIKEOUT` sınırdan hiç geçmiyor; `Cell` yapısında karşılıkları
  yok. Bu bir eksik değil, 003'ün bilinçli kararı: R2.3 "biçim bayrakları
  geçmez, 004'ün işi" diyor.
- **`bt-atlas` tek font tanıyor.** `Atlas.font` tek bir `CFRetained<CTFont>`;
  zincir `["SF Mono"] → Menlo` ve yalnız düz yüzü açıyor (`font.rs:16-24`).
  Önbellek anahtarı `HashMap<char, u16>` — karakterin hangi yüzle
  rasterize edildiği bilgisi **yok**, dolayısıyla kalın 'M' ile düz 'M' aynı
  yuvayı paylaşırdı.
- **Kural çizgisi diye bir kavram yok.** `Metrics` yalnız `cell_px` ve
  `baseline_px` taşıyor; alt çizginin nereye ve ne kalınlıkta çizileceğini
  söyleyen alan yok. `Frame`'de de karşılığı yok: iki liste var, arka planlar
  (`Instance`) ve glyph'ler (`GlyphCell`).
- **Altı çizili boşluk sessizce kayboluyor.** `frame()`'in atlama koşulu
  `bg.is_none() && ch.is_none()`, ve boşluk karakteri `ch: None` üretiyor.
  Yani `\e[4m` ile altı çizilmiş bir boşluk hücresi bugün hiç sink'e
  girmiyor. 003 bunu bilerek bıraktı ve kodun içine not düştü
  (`session.rs`, `MUREKKEPSIZ` bloğunun yorumu: *"004'te değişecek olan tam
  burası"*).

## Motivasyon

Terminal çıktısının büyük kısmı biçimli: `ls` dizinleri kalın basar, `git
diff` başlıkları kalın, `man` sayfaları altı çizili, derleyici hataları
kalın kırmızı, her modern CLI (`cargo`, `gh`, `npm`) SGR biçimlerine güveniyor.
Bugün bateri hepsini **düz** çiziyor — yanlış çizmiyor, hiç çizmiyor: bilgi
sessizce düşüyor. Bir `git diff` çıktısında hangi satırın başlık olduğu
ayırt edilemiyor.

Referans davranış `docs/ARASTIRMA.md`'de: Metalterm'in `mt-atlas`'ı `fontset`
ve `boxdraw` modülleri taşıyor, `mt-gpu` 9 pipeline'ının biri **`cell_rule`
(alt çizgi)** — yani kural çizgisi orada da glyph değil, ayrı geometri.
Font seti JetBrains Mono / SF Mono / Menlo / Apple Color Emoji.

## Kanıt

Ölçüm değil, gözle görülür kayıp. Bugünkü depoda:

```sh
printf '\033[1mkalın\033[0m \033[3meğik\033[0m \033[4maltı çizili\033[0m\n'
```

Üçü de düz ve tıpatıp aynı çiziliyor; `\033[4m ` (altı çizili boşluk) hücresi
ise sink'e hiç girmiyor. Kodun kendi kaydı da bunu doğruluyor:
`session.rs`'in `MUREKKEPSIZ` yorumu ve 003 phase-4'ün `## Uygulama
Notları`'ndaki "altı çizili boşluk" maddesi.

**Yeni crate gerekmiyor, ama manifest'e dokunuluyor.** Dört font yüzü de
CoreText'in kendi API'siyle açılıyor (`CTFontCreateCopyWithSymbolicTraits`),
alt çizgi konumu ve kalınlığı da öyle (`CTFontGetUnderlinePosition`,
`CTFontGetUnderlineThickness`) — `bt-atlas`'ın gördüğü **kütüphane kümesi**
değişmiyor. Ama `copy_with_symbolic_traits` ve `CTFontSymbolicTraits`
`objc2-core-text`'in `CTFontTraits` feature'ının arkasında ve workspace bugün
onu açmıyor (`Cargo.toml:35-37`: yalnız `"std"`, `"CTFont"`,
`"CTFontDescriptor"`, `"objc2-core-graphics"`). Yani bir feature satırı
eklenecek; yeni bağımlılık değil ama `## Yayın Etkisi`'ne düşen bir manifest
değişikliği.

## Mevcut Mimari

```
alacritty Flags                bt-core                 bt-gpu
──────────────                 ───────                 ──────
INVERSE ──┐
DIM ──────┴──► renge çözülür ──► Cell { col,row,        Frame
                                        ch: Option,       ├─ bg: Vec<Instance>
BOLD      ──►  ✗ düşer                  fg, bg }          └─ glyphs: Vec<GlyphCell>
ITALIC    ──►  ✗ düşer                                          │  { pos, ch, rgba }
UNDERLINE ──►  ✗ düşer                                          ▼
STRIKEOUT ──►  ✗ düşer                                    Atlas.slot(ch)
                                                          HashMap<char, u16>
                                                          tek CTFont
```

Üç yerde de aynı boşluk: **hangi yüz** ve **hangi kural** bilgisi taşınmıyor.
