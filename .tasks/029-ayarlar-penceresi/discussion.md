# Ayarlar penceresi — Tartışma

Düzen, kategoriler ve "tek kaynak dosya" kuralı kullanıcıyla kararlaştırıldı
(`context.md` → Motivasyon) ve burada yeniden açılmıyor. Kalan karar noktaları
birbirinden bağımsız; biçim karar-listesi. Değişecek dosyalar pahalı karar
sınıfına girmiyor (yeni crate yok, katman yönü korunuyor, `Cell`/terminfo/
betik/kare yolu yok), yani panel koşmadı (`/rfc` adım 6).

## Karar 1: Pencere dosyaya nasıl yazıyor → ✅ tipli düzenleme, `with_theme`'in genellemesi

- **A — `Settings::with_edit(text, SettingsEdit)`** (`bt-core`, saf): tipli
  bir düzenleme (`Scrollback(usize)`, `Cursor(CaretShape)`, `Theme(String)`,
  `FontFamily(Option<String>)`, …) tek anahtarı değiştirir; enum → dizge
  çevirisi `bt-core`'un yazılış tablosundan. `with_theme`'in bütün
  güvenceleri (bölüm yoksa sona, anahtar yoksa bölüme; süs ve satır sonu
  yorumu; kuyruk yorumunun bölüme taşınması; CRLF; ayrıştırılamayan metin,
  bölüm-olmayan bölüm ve bölüm-olan anahtar `Err`) **aynı kodla** geçerli,
  `with_theme` onun bir çağıranı olur.
- **B — Bütün `Settings`'i serileştirmek**: yorumları, bilinmeyen anahtarı ve
  kullanıcının yazmadığı anahtarların yokluğunu siler. Deponun kuralına aykırı
  (`CLAUDE.md` → Ayarlar; `toml_edit`'in seçilme sebebi).
- **C — `bt-shell`'de dizge anahtar + dizge değer**: enum yazılışlarını
  `bt-shell`'e ikinci kez taşır.

## Karar 2: Geçerli değerlerin tek kaynağı → ✅ `bt-core`'un yazılış tabloları ve aralıkları `pub`

Pencere popup'larını, slider/stepper sınırlarını **ayrıştırıcının okuduğu
listeden** kurar: her dizge enum'u (`CaretShape`, `CursorBlink`,
`UnfocusedCaret`, `ConfirmClose`, `CursorMotion`, `ReduceMotion`,
`SmoothScroll`, `ShellIntegration`, `Osc52`) tek bir `NAMES` tablosu taşır ve
ayrıştırıcı da `name()` de oradan okur (`UnfocusedCaret::NAMES` emsali);
aralıklar (`scrollback`, `cursor_radius`, `cursor_glow`,
`cursor_blink_interval`, `line_height`) `pub` sabit. Tanı cümleleri **değişmez**
— yol haritasının borcu taşımayı tam bunun için bekletiyordu; tablo aramayı
değiştiriyor, mesajı değil. Popup'taki insan okunur başlık (`"Only when a
program is running"`) UI dizgisi ve `bt-shell`'de, enum varyantı üstünde
**kapsamlı `match`** ile: yeni varyant derleme hatası verir, sessizce eksik
kalmaz.

`size` için dosyanın sınırı yalnız "0'dan büyük"; stepper'ın sınırı geçici
puntonun aralığı (`zoom.rs`'in `MIN_SIZE`/`MAX_SIZE`), ikinci bir sayı
uydurulmuyor. Dosyada aralığın dışında bir değer varsa alan onu **olduğu
gibi** gösterir.

## Karar 3: Font listesi nereden → ✅ `bt-atlas`, `bt-gpu` üzerinden

- **A — `bt-atlas::font::monospaced_families()`**: CoreText'in aile listesi
  (`CTFontManagerCopyAvailableFontFamilyNames`) + `open_chain`'in kullandığı
  **aynı** `TraitMonoSpace` biti; `FontNotice` gibi `bt-gpu`'dan yeniden ihraç.
  Popup'ta görünen her aile "not monospaced" uyarısı vermez, çünkü ölçüt tek.
  Bedeli `objc2-core-text`'e `CTFontManager` bayrağı (yeni crate değil).
- **B — `NSFontManager` `bt-shell`'de**: ikinci bir eşaralıklılık ölçütü
  (`NSFixedPitchFontMask`); iki ölçüt ayrıştığı gün listeden seçilen aile
  uyarı verir. Reddedildi.

Listenin başında **"Default (SF Mono, or Menlo)"** öğesi `family = ""` yazar
(belgelenmiş varsayılan, uyarısız; anahtar silinmez). Dosyadaki aile listede
yoksa (bulunamayan ya da eşaralıklı olmayan) seçili öğe olarak **o ad** eklenir,
yanında durumu (`— not found` / `— not monospaced`); popup kullanıcının
yazdığını gizlemez.

## Karar 4: Pencerenin iskeleti → ✅ `NSSplitViewController` + kaynak listesi + `NSGridView`

- Sol: `NSSplitViewItem` **sidebar** davranışıyla (sistemin kenar çubuğu
  materyali, başlık çubuğunun altına uzanan tam boy içerik), içinde tek sütunlu
  `NSTableView` (kaynak listesi stili), satırlar `NSTableCellView` + SF Symbol
  (`NSImage` `imageWithSystemSymbolName:`): General `gearshape`, Appearance
  `paintpalette`, Cursor `character.cursor.ibeam`, Motion `wind`. Sembol
  bulunamazsa ikon boş kalır, satır kalır.
- Sağ: seçilen kategorinin başlığı ve bir `NSGridView` — sol sütun sağa yaslı
  etiket, sağ sütun kontrol; açıklama kontrolün altında küçük, ikincil renkte
  (`secondaryLabelColor`, küçük sistem fontu). Açık/koyu görünümü sistemden
  miras alır (terminal penceresinin tema kromu **bu pencereye uygulanmaz**).
- `NSOutlineView` reddedildi: dört düz satırın hiyerarşisi yok.
  `NSTabViewController` (klasik araç çubuğu sekmeleri) kullanıcı kararıyla
  dışarıda.
- Pencere **tek örnek**, `AppDelegate`'in ivar'ında; kapatınca gizlenir
  (`releasedWhenClosed = false`), yeniden açınca aynı kategoride döner. Sekme
  **almaz** (`tabbingMode = disallowed`): ⌘T ayar penceresine sekme eklememeli.
  `AppDelegate::windows()` listesine girmez — ⌘Q'nun onayı, ayar yayılımı ve
  sekme işleri yalnız terminal pencerelerini görür.
- Modül `bt-shell/src/settings_window.rs`; kontrollerin eylemleri pencerenin
  kendi `define_class!` nesnesine gelir ve yazmayı `AppDelegate`'e verir.

## Karar 5: Ne zaman yazılır → ✅ popup/switch anında, slider bırakınca, sayı alanı onaylayınca

- Popup ve switch: seçim anında yazar.
- Slider (`cursor_radius`, `cursor_glow`, blink hızı): **bırakınca**
  (`continuous = false`). Sürükleme boyunca yazmak saniyede onlarca dosya
  kaydı, onlarca `reload_settings` ve editörde açık dosyaya "diskte değişti"
  demek olurdu. Yanında değer etiketi sürüklerken güncellenir.
- Sayı alanı + stepper (`scrollback`, `size`, `line_height`): alan **Enter'da
  ya da odaktan çıkınca**, stepper tıklamasında. Tuş başına yazmak `scrollback`'te
  ara değeri kayıt yapar ve küçültmek geçmişi o anda siler
  (`docs/AYARLAR.md` → `[terminal]`). Kabul edilmeyen girdi (harf, aralık
  dışı) alanı dosyadaki değere geri döndürür, yazmaz.
- Ondalıklar yazılırken iki basamağa yuvarlanır (`1.2000000000000002`
  dosyaya düşmesin); tamsayı anahtarlar tamsayı yazılır.
- Her yazmadan sonra `save_theme` örüntüsü: izleyiciyi beklemeden
  `reload_settings`; ardından gelen vnode olayı boş fark.
- Blink hızı slider'ı yarım periyodu **logaritmik** ölçekte gösterir, sağ
  "hızlı" (kısa periyot); sınırlar `cursor_blink_interval` aralığı. Değer
  etiketi saniye (`0.5 s`).

## Karar 6: Bağımlı satırlar → ✅ gizlenmez, devre dışı kalır

- Light theme / Dark theme: `theme` Match System değilken **devre dışı** (değer
  görünür kalır; Match System'e dönünce çift geri gelir — `with_theme`'in
  sözü). Listelerinde `system` yok.
- Blink speed: `cursor_blink = "off"` iken devre dışı.
- Satırın yeri oynamaz: gizlemek pencerenin boyunu seçime göre zıplatırdı.

## Karar 7: Dosya bozukken, yokken, dışarıdan değişince → ✅ kilit + sebep + "Open settings.toml"

- **Ayrıştırılamayan ya da okunamayan dosya**: bütün kontroller devre dışı,
  sağ bölmenin üstünde bir şerit sebebi söyler (alt başlığın `Source::Settings`
  yuvasındaki metnin aynısı) ve "Open settings.toml" düğmesi öne çıkar.
  Gerekçe: yazma zaten reddedilecek (`with_theme`'in sözü), ve kontrolleri
  açık bırakmak kullanıcıya işe yaramayan tıklamalar sunmak olurdu; kilit
  nedenini söyleyen tek görünür hâl. Kullanıcı tarafı seçildi: dosyayı onarmak
  bir tık uzakta.
- **Dosya yok**: kontroller varsayılanları gösterir ve açıktır; pencereyi
  açmak dosya **yaratmaz**, ilk değişiklik yaratır (`write_theme` emsali:
  şablon + anahtar).
- **Tek anahtarın değeri kabul edilmiyor**: kontrol geçerli (ekrandaki) değeri
  gösterir, altındaki açıklamanın yerine tanı çıkar (`Diagnostic::key` o
  satıra eşlenir). Kontrolden yeni bir değer seçmek satırı düzeltir.
- **Dışarıdan değişim**: `reload_settings` her koşuda (vnode ya da pencerenin
  kendi yazması) açık ayar penceresini tazeler; kilit hâli de oradan kalkar.
  Tema listesi de tazelenir (`themes/`'e yeni dosya).
- **Yazma hatası** (izin, bozuk bölüm): alt başlığın `Source::Write` yuvası
  (bugünkü yol) **ve** pencerenin şeridi; kontrol dosyadaki değere döner.

## Karar 8: "Open settings.toml" ve Cmd-, → ✅ düğme bugünkü `edit_settings`'i koşar

Cmd-, (bateri ▸ Settings…) pencereyi açar/öne getirir. Bugünkü davranış
(yoksa şablonla yarat, editörde aç) pencerenin altındaki **"Open
settings.toml"** düğmesine taşınır — sağ bölmenin altında, kategoriden
bağımsız, sağa yaslı. Süreli koşuda (`Inputs::Hermetic`) Settings… bugünkü
gibi hiçbir şey yapmaz: pencere hiç doğmaz, `make duman` onu görmez.

## Karar 9: Shell integration'ın notu → ✅ "yeni sekmelerde ve pencerelerde"

`[shell] integration` kabuk doğarken okunuyor (`AppDelegate::shell_integration`
güncel ayardan) ve yeni sekme de yeni bir kabuk; açıklama satırı
*"Takes effect in new tabs and windows."* der. Phase bunu yeni sekmeyle
gözle doğrular.

## Karar (2026-09-23, otonom akış)

- **Seçilen:** Karar 1–9'un ✅ satırları. Özü: tek kaynak `settings.toml`,
  pencere `bt-core`'un saf ve tipli düzenlemesiyle **yalnız yazar**, uygulayan
  ve pencereyi tazeleyen mevcut `reload_settings`; geçerli değerler ve
  aralıklar ayrıştırıcının kendi tablolarından, font listesi eşaralıklılık
  ölçütünün kendi yerinden. Kontrol biçimi değerin biçiminden: iki değerli
  (`osc52`, `smooth_scroll`) switch, üç ve fazla değerli popup, aralıklı
  ondalık slider, sayılar alan + stepper.
- **Reddedilen:** bütün ayarı serileştirmek (yorumları siler); `bt-shell`'de
  dizge düzenleme ve `NSFontManager` listesi (ikinci kopya); sürükleme boyunca
  ve tuş başına yazmak (dosya fırtınası, `scrollback`'te veri kaybı); bozuk
  dosyada kontrolleri açık bırakmak (reddedilecek tıklamalar); bağımlı
  satırları gizlemek (zıplayan pencere); `NSOutlineView`/`NSTabViewController`.
- Panel koşmadı: pahalı karar sınıfına dokunan dosya yok. Bayraklar mevcut
  crate'lerin (`objc2-app-kit`, `objc2-core-text`) ve `Cargo.lock`'un
  **değişmemesi bekleniyor** (gerekli isteğe bağlı bağımlılıklar grafta);
  değişirse `/implement` durur ve eskale eder.
