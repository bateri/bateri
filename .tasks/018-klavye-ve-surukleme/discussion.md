# Klavye ve dosya sürükleme — Tartışma

Dört karar noktası. Beşincisi (`[keyboard]` ayar bölümü) panelden sonra
**düştü**; gerekçesi `## Muhakeme`'de.

## Karar 1: Ölü tuş hangi yoldan çözülür?

Ölçüm (`context.md` → Kanıt b) bileşimin kırık olduğunu söylüyor: AppKit ölü
tuş durumunu `characters` zincirine taşımıyor. İki yol var.

### Seçenek A: `NSTextInputClient` + `interpretKeyEvents:`

`keyDown:` olayı AppKit'in metin yığınına veriliyor; yığın ölü tuş durumunu
kendi tutuyor ve bileşim tamamlanınca `insertText:` ile **metni** geri veriyor.

**Artıları:**
- Klavye düzeninin işini AppKit yapıyor; biz hiçbir düzen verisi okumuyoruz.
- Aynı kapıdan karakter paleti de geliyor (görünür kazancı 020'ye kadar yok).
- Alacritty/WezTerm/Ghostty'nin de yolu; macOS'ta desteklenen tek yol bu.

**Eksileri — panelden sonra gerçek boyutuyla:**
- **11 zorunlu metot**, hepsi yazılmadan derlenmiyor: `insertText:…`,
  `doCommandBySelector:`, `setMarkedText:…`, `unmarkText`, `selectedRange`,
  `markedRange`, `hasMarkedText`, `attributedSubstringForProposedRange:…`,
  `validAttributesForMarkedText`, `firstRectForCharacterRange:…`,
  `characterIndexForPoint:` (`objc2-app-kit` 0.3.2'de `#[optional]`
  işaretsiz). Üstüne `bt-shell/Cargo.toml`'a `NSTextInputClient` feature'ı
  (yeni crate değil, var olan bağımlılığın bayrağı). Kısmi uyum bir seçenek
  **değil**: `define_class!`'ın doc'u "debug assertion'lar açıkken protokolün
  zorunlu metotları uygulanmazsa `class` panikler" diyor.
- `keyDown:`'ın rolü değişiyor ve bu setin en riskli tek hamlesi (aşağıda
  mekanizma).
- `markedText`'in **çizimi** (CJK preedit) bu setin dışında — yol haritasında
  kendi borcu var. Bu set `insertText:`'i getirir, preedit'i çizmez.

### Seçenek B: Ölü tuşu kendimiz çevirelim

`keyCode` + `UCKeyTranslate` + kendi ölü tuş durumumuz (bu setin kanıtını
üreten betiğin yaptığı iş, ama canlı).

**Artıları:** `keyDown:`'ın tekliği bozulmaz; AppKit metin yığınına hiç
girilmez.

**Eksileri:** klavye düzeninin işini yeniden yazmak (düzen verisini açmak,
düzen değişimini izlemek, ölü tuş durumunu taşımak); karakter paleti ve IME
**hiç** gelmez — ikisi de yığının içinde yaşıyor. Bugün bir ölü
tuş bileşimini çeviren kodu yazmak, yarın Japonca girdiyi çeviren kodu
yazmaya başlamak demek.

**Seçilen: A.** B'nin tek gerçek artısı (`keyDown:`ın tekliği) A'nın tek
gerçek bedeli; A o bedeli bir kez ödeyip üçünü birden getiriyor, B ise düzen
verisini sahiplenmeyi kalıcı borç yapıyor.

### Mekanizma: yeniden giriş bayrağı

"Metin `insertText:`e, kontrol dizisi `keyDown:`te" bir **hedef durum**, yazılabilir
bir bölünme değil: hangi olayın metne döneceğine AppKit, olayı
`interpretKeyEvents:`e verdikten **sonra** karar veriyor (düzene ve
`StandardKeyBinding.dict`'e göre). Gerçek mekanizma:

```
ViewIvars: consumed: Cell<bool>        // metin yığını bu olayı aldı mı
                                       // (insertText: VE setMarkedText: set eder)

keyDown:
  1. Cmd'li olay → izin listesi (Karar 3); metin yığınına HİÇ girmez,
     çünkü ⌘⌫ orada `deleteToBeginningOfLine:` olurdu ve izin listesi
     o tuşu hiç görmezdi.
  2. Shift+PgUp/PgDn → bugünkü `page_scroll` kolu, yerinde durur.
  3. consumed.set(false); interpretKeyEvents(&[event]);
     if !consumed.get() { encode_key(…) }      // fonksiyon tuşu + kontrol
```

**`doCommandBySelector:` gövdesiz kalamaz.** Enter/Tab/Escape/Backspace ve
Ctrl'lü harfler bugün `encode_key`'in catch-all kolundan geçiyor; faz 1 o kolu
yönlendiricinin arkasına aldığı anda AppKit onları `insertText:`'e **değil**
`doCommandBySelector:`'a veriyor (`insertNewline:`, `insertTab:`,
`cancelOperation:`, `deleteBackward:`, `^A → moveToBeginningOfParagraph:`,
`^K → deleteToEndOfParagraph:`). Metot yoksa `NSResponder`'ın varsayılanı
koşar ve **bip çalar** — `view.rs`'in "`super`'e geçmiyoruz: … terminalde her
ok tuşu bip sesi olurdu" gerekçesinin aynısı, yeni kapıdan. Gövde sessiz bir
no-op: olay bayrak set edilmeden döner, fallback `encode_key`'e düşer.

**`setMarkedText:` stub bırakılmıyor.** "Boş bırakılırsa bileşim tamamlanır"
**ölçülmemiş** bir iddiaydı; alacritty ve ghostty ikisi de bir marked-text
alanı tutuyor. Faz 1 asgari bir `RefCell<String>` ivar'ı kuruyor ve
`hasMarkedText`/`markedRange`/`selectedRange` ona cevap veriyor. Çizim yok,
**durum** var.

Bayrağın adı **`consumed`** ve `setMarkedText:` de onu set ediyor: değişmez
"metin yığını bu olayı aldı", "metin geldi" değil. Ölü tuşun ilk vuruşunda
`characters` boş olduğu için fallback bugün tesadüfen zararsız; değişmezi
tesadüfe yazmak, bir gün boş olmayan bir bileşim başlangıcında tuşu iki kez
gönderir.

**Press-and-hold bir kazanç değil, kapatılacak bir yan etki.** `ü` tuşunu basılı
tutmak terminalde **tuş yinelemesi** demek (vim'de `j`, `u`); aksan popover'ı
onu çalar. Bugün bateri'de popover **yok**, çünkü protokol uygulanmıyor — yani
onu getiren şey A'nın kendisi. Kanıt kurulu bir üründe: iTerm2 kendi domain'inde
`ApplePressAndHoldEnabled = 0` yazıyor (`defaults read com.googlecode.iterm2
ApplePressAndHoldEnabled` → `0`, bu makinede 2026-09-20). Çare aynısı ve
kullanıcının plist'ine **yazmadan**: uygulamanın kendi `registerDefaults`'ı
(bellekte, rc/ayar dosyasına dokunmama kuralının aynı mantığı). Faz 1'in tek
satırlık kalemi.

**`firstRectForCharacterRange:`'in kapsam içinde tüketicisi yok.** Ölü tuş
önizlemesi popover değil **marked text** (satır içinde, altı çizili) ve onu bu
set çizmiyor; aday penceresi ise CJK'nın, yani tam IME borcunun. Bu yüzden
metot **belgelenmiş bir yaklaşımla** cevap veriyor (view'ın imleç bandı) ve
imleç hücresini crate sınırı ötesinden taşımak (`bt_gpu::Origin` emsali)
**bu sete girmiyor** — tam IME geldiğinde onun ilk işi olur. Sıfır dikdörtgen
dönmemenin sebebi kaldı, hassas olmanın sebebi kalmadı.

**Faz sırası bu karardan çıkıyor:** (1) yönlendirme + 11 metot + bayrak,
(2) değiştirici kodlaması, (3) Cmd izin listesi, (4) sürükleme. Faz 1 girdi
kaydının imzasını da (hangi değiştirici bayrakları `encode_key`'e girecek)
**baştan** tanımlıyor; faz 2'ye kalırsa faz 1'in testlerini faz 2 yeniden
yazar.

## Karar 2: Option hangi diziyi gönderir?

Kabuğun bütün metakarakterleri Option kombinasyonunda (`~ ^ | \ { } [ ] $ @ #`;
`context.md` → Kanıt a). Yani "Option = Meta" tek bir anahtar olsaydı Türkçe
Q'da kabuk **yazılamaz** hâle gelirdi: `{` = Option+7 ve o tuş `\e7` gönderirdi.

Ama kullanıcının şikâyeti (Option+oklar, Option+Delete) bu çatışmanın
**içinde değil**: Option+ok hiçbir düzende basılabilir karakter üretmiyor,
`characters`'ı AppKit'in fonksiyon tuşu kodu (U+F702…). İki sınıf:

| tuş sınıfı | çatışma | çözüm |
|---|---|---|
| Option + gezinme/silme | **yok** | koşulsuz Meta kodlaması, ayar sorulmaz |
| Option + basılabilir harf (`Option+b` → `∫`) | **var** | karakter kazanır (bugünkü davranış) |

**Diziler ölçüldü** (`zsh -f -c 'bindkey -e; bindkey -L'`, zsh 5.9, 2026-09-20 —
kullanıcının rc dosyası okunmadan, yani varsayılan zsh):

| tuş | dizi | varsayılan zsh'te |
|---|---|---|
| Option+← | `\eb` | `backward-word` ✅ |
| Option+→ | `\ef` | `forward-word` ✅ |
| Option+Delete | `\e\x7f` | `backward-kill-word` ✅ |
| ~~Option+ok~~ | ~~`\e[1;3D`~~ | `^[[1;*` → **hiç bağlama yok** ❌ |

Son satır planın ilk hâlinde vardı ve **yanlıştı**: değiştiricili ok dizisi
xterm'in sözleşmesi ama zsh onu varsayılanda tanımıyor, yani o diziyle set
motivasyonunu kapatmadan kapanırdı — şikâyet aynen durur, üstelik ayarsız
geldiği için kullanıcı belirtiyi hiçbir anahtara bağlayamaz. Yol haritasının
satırı (`\eb`, `\e\x7f` — "zsh onları zaten biliyor") baştan doğruydu.

**Home/End `bt-core`'un kararı, `keys.rs`'in literali değil.** Dizisi DECCKM'e
bağlı (`\e[H` / `\eOH`) ve bu hata bir kez yapıldı: ok baytı koşulsuz `\e[A`
yazılmıştı, `less` DECCKM'i açıp `\eOA` bekliyordu, çare `KeyInput::Arrow` ile
kararı `bt-core`'a taşımak oldu (`arrows_are_keys_not_bytes` o hatanın kaydı).
Home/End aynı yoldan gelir. **Kazanç hanesine yazılmıyor:** ölçüm zsh'te
`^[[H`/`^[[F`/`^[OH`/`^[OF` için **sıfır** bağlama gösterdi — yani Home/End
terminalin uygulamaya borcu (less, vim, terminfo `khome`/`kend`), kabuğun
satır başına gitme tuşu değil. Kabukta satır başı/sonu bugün de `^A`/`^E`
ile çalışıyor.

## Karar 3: Cmd'nin izin listesi — tek tuş

`reaches_terminal` bugün Command'lı **her** tuşu kesiyor ve bu doğru
varsayılan. Kullanıcının istediği Cmd+Delete bu kapıyı adı konmuş bir listeyle
açmayı gerektiriyor.

**Liste tek tuş: Cmd+Delete → `\x15`.** zsh'te `^U` = `kill-whole-line`, yani
satırın tamamı gider.

Bu, macOS'un katı anlamından (satır **başına kadar** sil) sapıyor ve sapma
bilinçli: ölçüm gösterdi ki zsh'te `backward-kill-line` varsayılanda **hiç
bağlı değil**, yani macOS semantiğini birebir vermenin yolu yok. Kullanıcının
kendi cümlesi de ölçütü veriyor — "Cmd+Delete **satırı silmiyor**" — yani
beklenti satırın gitmesi ve `^U` tam onu yapıyor.

**Cmd+←/→ listeye girmiyor.** İstenmedi; satır başı/sonuna üçüncü bir yol
olurdu (`^A`/`^E` bugün çalışıyor, Home/End bu setin zaten kapattığı borç) ve
ölçüm onların zsh'te karşılıksız olduğunu gösterdi. Liste **kapalı** kalır:
menüde karşılığı olmayan başka Cmd'li tuş yine yutulur, yoksa bir gün Cmd-T
kabuğa `t` yazar.

## Karar 4: Sürükleme — yalnız dosya URL'si

**Kapsam: `NSPasteboardTypeFileURL`.** Düz metin damlası planın ilk hâlinde
"bir satır ek maliyet" diye geçiyordu; değil — dosya yolu ters bölüyle
**kaçmalı**, sürüklenen metin **harfi harfine** gitmeli, yani kaçış kuralı
tipe koşullu hâle gelir. Üstüne Finder tek sürüklemede panoya hem
`public.file-url` hem bir dizge koyuyor, yani kolların sorulma **sırası** da
bir karar olur ve yanlış sıra kaçmamış bir yol üretir. Tek tip, tek kural,
sıra sorusu yok.

**Kaçış:** Terminal.app paritesi — ters bölü (`/Users/…/İki\ Kelime/a.txt`),
çok dosya boşlukla ayrılır. Kaçacak küme boşlukla bitmiyor: kabuğun
metakarakterlerinin tamamı.

**Tesisat gerçekten iki metot:** `NSDraggingDestination`'ın **bütün** metotları
`#[optional]`, yani `draggingEntered:` + `performDragOperation:` yeter
(+ `NSDragging` feature'ı). `NSTextInputClient`'ın tersi.

**Çıkış: `Session::paste`.** Bracketed paste ve dock istisnası bedavaya
geliyor; `can_be_typed`'ın ham dalı ters bölülü yolu sorunsuz geçiriyor (`\`
kontrol karakteri değil). Tek dosyalık damla dock satırına "yazılmış gibi"
girer ve bu **doğru** davranış: kullanıcı damlayı yazdığı satırın devamı
olarak görüyor.

**Kapsam dışı, adıyla:** damlama sırasında görsel geri bildirim (yeni çizim
yüzeyi ister); Cmd'li damlanın `cd` olması; damlayı uygulama ikonuna bırakıp
yeni pencere açmak.

## Kapsam dışı — adıyla

- **Tam IME** (CJK, altı çizili preedit'in çizilmesi) — `bt-gpu`/`Frame` işi,
  yol haritasında kendi borcu. Bu set `insertText:`'i getirir, preedit'i
  çizmez.
- **`[keyboard]` ayar bölümü ve Option kipleri** — panelde düştü, gerekçe
  aşağıda. İlk "Meta istiyorum" isteğinde açılır.
- **kitty klavye protokolü / CSI u** — `keys.rs`'in doc'unda zaten kapsam dışı.
- **Odak raporu (DEC 1004)**, **Secure Keyboard Entry** — ayrı borçlar.
- **Emoji/karakter paleti bir kazanç değil** (kullanıcı kararı, 2026-09-20):
  `insertText:` girişini açar ama glyph **çizilmez**, atlas `R8Unorm` ve emoji
  020'nin işi. Sıra değişmiyor.

## Muhakeme (2026-09-20)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de yaklaşımı (A) onayladı; üçü de **yükün eksik sayıldığını** ve en riskli
dikişin kılavuzsuz kaldığını söyledi. Hiçbiri `KIRMIZI` değil, yani tasarım
değişmiyor — kapsam **daralıyor** ve mekanizma **adlanıyor**.

**Kabul edilen itirazlar → plan değişikliği:**

- `[keyboard]` bölümü + `left_option`/`right_option` + "meta" kolu **çıktı**.
  Üç gerekçe birleşti: (i) Karar 2 kararı iki sınıfa bölünce ayarın kapsamı
  yalnız Option+harfe indi ve **varsayılanı bugünkü davranış** — yani set
  teslim edildiği gün ayardan hiçbir kullanıcı davranışı çıkmıyor; (ii) tek
  tüketicisi hipotetik, isteği getiren Türkçe-Q kullanıcısı için `"meta"`
  değeri `{`'i yazılamaz yapıyor; (iii) `"meta"` kolu setin en riskli
  hamlesinin yanına **varsayılanda hiç koşmayan ikinci bir yönlendirme kolu**
  koyuyor ve o kol o Option tarafındaki ölü tuşu öldürüyor. Emsal deponun
  kendisi: `[shell] prompt` tüketicisi olmadığı için emekli edildi, `cursor`
  rolü gerçek istek gelince ayrıldı.
- **`\e[1;3D` → `\eb`/`\ef`/`\e\x7f`.** Ölçüldü: değiştiricili ok dizisinin
  varsayılan zsh'te hiç bağlaması yok. Planın ilk tablosu setin motivasyonunu
  kapatmayacak bir dizi seçmişti.
- **`doCommandBySelector:` gövdesiz kalamaz.** Plan yalnız "harf iki yoldan
  gider" riskini görüyordu; tersi (Enter/Tab/Esc/Ctrl-harf hiç gitmemesi ve
  bip) yazılı değildi.
- **Yeniden giriş bayrağı** mekanizma olarak yazıldı; "metin şuraya, kontrol
  dizisi buraya" bir hedef durumdu, ölçüt değildi.
- **11 zorunlu metot + feature + `define_class!` paniği.** "İki metot" iddiası
  yanlıştı ve A-vs-B karşılaştırmasının üstünde duruyordu.
- **`setMarkedText:` stub → asgari durum ivar'ı.** "Boş bırakılırsa bileşim
  tamamlanır" ölçülmemişti.
- **`firstRectForCharacterRange:`** sıfır dikdörtgen dönmüyor ama hassas da
  değil: kapsam içinde tüketicisi yok (ölü tuş önizlemesi marked text, aday
  penceresi CJK'nın). İmleç hücresini crate sınırı ötesinden taşımak sete
  **girmedi** — jürilerin bunu press-and-hold'a bağlaması yanlıştı ve o
  bağlamadan şişen faz 1 böylece küçüldü.
- **Press-and-hold artı hanesinden çıktı ve kapatılacak bir kalem oldu.** Üç
  jüri onu A'nın kazancı sayıyordu; terminalde aksan popover'ı **tuş
  yinelemesini** çalıyor (vim'de basılı `j`). Ölçüm kurulu bir üründen geldi:
  iTerm2 kendi domain'inde `ApplePressAndHoldEnabled = 0` yazıyor. Çare
  uygulamanın kendi `registerDefaults`'ı — kullanıcının plist'i yine
  dokunulmaz.
- **Bayrağın adı `inserted` → `consumed`** ve `setMarkedText:` de set ediyor:
  değişmez "yığın bu olayı aldı". Ölü tuşun ilk vuruşunda `characters`'ın boş
  olması bugün fallback'i zararsız kılıyor, ama o bir tesadüf ve değişmez
  tesadüfe yazılmaz.
- **Home/End `bt-core`'un kararı** (`Arrow` emsali, DECCKM); ve zsh'te
  karşılığı olmadığı için kazanç hanesine yazılmıyor.
- **Cmd izin listesi üç tuştan bire indi**; Cmd+Delete'in `^U` sapması
  gerekçesiyle kayda geçti.
- **Sürüklemede düz metin çıktı**; kaçış kuralı tek tip kaldı.
- **Faz 1'in kabul ölçütü "bugünkü tuş kümesinde sıfır regresyon"** ve
  checklist'ine elle basılacak tuş listesi giriyor (Enter, Tab, Esc,
  Backspace, oklar, ^A/^C/^D/^E/^K/^U/^W, Shift+Tab, fn+Backspace,
  Shift+PgUp). Sebebi ölçülü: `keys.rs`'in dokuz sınaması `encode_key`'in ne
  **döndürdüğünü** çiviliyor, hangi olayın ona **ulaştığını** değil — Enter
  yönlendiriciye kaçsa da yeşil kalırlar. `make duman` da klavyeye kör (tuş
  sentezi yok), o yüzden phase'lerin `## Doğrulama`'sı bunu açıkça söylüyor.
- **Geri alma birimi faz 2'den sonra commit değil `set`** — `teslim.md`'ye
  yazılacak.
- `context.md`'deki "dört bölüm" sayımı yanlıştı: altı bölüm var
  (`[terminal] [appearance] [font] [clipboard] [motion] [shell]`). Karar 5
  düştüğü için cümle de düştü.

**Reddedilenler:**

- *"Option=meta `charactersIgnoringModifiers` ister, faz 1 girdi kaydını ona
  göre kursun."* — `"meta"` kolu tamamen çıktığı için gerekçesi kalmadı. Girdi
  kaydı yine faz 1'de tanımlanıyor ama ihtiyacı **değiştirici bayrakları**;
  `charactersIgnoringModifiers` yol haritasındaki ayrı borcun (Ctrl+Shift+Tab)
  adayı olarak yerinde duruyor.
- *"Sürükleme ayrı sete çıksın."* — tek dosya + tek saf fonksiyon; ayrı set
  süreç maliyetini artırır, faz olarak kalıyor.

## Muhakeme — 2. tur (2026-09-20)

Birinci tur `discussion.md`'nin seçeneklerini sınadı; bu tur **onaylanmış
`plan.md`'yi** sınadı.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de yaklaşımı ve kapsam daralmasını onayladı, hiçbiri `KIRMIZI` değil:
tasarımda kusur yok, **çıkarılacak kilo** ve **bağlanmamış şekil** var.

**İki mercek çatıştı, ölçüm ayırdı.** Sadelik "R3.2 (Home/End) setten çıksın"
dedi; Codebase-fit "çıkmasın ama şekli bağlansın" dedi. Codebase-fit'in kendi
kanıtı birinciyi haklı çıkardı: `bt_core::Arrow`'un doc'u değişmezi yazıyor
("Ok tuşu: klavyenin dördü, tekerleğin ikisi… DECCKM sorusu depoda tek yerde")
ve tekerlek onu `session.rs:2444`'te gerçekten kuruyor, yani Home/End `Arrow`'a
**eklenemez**. Kalan şekil yeni bir `pub enum` + yeni `Session` metodu +
`KeyInput`'a üçüncü varyant + iki kip için sınamalar — yani "bedavaya geliyor"
yanlıştı. Kullanıcının istemediği, hedef cümlesinde olmayan ve zsh'te karşılığı
**sıfır** ölçülen bir tuş için setin **tek** `bt-core` `pub` sınır
genişlemesini ödemek yanlış takas. **R3.2 çıktı**, şekli
`docs/YOL-HARITASI.md`'nin borç satırına taşındı.

**Kabul edilen itirazlar → plan değişikliği:**

- **R3.2 (Home/End) çıktı** (yukarıda). Çıkarmak güvenli: Home
  `doCommandBySelector:`'a düşer, no-op'tan geçer, `encode_key` bugünkü gibi
  yutar — davranış bit bit aynı, `keys.rs:212`'nin çivisi de yerinde kalır.
- **Faz 3 faz 2'ye katlandı (4 → 3 faz).** Cmd bir değiştirici bayrağı ve
  `` aynı tablonun bir satırı; dokunduğu yüzey `view.rs:160` + iki
  assertion, yani `duzen.md` → Ek phase eşiği'nin ölçüsü. İki dokunuş olduğu
  plana yazıldı (router'ın Cmd kolu + encoder'ın Option satırları).
- **R1.5 → R4.2.** Yanlış ebeveynin kanıtı planın kendi metnindeydi
  (`plan.md:24` "R3'ün listesi" diyordu, oysa liste R4'ün). Cmd'li olayın
  yığına girmemesi bağımsız yüklem değil, izin listesinin gerekçesi.
- **Girdi kaydı faz 2'ye indi.** Faz 1'de tanımlanmasının gerekçesi boştu:
  faz 1 imzaya dokunmazsa `keys.rs`'in dokuz sınamasından hiçbirini
  değiştirmiyor. Kaydı tanımlayan faz onu tüketen faz oldu.
- **`insertText:`/`setMarkedText:` argümanı `&AnyObject`** (doc: "`string`
  should be of the correct type"), `NSString` değil. Tek downcast kuralı
  yazıldı (R1.3) — birinci turun "11 zorunlu metot" sürprizinin aynı sınıfı.
- **`NSDragging` feature'ı plana girdi**; `NSUserDefaults`'ın feature
  **istemediği** (Foundation'ın varsayılan setinde) ve `Cargo.lock`'un
  değişmediği aynı yerde yazıldı.
- **Sürüklemenin okuma API'si seçildi:** `readObjectsForClasses:` +
  `NSURL::class()` (ek feature yok; `pasteboardItems()` `NSPasteboardItem`
  isterdi). Yol `NSURL.path`'ten — yüzde çözme ikinci kez yazılmıyor.
- **R4.3 doğdu:** `reaches_terminal` tuşun kimliğini öğrenir ve
  `command_keys_never_reach_the_terminal` tek istisnayla yeniden yazılır.
- **R1.2'ye dördüncü kol: Control'lü olay yığına girmez.** İşletme merceğinin
  tablosu gerekçeyi verdi — numpad Enter (U+0003, Ctrl'süz) ve Ctrl+Y
  (U+0019'u Shift+Tab ile paylaşıyor) AppKit'in kolu seçmesine bırakılamaz;
  numpad Enter yanlış kola düşerse **her komut kesilir**. Yan kazanç:
  Ctrl+Shift+Tab ve Ctrl+numpad Enter borçları bugünkü hâllerinde kalıyor ve
  `## Kapsam Dışı`'da adıyla duruyor.
- **R1.7'nin listesi yeniden türetildi.** Ölçüt "encode_key ne tanıyor" değil,
  **"doğruluğu artık AppKit'in hangi kolu seçtiğine bağlı olan tuşlar"**;
  dört kalem eklendi (numpad Enter, Ctrl+Shift+harf, Ctrl+Y, düz çok baytlı
  harf) ve dördü de yeşil kalacak bir sınamayla eşleşiyor.
- **Faz 1 riskli phase kutusunu tutar** → `/code-review` faz 1 sonunda.
  Mekanik tetikleyici yok ama faz 1 kapının yazılı gerekçesinin tanımı.
  Yanında ikinci bulgu: `make hepsi` R1.1'in arıza kipini **göremiyor** —
  hiçbir sınama `BateriView` üretmiyor, yani protokol assertion'ı ilk kez
  gerçek pencerede patlar ve tek uyum kapısı `make duman`.
- **`## Doğrulama` bölümü doğdu.** Plan hiçbir `make` hedefini ve "elle"
  kelimesini içermiyordu; şimdi tablo hâlinde, elle turun en az üç oturum
  olduğu yazılı.
- **Geri alma birimi `## Göç`'e bir cümle oldu** (faz 2'den sonra commit
  değil set), aynısı faz 2'nin `## Yayın Etkisi`'ne gidecek.
- **R2'nin kazanç listesi** faz 1'in `## Kabul`'üne giriyor; R2 hermetik
  kapanamaz.
- **R1.6 ölçüm borcu yazıldı:** bellek içi registration domain'in popover'ı
  bastırdığı ölçülmedi (iTerm2'nin kanıtı kalıcı domain).
- **R6'nın listesi genişledi:** `keys.rs`'in kapsam-dışı satırı eksilmiyor
  **bölünüyor**; `docs/YOL-HARITASI.md`'nin "Klavye kalanları" maddesi tek
  satıra iniyor.
- **Shift+PgUp kolunda `scroll_page` `None` dönerse** ne olacağı akış
  şemasında adı kondu (bugünkü yol: `encode_key`).

**Reddedilenler:**

- *"`docs/YOL-HARITASI.md`'nin 'Home/End bilerek yutuluyor' satırı düşer."* —
  İşletme merceği bunu R3.2'nin kalacağını varsayarak yazdı; R3.2 çıkınca
  Home/End yutulmaya **devam ediyor** ve satır doğruluğunu koruyor.
- *Codebase-fit'in R3.2 şekli* — plana girmedi (gereksinim çıktı), ama
  **kaybolmadı**: yol haritasının borç satırına taşındı, böylece o iş
  yapıldığında `Arrow`'un değişmezi ikinci kez keşfedilmez.
- *"R6 taşıma yük."* — Sadelik merceği kendi itirazını geri aldı: 017'nin
  R6'sı birebir aynı şekilde, yani ev üslubu.

## Karar (2026-09-20, kullanıcı onayı)

- **Seçilen: Seçenek A** — `NSTextInputClient` + `interpretKeyEvents:`, üstüne
  `consumed` bayrağıyla yeniden giriş arbitrajı. Gerekçe: ölü tuş bileşimini
  AppKit'in metin yığını dışında tamamlamanın yolu yok (Kanıt b) ve düzen
  verisini sahiplenmek kalıcı borç olurdu.
- **Seçilen: Option iki sınıfa ayrılıyor** — gezinme/silme tuşları koşulsuz
  Meta dizisi (`\eb`, `\ef`, `\e\x7f`; ölçüldü), basılabilir harf karakter
  üretmeye devam ediyor. Kullanıcının şikâyeti böylece **ayarsız** kapanıyor.
- **Seçilen: Cmd izin listesi tek tuş** — Cmd+Delete → `\x15`. macOS'un katı
  anlamından sapıyor; ölçüm zsh'te karşılığının bağlı olmadığını gösterdi ve
  kullanıcının kendi cümlesi ("satırı silmiyor") beklentiyi satırın gitmesi
  olarak veriyor.
- **Seçilen: sürükleme yalnız dosya URL'si**, Terminal.app paritesiyle ters
  bölü kaçışı, çıkış `Session::paste`.
- **Reddedilen: `[keyboard]` bölümü + `left_option`/`right_option` + "meta"
  kolu** — varsayılanı bugünkü davranış olduğu için teslim günü hiçbir
  davranış üretmiyor, tek tüketicisi hipotetik ve `keyDown:`'a varsayılanda
  hiç koşmayan ikinci bir yönlendirme kolu ekliyor. İlk "Meta istiyorum"
  isteğinde açılır.
- **Reddedilen: Cmd+←/→** — istenmedi, `^A`/`^E` çalışıyor, ölçüm zsh'te
  karşılıksız olduğunu gösterdi.
- **Reddedilen: metin/URL damlası** — kaçış kuralını tipe koşullu yapıyor ve
  Finder'ın tek damlada iki tip koyması kolların sırasını bir karara
  çeviriyor.
- **Reddedilen: Seçenek B** (kendi `UCKeyTranslate` yolumuz) — karakter paleti
  ve IME'yi kalıcı olarak kapatır, düzen verisini sahiplenmeyi borç yapar.
