# TerminalPane ayrımı ve bölmeler — Tartışma

Karar-listesi biçimi. Karar 1 ve 13 kullanıcının (2026-09-27); kalanı
teknik ya da emsalden okunan UX kararı ve gerekçesiyle burada.

## Karar 1: Sınır — pane neyi dışarı verir → ✅ tek `NSView` + olay geri bildirimi + eylem API'si

Kullanıcı onayladı (2026-09-27). Sahip (bugün `TerminalWindow`, yarın bir
workspace uygulaması) pane'i bir `NSView` olarak görünüm ağacına takar; pane
olaylarını bir arayüzden geri bildirir (Karar 3) ve menünün karşıladığı her
iş pane üstünde bir yöntemdir — menü seçicisi o yöntemi çağıran ince bir
sarmalayıcı.

## Karar 2: Pane'in tipi — `NSView` alt sınıfı mı, `NSObject` denetleyici + view mi → ✅ `NSView` alt sınıfı

`TerminalPane` `define_class!` ile bir `NSView` alt sınıfı ve bugünkü içerik
kapsayıcısının (window.rs:1227, 033 → R4.1) ta kendisi: `BateriView` ve
arama paneli onun çocukları, ivar'lar onda.

Ayıran şey responder zinciri: `NSView`'ın `nextResponder`'ı üst view, yani
zincir `BateriView` → **pane** → pencere → delegate. Pane düzeyindeki
seçiciler (punto, bul, temizle, kaydır, `cancelUpload:`) pane'de
uygulanınca hedefsiz menü öğesi onlara odaktaki pane'den kendiliğinden
varıyor; arama alanının alan düzenleyicisi de üst view'lardan pane'e
çıkıyor. 033'ün bu seçicileri pencere delegesine koyma gerekçesi ("alan
odaktayken zincir `BateriView`'dan geçmiyor") böylece konusuz kalıyor.
`BateriView`'ın iki ayrı sahip araması (view.rs:1803 `window_owning`'in
doğrusal araması, view.rs:1491 delegate downcast'i) tek bir `superview()`
downcast'ine iniyor. Gömülen bir sahip de menüsünü aynı zincirden bedava
alır.

Reddedilen: `NSObject` denetleyici + `view()` — zincire girmek için
seçicileri yine bir view'a ya da delegeye yönlendirmek gerekir, yani bugünkü
dolambaç ikinci bir nesneye taşınır.

## Karar 3: Sahip arayüzü — girdiler ve olaylar → ✅ girdi paket, olay `PaneHost`

- **Girdiler doğumda paket** (`PaneLaunch` gibi tek yapı): ayar anlık
  görüntüsü ve tema, süreli koşu tarifi (`Run`), ölçüm defteri (`Stats`),
  entegrasyon ortamı ve dock payı (bugün `app.shell_integration()`'ın
  döndürdüğü çift), kimlik (`TabId`), başlangıç dizini ve ilk girdi
  (`Launch`), çözülmüş hareket bayrakları (Hareketi Azalt, yumuşak
  kaydırma). Canlı değişim bugünkü `set_*` yöntemleriyle (Karar 1'in eylem
  API'si), yani `reload_settings`'in dağıtımı pencere başına değil pane
  başına varır. Pane'in içinde `app::delegate(…)` ve `app.settings()` kalmaz:
  ayar dosyasını okuyan ve izleyen sahip.
- **Olaylar `PaneHost` trait'inden**, pane'in doğumda aldığı sahip
  tutamağıyla: başlık/dizin değişti, kabuk çıktı (pane kapanmalı), yükleme
  durumu değişti (Dock simgesi), bildirim isteği, alt başlık tanısı
  (`post_notices`), OSC 52 kopyası (`copy_to_clipboard`; varsayılan kolu genel
  panoya yazar — pano bir sahip kararı olarak ayrılabilir durur). Sayfalar ve
  popover pane view'ının kendi `window()`'unu kullanır; bunun için sahibe
  sorulmaz.
- **Okuyucu thread'den dönüşler kimlikle kalıyor**: `Send` bir `u64`, çember
  açmıyor (alternatif ekran habercisinin gerekçesi, window.rs:340). Değişen
  yalnız çözüm: `app.window(id)` yerine pane'i pencerelerin pane'lerinde
  arayan bir `app.pane(id)`.
- Sarmalayıcı betiğin yolu zaten parametre (`child`, `shell_integration_env`
  ortamı hazır veriyor); `bateri://` şeması, bildirimler, OSC 52 panosu ve
  `TERM_PROGRAM` kimliği sahibin tarafında kalıyor.

Reddedilen: pane'in içinde `AppDelegate`'e uzanmayı sürdürmek — gömme hedefini
kapatır ve pencere ile pane arasındaki sınırı yine görünmez kılar.

## Karar 4: Pane ayrı bir crate mi → ✅ hayır, `bt-shell`'de bir modül

Pane `view`, `keys`, `gesture`, `clipboard`, `quote`, `jobs`, `upload`,
`uploader`, `search_bar`, `zoom` ve `child`'ın neredeyse tamamına bağlı; ayrı
crate `bt-shell`'i ikiye bölmek demek ve bugün ikinci bir tüketicisi yok.
Sınırın kendisi (Karar 1–3) çıkarma gününün kapısı: gömme seti geldiğinde
`PaneHost` + `PaneLaunch` + eylem API'si dışarı açılacak yüzeydir. Yeni
bağımlılık yok.

## Karar 5: Renderer pane başına mı, pencere başına paylaşımlı mı → ✅ pane başına

Ayıran kısıt window.rs'in başlığında: atlasın anahtarı ölçek **ve punto**
(026 → Karar 2a). Punto farkı (Cmd +/−/0) bugün sekme başına (026 → Karar 3)
ve bölmelerde odaktaki pane'in (iTerm2 ve Ghostty'nin davranışı; Karar 8'de
yeni bölme onu devralıyor). Pane başına punto, pane başına renderer ister.
Pane'in kendi kendine yeten bir birim olması (Karar 1, gömme) da aynı yönde.
Bellek bedeli ölçülmedi ve iddia edilmiyor; paylaşım gerekirse
`DisplayLink::new` zaten `Rc<Renderer>` alıyor, yani sonradan açılabilir.

## Karar 6: Bölme düzeni — iç içe `NSSplitView` mi, kendi kapsayıcımız mı → ✅ saf ağaç + kendi kapsayıcımız

Bölme, kapatma, yöne göre gezinme, klavyeyle boyutlama, eşitleme ve büyütme
**ağaç işlemleri**. Ağaç `bt-shell`'de saf bir modül (ikili ağaç, düğümde
eksen + oran; `quote`/`upload`/`zoom` emsali, sınamalı): çerçeveleri,
komşuyu ve sınırları o hesaplıyor. AppKit parçası yalnız pane'leri o
çerçevelere oturtan, ayırıcıları çizen ve ayırıcı sürüklemesini ağaca
yazan düz bir kapsayıcı `NSView`.

Reddedilen: iç içe `NSSplitView` — bedava gelen tek şey sürükleme; her klavye
işlemi yine her düzeyde `setPosition` hesabı, iç içe bölmenin boyutlanması
tutma önceliklerine bağlı ve ayırıcı rengi bir alt sınıf istiyor. Ağaç yine
gerekiyor (gezinme ve büyütme için), üstüne ikinci bir durum kaynağı ekleniyor.

## Karar 7: Odağın görsel dili → ✅ içi boş caret + soluk örtü + ince ayırıcı

"Odak iki bit" `bt-gpu` değişmeden taşınıyor: pencerenin key biti bütün
pane'lerin link'ine (`set_focused`), klavye biti her `BateriView`'ın kendi
first responder kancalarından (`set_keyboard_in_terminal`) iniyor. Sonuç:
odakta olmayan pane'in caret'i **içi boş** ve blink durur (bugünkü
`cursor_unfocused` yolu), seçim ve arama vurgusu **solmaz** — o soluma
pencerenin key olmadığı hâlin sinyali ve bölmede pencere hâlâ key.

Üstüne **odakta olmayan pane'ler hafif soluklaşıyor** (iTerm2'nin "Dim
inactive split panes"i ve Ghostty'nin `unfocused-split-opacity`'si, ikisinde
de varsayılan açık): tema zemininin yarı saydam bir örtüsü, pane'in içinde
Metal katmanının kardeşi bir AppKit view (arama panelinin örüntüsü),
`hitTest` → `nil`. Kare yoluna girmiyor, `bt-gpu`'ya uniform ya da pipeline
eklenmiyor — örtü CoreAnimation'ın bileşimi, boşta sıfır kare korunuyor.
Oranı bir tasarım sabiti (`GUTTER_PT` emsali) ve gözle kontrolde
ayarlanıyor. Tek pane'de örtü yok.

Ayırıcı bir piksel, rengi temanın `separator` tonu (dock'un saç çizgileriyle
aynı kademe, `Theme::separator_linear`); sürükleme alanı çizilen çizgiden
geniş ve imleci `resizeLeftRight`/`resizeUpDown`.

## Karar 8: Kısayollar ve kapanış anlamı → ✅ Ghostty/iTerm2 emsali

| Eylem | Kısayol | Menü |
|---|---|---|
| Sağa böl | ⌘D | Shell ▸ Split Right |
| Aşağı böl | ⇧⌘D | Shell ▸ Split Down |
| Önceki / sonraki bölme | ⌘[ / ⌘] | Window ▸ Select Previous/Next Split |
| Yöndeki bölmeye geç | ⌥⌘←↑→↓ | Window ▸ Select Split ▸ |
| Boyutla | ⌃⌘←↑→↓ | Window ▸ Resize Split ▸ |
| Eşitle | ⌃⌘= | Window ▸ Equalize Splits |
| Büyüt / geri al | ⇧⌘↩ | Window ▸ Zoom Split |
| Bölmeyi kapat | ⌘W | Shell ▸ Close (tek pane'de "Close Tab") |

Menüdeki taramada (menu.rs) hepsi boş; sekme geçişi ⇧⌘[ / ⇧⌘] ile ayrı.
Menü kısayolları `keyDown:`'dan **önce** çözülüyor (`performKeyEquivalent`),
yani `keyDown:`'ın üç tuşluk kapalı Cmd izin listesi değişmiyor (026 Karar 6
emsali). ⌘W odaktaki pane'i kapatır (koşan iş varsa yalnız onu sorar), son
pane sekmeyi kapatır — tek pane'de bugünkü davranışın ta kendisi. ⇧⌘W ve
kırmızı düğme sekmenin **bütün** pane'lerini kapatır ve soruyu hepsinden
toplar. Kabuk çıkınca yalnız o pane kapanır, odak ağaçtaki komşuya geçer.
Büyütülmüş hâlde bölme, gezinme ya da pane kapanışı büyütmeyi bırakır
(Ghostty'nin davranışı).

## Karar 9: Yeni bölme neyi devralır → ✅ yeni sekmenin kuralı

Odaktaki pane'in OSC 7 dizini, punto farkı ve teması; uzak oturumda ⌘T'nin
kuralı (037 Karar 6): yerel dizinde yerel kabuk, ilk girdisi hedefin satırı
— bölme kendi `Opening` kolu, ⌘T'nin `initial_line`'ından geçiyor. Bölme
eksenindeki alan ikiye eşit bölünür.

## Karar 10: Kimlik ve `bateri://` → ✅ pane başına kimlik, URL pane'i odaklar

`TERM_SESSION_ID` oturumun kimliği (iTerm2'de pane başına); bölmede iki kabuk
aynı kimliği paylaşsaydı onu anahtar alan araç ikisini karıştırırdı. Her pane
kendi `TabId`'sini taşır; `bateri://tab/<id>` pane'i bulur, penceresini öne
getirir ve o pane'i first responder yapar — "URL yalnız odaklar" değişmezi
(038 Karar 5–7) aynen. Değişken ve URL adları 038'in sözleşmesi, değişmiyor.

## Karar 11: Sekme düzeyindeki toplamalar → ✅ odaktaki pane + toplam

- Pencere/sekme başlığı, `⇄` öneki, yükleme yüzdesi ve sekmenin host işareti
  noktası **odaktaki pane'den**; odak değişince tazelenir.
- ⌘Q ve ⇧⌘W soruları koşan işi **pane'lerden** toplar; tek pane'li sekmede
  metin bugünküyle bayt bayt aynı, çok pane'de liste pane'leri sayar.
- Dock simgesinin ilerlemesi bütün pane'lerin kuyruklarının toplamı.
- Örtülme (`set_visible`) ve ekran ölçeği değişimi bütün pane'lere dağılır.
- Alternatif ekranın dock'u kaldırması pane'in kendi işi (haberci pane
  kimliğiyle).

## Karar 12: Süreli koşu → ✅ "tek pencere, tek pane" değişmezi

Süreli koşu ⌘D görmez; `quiet_since`, `shutdown` ve `report_and_exit`
pencerenin tek pane'inin renderer ve link'ini okur. Jeton satırı değişmez
(jeton silinmez kuralı; değer de aynı kalmalı).

## Karar 13: Sıra → ✅ önce ayrım (davranış değişmez), sonra bölmeler, tek set

Kullanıcı kararı (2026-09-27). Ayrım phase'leri kendi başına yeşil ve
davranışsız; bölme phase'leri onun üstüne.

## Karar 14: En küçük pane → ✅ tasarım sabiti, bölme ve boyutlama kırpılır

Bir pane ızgarası bir alt sınırın (sütun ve satır, tasarım sabiti) altına
inecekse bölme yapılmaz ve boyutlama orada durur; pencerenin kendisi
küçültülürken pane'ler oranlarını korur. Sayı `const`'un doc'unda gerekçeli.

## Karar (2026-09-27, kullanıcı onayı + otonom akış)

- **Seçilen:** Karar 1 ve 13 **kullanıcı onayı** (native yol, önce pane
  ayrımı sonra bölmeler, "tek NSView + olay geri bildirimi + eylem API'si").
  Karar 2–12 ve 14 **otonom akış**: teknik kararlar yukarıdaki gerekçeyle;
  UX kararları (7, 8, 9, 11) macOS/iTerm2/Ghostty emsalinden, soru yapılmadı.
  Panel koşmadı: değişen dosyalar yalnız `bt-shell` — pahalı karar sınıfının
  (`proje.md`) hiçbir satırına dokunmuyor; soluk örtü bilerek AppKit'te
  tutuldu, kare yoluna girseydi sınıf tetiklenirdi.
- **Reddedilen:** `NSObject` denetleyici (Karar 2), pane içinde `AppDelegate`
  erişimi (Karar 3), ayrı crate (Karar 4), paylaşımlı renderer (Karar 5), iç
  içe `NSSplitView` (Karar 6), GPU'da soluklaştırma (Karar 7), paylaşılan
  `TERM_SESSION_ID` (Karar 10).
