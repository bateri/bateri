# Tıklanabilir bağlantılar — Tartışma

Karar-listesi biçimi: birbirinden bağımsız karar noktaları. Kullanıcı
beklentisinin emsali iTerm2 / Ghostty / Terminal.app / kitty / WezTerm / VS
Code terminali; bariz beklenti soru yapılmadı, varsayılan oldu.

## Karar 1: Algılama motoru

→ ✅ B (el yazması saf tarayıcı; mantıksal satır `search::wraps`/`WRAP_REACH` ile — Muhakeme).

Düz metindeki URL ve yol nasıl bulunuyor?

- **A — alacritty'nin `RegexSearch`'ü** (aramanın motoru, yeni kenar yok).
  Sarılmış satırı kendisi yürüyor. Ama: sondaki noktalama (`https://x.dev).`),
  parantez dengesi (`(bkz. https://tr.wikipedia.org/wiki/X_(Y))`) ve
  `:satır:sütun` ayrımı regex'le kurulamıyor, bir el yazması son işlem yine
  gerekiyor; dock'un metni `Term`'de olmadığı için dock'a **ikinci** bir motor
  gerekiyor — iki aritmetik ayrıştığı gün bağlantı bir yüzeyde var ötekinde
  yok (024/035'in "tek yürüyüş" kuralının tersi).
- **B — el yazması tarayıcı, saf fonksiyon** (`bt-core::link::scan(&str)`).
  Bir mantıksal satırın dizgisini alır, `(başlangıç, bitiş, tür)` aralıkları
  verir: şema öneki (`http://`, `https://`, `ftp://`, `mailto:`, `file://`)
  + izinli karakter koşusu, sondaki noktalamayı kırpma ve parantez/köşeli
  parantez dengesi; yol adayı ve `:satır(:sütun)` soneki. Üç yüzey (ızgara,
  bant, dock) aynı fonksiyonu çağırıyor; ızgaranın dizgisi `Term` kilidi
  altında mantıksal satırdan kuruluyor (hücre ↔ karakter eşlemesiyle,
  `cluster::Walk`'ın kümesi ve geniş karakterin spacer'ı atlanarak).
- **C — `regex-automata`'yı doğrudan bağımlılık yapmak.** Grafta var
  (alacritty), yalnız bir kenar. A'nın son işlem ve iki yüzey sorununu
  çözmüyor, 033 Karar 11'in kaçındığı kenarı ekliyor.

**Öneri: B.** Yeni kenar yok; üç yüzey tek fonksiyondan; kurallar (noktalama,
parantez, sonek) regex'in ifade edemediği şeyler ve zaten el yazması
olacaktı. Tarama **kare yolunda değil**: yalnız ⌘ basılıyken fare hücre
değiştirdiğinde ve tıklamada koşuyor.

## Karar 2: OSC 8 bağlantısının kapsamı

→ ✅ Öneri olduğu gibi.

- Bağlantının kapsamı, imlecin altındaki hücreyle **aynı `Hyperlink`'i**
  (id + uri, alacritty'nin eşitliği) taşıyan, mantıksal satırda (sarma
  boyunca) bitişik hücre koşusu. Aynı id'yi taşıyan uzak hücreler (ekranın
  başka yerinde) vurgulanmıyor — kapsam seçimi, sınır değil; bugün bunu
  kullanan yaygın bir üretici yok.
- OSC 8 aynı hücrede metin taramasını **yener** (görünen metin hedef değil).
- `bateri://` önekli bağlantı **yok sayılır**, hücre bağlantısızmış gibi —
  yani blok çıpasının altındaki komut satırında yazılı bir URL yine düz
  metin olarak bulunur, `bateri://block/N` hiç vurgulanmaz, hiç açılmaz.
  Herhangi bir yoldan `bateri://` hedefi açılmak istenirse `NSWorkspace`'e
  gitmeden yutulur (038 Karar 7).

**Öneri:** yukarıdaki üçü.

## Karar 3: Vurgunun görünümü ve çizim yolu

→ ✅ A, ama damga `row_identity` değil `LedgerMark` (+ OSC 8'de `Hyperlink`), bayat yuva düşüp yeniden bulunuyor; ezme üç sink'te tek yardımcıdan (Muhakeme).

Görünüm (emsal): ⌘ basılı + fare bağlantının üstünde → bağlantının bütün
hücreleri (sarılmışsa iki satırda) **düz alt çizgi**, metnin kendi renginde,
imleç el. ⌘ bırakılınca ya da fare çıkınca vurgu kalkar. **OSC 8** bağlantısı
⌘'siz hover'da **kesikli** alt çizgi alır (kitty/WezTerm/VS Code: hedefi
olan metnin tıklanabilir olduğunu söyler), dinlenme hâlinde süssüz kalır
(`ls --hyperlink`'in her satırı çizgili olmasın). Düz metin bağlantısı ⌘'siz
hiç vurgulanmaz (her `ls` kelimesi yol adayı).

Çizim yolu:

- **A — `frame()` hücrenin `underline`'ını ezer.** Hover yuvası (yaprak
  kilit, `Theme` örüntüsü: `Term` kilidinden önce kopya) bir aralık taşır;
  sink'e giden hücre aralıktaysa `underline` vurgu stiline, `underline_color`
  `None`'a (metnin rengi) çekilir. Yeni pipeline, shader, tema rolü yok;
  maliyet hover kuruluyken hücre başına bir aralık karşılaştırması, kurulu
  değilken tek dal. Alt çizgisi olan hücrede vurgu süresince vurgunun stili
  kazanır.
- **B — `selection` pipeline'ının üçüncü kullanıcısı** (yol haritasının
  taslağı). Zemin vurgusu: emsalin hiçbiri bağlantıyı zeminle vurgulamıyor
  ve seçimle karışır; ayrıca yeni bir run listesi ve rol ister.

**Öneri: A.** Taslaktaki B'den sapmanın gerekçesi görünüm: emsalin dili alt
çizgi ve o çizim zaten var. Vurgu bayatlamasın diye aralık satırın kimliğiyle
(`session::row_identity`) damgalanır; `frame()` damga tutmuyorsa (çıktı
kaydı, ekran temizlendi) aralığı çizmez ve düşürür — yanlış metnin altını
çizmektense hiç çizmemek. Hover değişimi hasar diker (`Waker::wake`), boşta
kare yok.

## Karar 4: Fare rotasının dördüncü kolu ve ⌘'nin arbitrajı

→ ✅ Davranış öneri gibi; karar `button_route`/`Click`'te değil jest defterinin ön-rotasında (`Gesture::pressed_link`, `Release::Link` — Muhakeme).

- ⌘ basılıyken bağlantıya basış **her kipte** bağlantının jesti: fare kipi
  açıkken (vim, htop, Claude Code) uygulamaya **ne basış ne bırakma raporu**
  gider; kip kapalıyken seçim başlamaz. iTerm2 ve Ghostty'nin davranışı.
  ⌘ + bağlantısız hücre bugünkü yolundan (rapor ya da seçim) — ⌘ fare
  kipinden kaçış değil, kaçış Shift.
- Karar `bt-core`'da: `input::button_route` bir bağlantı bitini alır ve
  `ButtonRoute::Link` kolu `Report`'tan **önce** gelir; `Click` dördüncü
  varyantı (`Link`) kazanır. Cmd `MouseModifiers`'a girmez (o tip xterm'in
  bitleri), ayrı argüman.
- Jest defteri rotayı basışta kilitler: sürükleme hiçbir şey yapmaz (seçim
  yok, rapor yok), bırakma rapor göndermez. **Açma bırakmada**, fare hâlâ aynı
  bağlantının üstündeyse (macOS düğme kuralı: bastığın yerden kayarak iptal);
  ⌘ bırakmada sorulmaz, basışta kilitli.
- Çift tıklamada ikinci basış yine bağlantı jesti; bir kez açılır (yalnız
  `clickCount == 1`'in bırakması açar).

**Öneri:** yukarıdaki dördü. Bekçi `shift_overrides_the_button_but_not_the_wheel`'in
yanına: "bağlantı her kipte raporu yener, Shift'i de".

## Karar 5: Ne açılır, nasıl açılır

→ ✅ Türler öneri gibi; aşağıdaki tablo **yerini beyaz listeye bıraktı** (bilinen içerik tipi ve `x` bitsiz dosya açılır, kalan her dosya Finder'da gösterilir — `plan.md` → R5.1); kuyruk `bt-shell-macos`'ta, `bt-shell-common`'da saf `resolve`; makine adı `SessionOptions` ile; uzak oturumda hiçbir `file://` yok (Muhakeme).

**Bağlantı türleri:**

- Düz metin URL: `http`, `https`, `ftp`, `mailto`, `file`.
- Düz metin yol: boşlukla/tırnakla ayrılan, noktalaması kırpılmış belirteç,
  `:satır`, `:satır:sütun` ya da `(satır,sütun)` soneki ayrılarak; `~`
  ev dizinine, göreli yol pane'in OSC 7 dizinine çözülür. **Var olmayan yol
  bağlantı değildir** (vurgulanmaz, açılmaz) — iTerm2'nin semantic history
  kuralı; bu sayede `ls`'in çıplak `foo.txt`'si de tıklanabilir, `ab/cd`
  gibi rastgele bir dizgi değil.
- OSC 8: hedefin şeması ne olursa olsun bağlantı (aşağıdaki açma
  politikasıyla).

**Varlık denetimi kare yolunda ve `Term` kilidinde değil**: `bt-core` yalnız
aralığı ve ham metni verir, çözümleme `bt-shell-common`'da (`std::fs`), arka
planda bir dispatch kuyruğunda; sonuç ana kuyruğa nesille döner ve eski
neslin cevabı düşer. Doğrulanana kadar düz metin yol vurgulanmaz (ağ
diskinde takılan bir `stat` ana thread'i ve kareyi kilitlemesin).

**Açma politikası** (saf fonksiyon, `bt-shell-common`):

| hedef | eylem |
|---|---|
| `http`/`https`/`ftp`/`mailto` | varsayılan uygulamada aç (`NSWorkspace`) |
| dizin | Finder'da aç |
| sıradan dosya | varsayılan uygulamasında aç |
| çalıştırılabilir dosya ya da paket (`x` biti, `.app`, `.command`, `.tool`, `.pkg`, …) | **Finder'da göster**, çalıştırma |
| OSC 8 `file://` — yetki boş, `localhost` ya da bu makinenin adı | yol gibi |
| OSC 8 `file://` — yabancı yetki | bağlantı değil |
| OSC 8, başka şema (`vscode://`, `x-man-page://`, `ssh://`, …) | **onay sayfası**: hedefin tamamı + "Open" / "Cancel" (varsayılan ve Esc Cancel) |
| `bateri://` | yutulur |

Gerekçe: http(s)/mailto emsalde sorgusuz ve beklenen; özel şemalar ekrana
bayt basabilen her programın başka bir uygulamayı tetiklemesi demek (VS Code
terminali dış bağlantıda soruyor, iTerm2 ve Ghostty sormuyor — ortada
kalındı: yaygın şemada sorgu yok, kalanında tek tıkla geçilen bir sorgu).
Çalıştırılabilir dosyayı "aç"mak LaunchServices'te onu koşturmak demek;
⌘-tıkla bir betiğin çalışması kimsenin beklediği şey değil.

`satır:sütun` **tanınır ama atlanmaz**: dosya açılır, satıra gitmek bir
editör ayarı ister (iTerm2'nin semantic history komutu) → kapsam dışı.

**Uzak oturum** (036): düz metin yol bağlantı değildir (yerel diskte uzak
yol yok ya da yanlış dosya); URL ve yerel yetkili olmayan `file://` hariç
OSC 8 kalır. Karar `bt-core`'un hit testinde (`DockContext::remote` orada).

**Bilinen sınır (adıyla):** göreli yol **bugünkü** OSC 7 dizinine çözülür,
satırın basıldığı andakine değil; `cd`'den sonra eski çıktıdaki göreli yol
yanlış dizine bakar ve çoğu zaman bulunamayıp vurgulanmaz — yanlışın yönü
güvenli.

## Karar 6: Hangi yüzeylerde

→ ✅ Üç yüzey; nokta `LinkPoint::{Screen { row: i32, col }, Dock}` (negatif satır = bant, `drawn_lines` ile kapılı — Muhakeme).

- **Izgara:** evet.
- **Doldurma bandı:** evet. Satırları seçim ucu olarak temsil edilmiyor
  (`point_to_cell`'in reddi seçimin gerekçesi), ama bağlantının hit testi
  seçim ucu değil: bant satırı defterin belli bir satırı ve `bt-core` onu son
  çizilen `fill_shown`'dan mutlak satıra çeviriyor. Hit testin noktası
  yüzeyi adlandıran bir tip (`LinkPoint::{Grid, Fill, Dock}`).
- **Dock:** evet — yazılan komuttaki URL ya da yol (`open https://…`,
  `vim src/x.rs`). Metin `dock::selectable`'dan, isabet dock'un tek düzen
  yürüyüşünden (`dock_layout`, seçimin yolu). Bağlam satırı (yol + dal)
  bağlantı değil.
- **Alternatif ekran:** ızgara gibi (vim'in içindeki URL ⌘-tıkla açılıyor;
  fare kipinde de, Karar 4).

**Öneri:** üçü de. Dışarıda bırakmak kapsam seçimi olurdu, zorunluluk değil;
kullanıcı bağlantıyı gördüğü her yerde tıklamayı bekler.

## Karar 7: Hedefin gösterimi ve bağlam menüsü

→ ✅ İkisi de kapsamda, son phase.

- **OSC 8 hedefi:** ⌘ ile bir OSC 8 bağlantısının üstündeyken hedef URL
  pane'in sol altında küçük bir AppKit etiketinde görünür (tarayıcının durum
  çubuğu, Ghostty) — metin hedefi söylemediği için tıklamadan önce görmek
  güvenlik. `hitTest → nil`, kare yolunun dışında (odaksız pane örtüsünün
  emsali). Düz metin bağlantıda gösterilmez (metin hedefin kendisi).
- **Sağ tık menüsü:** fare kipi kapalıyken bağlantının üstünde sağ tık →
  `Open Link` / `Copy Link` (yolda `Open` / `Reveal in Finder` / `Copy
  Path`). Fare kipinde sağ tık bugünkü gibi uygulamanın. Bağlantısız yerde
  sağ tık bugünkü gibi hiçbir şey (genel bir bağlam menüsü bu setin konusu
  değil).

**Öneri:** ikisi de kapsamda; ikisi de saf AppKit, `bt-core`'a ve kare
yoluna dokunmuyor.

## Karar 8: Ayar

→ ✅ Ayar yok.

Ayar anahtarı yok: davranış emsalin varsayılanı ve kapatılacak bir şey
değil (iTerm2/Ghostty'de de çoğu kullanıcı hiç değiştirmiyor). Editör
komutu (`satır:sütun`'a atlama) gelecekte bir anahtar olursa ayrı set.

## Muhakeme (2026-10-01)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

**Kabul edilen itirazlar → plan değişikliği:**

- (Codebase-fit 1, İşletme 1) `row_identity` yerinde yeniden yazılan satırı
  görmüyor (vim/htop'un yeniden çizimi, `\r`'lı ilerleme satırı, Claude
  Code'un tazelemesi), yani vurgu yanlış metnin altında asılı kalırdı →
  hover yuvası aramanın bayatlık damgasını taşıyor (`search::LedgerMark`:
  `epoch` her PTY çıktısında ilerliyor, `wipes`, ofset, alternatif ekran,
  boyut); OSC 8 aralığında ayrıca hücrenin `Hyperlink`'i saklanan
  bağlantıyla karşılaştırılıyor. Damga tutmazsa `frame()` vurguyu çizmiyor,
  yuvayı düşürüyor ve `Wake` ile ana kuyruğa haber veriyor; ⌘ hâlâ
  basılıysa view aynı noktada hit testi **yeniden koşuyor** (ürün sorusu:
  "sönsün mü, yeniden mi bulunsun" — kullanıcı tarafı seçildi, vurgu doğru
  yerde kalıyor). Dock'un damgası aynanın `BUFFER`'ı/nesli (`DockWindow`).
- (İşletme 1) ⌘'nin bırakılışı başka uygulamaya giderse vurgu asılı kalırdı →
  `flagsChanged:` + pencerenin key'liği gidince ve uygulama deaktive olunca
  temizleme; her hareket olayında ⌘ `modifierFlags`'tan yeniden okunuyor
  (kendini onaran yol). `set_link_hover` aynı değerde no-op ve uyandırmıyor
  (`set_theme` emsali) — bağlantı içinde gezinmek kare üretmiyor.
- (İşletme 1, Codebase-fit) El imlecinin dikdörtgenleri yükleme
  düğmeleriyle **tek** `resetCursorRects`'te ve tek eşitlik listesinde
  (`sync_cursor_rects`), ikinci bir kaynak değil.
- (Codebase-fit 2) Bant için yeni eşleme kurulmuyor → hit testin noktası
  imzalı ekran satırı (`LinkPoint::Screen { row: i32, col }`, negatif =
  bant; `cover_of`'un aritmetiği), `bt-core` onu `drawn_lines` ile kapılıyor;
  ikinci kol `LinkPoint::Dock`.
- (Codebase-fit 3, İşletme) "Çözümleme `bt-shell-common`'da bir dispatch
  kuyruğunda" `make audit`'i kırardı (`dispatch2` orada yalnız
  `watch/dispatch.rs`'de) → `bt-shell-common` saf `resolve` (stat enjekte
  edilebilir) + açma politikası tablosu; arka plan kuyruğu ve ana kuyruğa
  dönüş `bt-shell-macos`'ta (`pane::RemoteProbe` emsali, `upload`/`uploader`
  ayrımı). Kapanışta bekleyen iş pane'i tutmuyor, kimlikle buluyor
  (`PaneLookup`).
- (Sadelik 3) Ayrı nesil sayacı yok → async dönüşte yalnız "fare hâlâ aynı
  adayın üstünde mi" (görüntünün tuttuğu güncel aday) soruluyor; içerik
  bayatlığının tek sahibi damga ve damga hit test anında alınıp taşınıyor.
- (Sadelik 1) `button_route`'a dördüncü kol ve `Click::Link` gereksiz:
  bağlantı biti ancak görüntünün **doğrulanmış** hover'ından gelebiliyor
  (yolun varlığı `bt-core`'da bilinmiyor), yani `bt-core` kararı yalnız geri
  yansıtırdı → ön-rota jest defterinde (`Gesture::pressed_link`, bırakmada
  `Release::Link`); basış ⌘ + doğrulanmış hover'ın aralığındaysa
  `mouse_button` hiç çağrılmıyor. Dock'a basışın emsali (`pressed_dock`).
  Saf ve sınanıyor, bekçi `bt-shell-common::gesture`'da. Yol haritasının
  "dördüncü kol" taslağı böylece `bt-core`'da değil defterde.
- (İşletme 2) `file://` yetkisi `shell.rs` → `LOCAL_AUTHORITIES`'in bilerek
  reddettiği ad karşılaştırmasını istiyor (GNU `ls --hyperlink`
  `file://$HOSTNAME/…` basıyor) → makine adını `bt-shell-common` okuyor ve
  `SessionOptions` ile geçiriyor (doc'un kendi önerdiği çare,
  `decide_locale` emsali); "yerel mi" tek fonksiyonda, OSC 7 ile ortak.
  Uzak oturumda hiçbir `file://` bağlantı değil (tek anlam).
- (İşletme 3) Çalıştırılabilir kara listesi "…" ile delinir (`.terminal`,
  `.jar`, `.webloc`, `.workflow`, `.mobileconfig`, …) ve projenin emsali beyaz
  liste (`quote::shell_quote`) → **varsayılan "Finder'da göster"**;
  varsayılan uygulamada açma yalnız bilinen içerik tiplerinde (UTType
  uyumu: metin/kaynak kodu, görsel, PDF, ses/görüntü) ve `x` biti olmayan
  dosyada; dizin Finder'da açılıyor. Ürün sorusu (bazı dosyalar "açılmak"
  yerine Finder'da gösterilir) — güvenli yön seçildi.
- (Codebase-fit) Alt çizgi ezmesi üç sink'te (ızgara, bant, `Session::dock`)
  **tek yardımcıdan**; gözle kontrol üç yüzeyi sayıyor.
- (Codebase-fit) Mantıksal satırın kapsamı yeniden yazılmıyor:
  `search::wraps` + `WRAP_REACH`.
- (İşletme, küçük) Adıyla yazılacak sınırlar: doğrulama dönmeden gelen ⌘-tık
  bugünkü yoldan (rapor ya da seçim) gider; seri kuyrukta takılan bir `stat`
  o pane'in sonraki doğrulamalarını bekletir; bırakmada basışta kilitlenen
  aralık karşılaştırılır, hit test yeniden koşmaz.
- (İşletme) Hedef etiketi, bağlam menüsü ve ⌘'siz OSC 8 vurgusu çekirdeğe
  dokunmuyor → ayrı son phase.

**Reddedilenler:**

- (Sadelik 2) ⌘'siz OSC 8 kesikli vurgusunu ve sağ tık menüsünü setten
  çıkarmak — ürün sorusu; kullanıcının talebi bağlam menüsünü adıyla
  andı ve "UI/UX elverişli" dedi, kapsamdaki boşluk kullanıcı lehine
  okunuyor. Maliyetleri ayrı bir son phase'e alındı; çekirdek onlarsız
  tamam.
- (İşletme, basitleştirme) Yolu yalnız tıkta doğrulayıp hover'da adaya göre
  çizmek — var olmayan yolun da altı çizilirdi, emsalin (iTerm2) tersi.
- (Sadelik, not) `bateri://` yutma kolunun bugünkü tasarımda ulaşılamaz
  olması — doğru; kol tek satırlık bir savunma olarak kalıyor (038 Karar 7
  onu adıyla istiyor) ve bir sınaması var.

## Karar (2026-10-01, otonom akış)

- **Seçilen:**
  - **Karar 1 → B**: el yazması saf tarayıcı `bt-core::link`, üç yüzey tek
    fonksiyon; mantıksal satır `search::wraps`/`WRAP_REACH` ile, hücre ↔
    karakter eşlemesi kümeyi ve geniş spacer'ı atlayarak. Yeni kenar yok.
  - **Karar 2**: OSC 8 kapsamı aynı `Hyperlink`'in mantıksal satırdaki
    bitişik koşusu, metin taramasını yener; `bateri://` bağlantı sayılmaz
    ve açma yolunda yutulur.
  - **Karar 3 → A + Muhakeme**: `frame()`'in iki sink'i ve `Session::dock`
    tek yardımcıyla `underline`'ı eziyor (⌘: `Single`, ⌘'siz OSC 8:
    `Dashed`, renk metnin); yuva `LedgerMark` (+ OSC 8'de `Hyperlink`)
    damgalı, tutmazsa düşüp ana kuyruğa haber veriyor ve view yeniden
    buluyor; aynı değerde no-op.
  - **Karar 4 → Muhakeme**: ön-rota jest defterinde; ⌘ + doğrulanmış
    bağlantı her kipte raporu ve Shift'i yener, sürükleme hiçbir şey yapmaz,
    açma bırakmada ve yalnız aynı bağlantının üstünde, çift tıklamada bir kez.
  - **Karar 5 → Muhakeme**: URL'ler (`http`/`https`/`ftp`/`mailto`/`file`),
    var olan yollar (`~`, OSC 7 dizini, `:satır:sütun` tanınır ama
    atlanmaz), OSC 8; açma politikası beyaz listeli (bilinen tip açılır,
    kalanı Finder'da gösterilir, dizin Finder'da açılır), OSC 8'in yaygın
    olmayan şemasında onay sayfası; uzak oturumda yol ve `file://` yok;
    makine adı `SessionOptions` ile.
  - **Karar 6**: ızgara, bant (imzalı ekran satırı) ve dock; alternatif
    ekran dahil.
  - **Karar 7**: OSC 8 hedef etiketi ve bağlantı üstünde sağ tık menüsü,
    son phase.
  - **Karar 8**: ayar anahtarı yok.
- **Reddedilen:** `RegexSearch` (dock'u göremiyor, son işlem yine el
  yazması, iki motor ayrışır); doğrudan `regex-automata` kenarı (033 Karar
  11'in kaçındığı kenar, sorunları çözmüyor); `selection` pipeline'ıyla
  zemin vurgusu (emsalin dili alt çizgi); `row_identity` damgası (yerinde
  yazımı görmüyor); `button_route`/`Click`'e dördüncü kol (kararı yalnız
  yansıtırdı); çalıştırılabilir kara listesi (delinir); ad karşılaştırmasını
  `bt-core`'a `gethostname` ile koymak (platform kenarı).
