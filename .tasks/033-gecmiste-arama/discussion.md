# Geçmişte arama (⌘F) — Tartışma

Karar-listesi biçimi. İki karar gerçek seçenek taşıyor (Karar 1: çubuğun
yüzeyi, Karar 2: taramanın mimarisi) ve ikincisi projenin pahalı karar
sınıfına dokunuyor (kare yolunda CPU hesabı, `proje.md` → Pahalı karar
sınıfı); panel o yüzden koşuyor. Kalanı macOS'un ve emsal terminallerin
(Terminal.app, iTerm2, Ghostty, Warp, VS Code'un terminali, Safari)
yerleşik beklentisi — soru yapılmadı, gerekçesiyle yazıldı.

## Karar 1: Arama çubuğu neyle çizilir, nereye oturur?

### Seçenek A: AppKit — `NSSearchField`'lı yüzen panel, Metal'in üstünde kardeş view

İçerik view'ı düz bir kapsayıcı olur; `BateriView` onu doldurur, arama
paneli onun **kardeşi** olarak üstte durur (layer-hosting bir view'a alt view
eklenmez, Apple'ın kuralı). Panel pencerenin **sağ üst** köşesinde, kenardan
iç payla yüzer: `NSSearchField` + iki küçük anahtar (`Aa` büyük/küçük harf,
`.*` regex) + sayım etiketi ("3 of 17") + yukarı/aşağı ok + kapatma.

**Artıları:**
- Metin girişinin bütün doğruluğu bedava: ölü tuş (`Option+ü` + boşluk),
  IME ve preedit, ⌘A/⌘C/⌘V/⌘X/⌘Z, kelime gezinmesi, VoiceOver, sağ tık
  menüsü. Metal yolunda bunların her biri yeniden yazılırdı ve IME'nin preedit
  borcu (`docs/YOL-HARITASI.md` → "Tam IME") tam da bu yüzeyi istiyor.
- Açık/koyu görünüm pencerenin görünümünden geliyor (`apply_chrome` onu
  temanın zemininden zaten kuruyor), yani alan temanın açıklığına uyuyor.
- PTY boyutu **değişmiyor**: panel içeriği itmiyor, üstüne biniyor. 032'nin
  kuralıyla aynı ("PTY sabit, çizim büyür").
- `Cargo.lock` değişmiyor (ölçüldü, `context.md`).

**Eksileri:**
- Panel sağ üstteki birkaç hücreyi örtüyor; eşleşme oraya düşerse pencere
  kaydırılmalı (Karar 4). Alternatif ekranda TUI'nin sağ üst köşesi örtülür
  (VS Code, iTerm2 ve Ghostty de böyle).
- İçerik view'ı değişiyor: first responder, sürükleme hedefi ve
  `viewFrameDidChange:` gözlemcisi kapsayıcıya değil `BateriView`'a bağlı
  kalmalı — phase'in dikkat noktası.
- Panelin zemini ve kenarı temadan türetilmeli, alanın kendisi sistem
  kontrolü; ikisinin uyumu gözle kontrolün işi.

### Seçenek B: Metal overlay (referansın yolu)

Panel `bt-gpu`'da yeni bir pipeline (`ui_text` benzeri) ve kendi metin
düzenleyicisiyle çizilir.

**Artıları:**
- Tema renkleriyle piksel piksel uyum; referansla aynı mimari.

**Eksileri:**
- Metin düzenleme (caret, seçim, ölü tuş, IME, pano, geri alma) sıfırdan;
  her biri bugün AppKit'in bedava verdiği doğru davranışın kötü bir kopyası
  olur. Yeni bir pipeline, yeni bir odak modeli ve erişilebilirlik boşluğu.
- `bt-gpu`'ya UI durumu girer; maliyet setin geri kalanının birkaç katı.

### Seçenek C: İçeriği iten şerit (Terminal.app, `NSTitlebarAccessoryViewController`)

**Eksileri:**
- İçerik view'ı kısalır → `refresh_geometry` → her ⌘F'de PTY yeniden
  boyutlanır, TUI'ler yeniden çizer, uzun satırlar yeniden sarılır; Esc'te
  aynısı tersine. Kullanıcı bunu "arama açınca ekran zıpladı" diye görür.

### Seçenek D: Çocuk pencere (`NSPanel`)

**Eksileri:**
- Metin alanı ancak panel key olunca yazılır; terminal penceresi key'i
  kaybeder, caret odaksız görünür, başlık çubuğu pasifleşir. Sekme geçişinde
  ayrıca gizlenmesi gerekir.

## Karar 2: Tarama nerede ve ne kadar koşar?

İki ayrı soru var ve bütçeleri ayrı: **vurgu** (ekranda görünen eşleşmeler —
yazdıkça anında) ve **sayım + gezinme** (bütün defter — "3 of 17", ⏎ ile en
eski eşleşmeye kadar gitmek).

### Seçenek A: Her tuşta bütün defteri tek seferde tara

**Eksileri:**
- 10 000 satırı `Term` kilidi altında tek parçada taramanın süresi ölçülmedi
  ve o süre boyunca PTY okuyucusu **ve** kare yolu durur (aynı `FairMutex`).
  "Render yolu bloklanmaz" sözleşmesini sayıya bağlı bırakır.

### Seçenek B: İki bütçe — vurgu kare yolunda sınırlı, dizin ana kuyrukta parça parça

- **Vurgu:** arama etkinken her **içerik** karesinde `frame()`'in zaten
  aldığı `Term` kilidi turunda, yalnız çizilen satırlar üzerinde (`RegexIter`,
  görünür pencere + doldurma bandı + kesrin tepe satırı). Maliyet ekranın
  boyuyla sınırlı — `frame()`'in hücre döngüsüyle aynı büyüklük. Hareket
  karesi listeleri koruduğu için orada tarama yok. alacritty'nin kendi
  uygulamasının yolu.
- **Dizin** (sayım + gezinme): `bt-core`'da bir eşleşme dizini, **dipten
  yukarı** doğru, parça başına sınırlı satırla (`Session::search_step`);
  sürücüsü `bt-shell`, ana kuyrukta bir sonraki turda yeniden kuruluyor —
  tuş olayları parçaların arasına giriyor. Yeni sorgu nesli artırıyor, eski
  neslin parçası düşüyor (iptal). Parçanın boyu tasarım sabiti ve ilk phase'in
  ölçümünden türüyor.
- *(Muhakeme'de düştü: çıpa güvenilir değil, dizin çıpasız ve baştan kuruluyor — `## Karar`.)* **Geçmiş değişmez, ekran değişir:** defterin satırları (resize'ın yeniden
  sarması, `CSI 3 J` ve RIS dışında) bir kez tarandı mı bir daha taranmaz.
  Dizin eşleşmeleri çıpalı bir koordinatta tutuyor; çıpa bir satırın kimliği
  (`row_identity`, 011/017'nin ölçüsü) ve her okumada "o satır şimdi kaçıncı
  satırda" sorusuyla kayma bulunuyor. Yeni çıktı yalnız geçmişe **yeni düşen**
  satırların taranmasını istiyor; çıpa kaybolduysa (yeniden sarma, defterin
  silinmesi) dizin baştan kuruluyor.
- Dizin boşta kare **istemiyor**: parçaların sonucu yalnız sayım etiketini
  (AppKit) değiştiriyor; vurgu zaten içerik karesinden geliyor.

**Artıları:**
- Yazılan her tuşun görünür cevabı bir içerik karesi — referansın "no lag on
  the first keystroke" sözü, ekranın boyuyla sınırlı bir işle.
- Kilit tutma süresi sabitle sınırlı; akan çıktıda iş yalnız yeni satır
  kadar.

**Eksileri:**
- İki tarama yolu (görünür + dizin) aynı eşleşmeyi iki kez bulabiliyor;
  sayım dizinden, vurgu kareden. İkisinin aynı eşleşmeye aynı sonucu vermesi
  bir değişmez ve bekçisi gerek.
- Çıpa-kayma aritmetiği yeni bir durum; yanlışlığı sessiz (yanlış satırda
  "geçerli" vurgu).

### Seçenek C: Dizini `frame()`'in kilit turunda kare başına bütçeyle ilerlet

**Artıları:**
- İkinci bir kilit turu yok; tetik hasar yolundan.

**Eksileri:**
- Tarama sürdükçe içerik karesi ister — ekranda değişmeyen bir şey için GPU
  kareleri; "boşta sıfır kare" sayacında (`icerik=`) görünür. Sayım bir AppKit
  etiketi ve kare istemesi için gerekçesi yok.
- `frame()`'in imzası ve sorumluluğu büyür.

### Seçenek D: Ayrı bir arama thread'i

**Eksileri:**
- Kilit tutma süresi B ile aynı sınırda (parça başına `FairMutex`), üstüne
  thread ömrü, iptal kanalı ve sonuçların ana thread'e taşınması. B'nin ana
  kuyruğu aynı paralelliği tuş olaylarını araya sokarak veriyor.

## Karar 3: Sayım, sıra ve gezinme yönü

Terminal en yenisi altta okunur ve arama çoğunlukla "az önce gördüğüm şey"
içindir (iTerm2 ve VS Code'un terminali Enter'la **yukarı** gidiyor).

- Numaralandırma **en yeniden**: en alttaki eşleşme 1.
- ⏎ ve ⌘G bir sonrakine = **yukarıya, daha eskiye**; ⇧⏎ ve ⇧⌘G aşağıya.
  Paneldeki okların yönü aynı (yukarı ok = ⏎).
- Uçlarda sessizce sarar (Safari).
- Yazarken geçerli eşleşme: aramanın başladığı pencerenin **altından yukarı**
  ilk eşleşme; görünürde bir eşleşme varsa pencere hiç oynamıyor.
- Etiket: "3 of 17"; dizin henüz bitmediyse "3 of 17…"; eşleşme yoksa
  "No matches"; geçersiz regex'te "Invalid pattern"; boş sorguda boş.

## Karar 4: Pencere eşleşmeye nasıl gider?

- Eşleşme görünür pencerede ve panelin altında değilse pencere **oynamaz**.
  Doldurma bandındaki eşleşme de görünürdür (bant gerçek geçmiş satırlarını
  gösteriyor).
- Değilse eşleşmenin satırı pencerenin ortasına gelecek kadar kaydırılır.
- Mesafe bir ekranı aşmıyorsa 027'nin süzülmesiyle (`Glide`); aşıyorsa hedefin
  bir ekran yakınına anında konup son ekran süzülür — `scroll_in`'in
  "patlama" emsali: her gezinme aynı hareketle okunur.
- `smooth_scroll = "off"`, Hareketi Azalt ve `cursor_motion = "snap"`: anında
  (027'nin `resolve_smooth_scroll` tek `bool`'u).
- Konumu dışarıdan sıfırlayan her yol gibi gezinme de süzülmenin neslini
  artırır (027'nin kuralı).

## Karar 5: Kapatma — Esc ne yapar?

- Esc (alandayken), kapatma düğmesi: panel kapanır, **pencere olduğu yerde
  kalır**, geçerli eşleşme **seçim** olur (031'in ızgara seçimi; ⌘C onu hemen
  kopyalar) ve klavye terminale döner.
- Gerekçe: Safari, Terminal.app, iTerm2 ve VS Code kapatırken zıplamıyor;
  bulunan yeri okuyan kullanıcıyı başladığı yere geri atmak okuduğunu
  kaybettirir. Başlanan yere dönmenin bedeli zaten bir tuş: girdi pencereyi
  dibe döndürüyor (`Session::write_owned`).
- **Bilinen sınır:** eşleşme doldurma bandındaysa seçim kurulur ama bant
  seçim çizmiyor (017'nin kayıtlı kararı; yol haritasında borç) — ⌘C yine
  kopyalar, vurgu görünmez.
- Terminale tıklamak paneli kapatmaz (Safari); vurgu kalır, klavye terminale
  geçer. Terminaldeki Esc kabuğa gider — vim'in Esc'i yutulmaz.
- ⌘F panel açıkken alanı odaklar ve metnini seçer.

## Karar 6: ⌘E ve find panosu

- ⌘E (Use Selection for Find): seçim (ızgaranın ya da dock'un) sorgu olur,
  panel açılır; regex kipindeyse metin kaçırılarak girer (düz eşleşsin).
- macOS normu: ⌘E sistemin find panosuna da yazar; sekmede önceki sorgu
  yoksa ⌘F paneli find panosunun metniyle açar (Safari'de ⌘E ile seçilen şey
  bateri'de ⌘G ile aranabilir).
- Sorgu ve iki anahtar **sekme başına** (026: her sekme kendi oturumu),
  kapanınca unutulmaz, yeniden ⌘F'de seçili gelir. Ayar dosyasına
  yazılmaz; ayar penceresine satır yok.

## Karar 7: Görünüş

- Temaya iki rol: `search_match` (bütün eşleşmeler, soluk) ve
  `search_current` (geçerli eşleşme, belirgin). Kuralı istisnasız: eksikse
  gömülü tabandan (`docs/AYARLAR.md` → Temalar). Gömülü iki temanın değerleri
  bir tasarım kararı ve gözle kontrolle iner.
- Şekil 031'in şekli: yuvarlak köşeli koşu, `selection` pipeline'ı,
  `SELECTION_RADIUS`; renk çağrı başına uniform olduğu için iki rol iki ek
  encode. Köşeler **eşleşme başına** hesaplanır: ardışık satırlardaki iki
  ayrı eşleşme tek şekle kaynamaz (sarılan tek eşleşme kaynar).
- Metin kendi ön planıyla çizilir (031 Karar 3). Odaksız pencerede iki rol de
  seçim gibi zemine doğru soluklaşır (031 Karar 9'un kuralı).
- Çizim sırası: zemin → `search_match` → `search_current` → seçim → caret →
  glyph. Kullanıcının seçimi aramanın üstünde.
- Alan odaktayken terminalin caret'i odaksız hâlini alır (içi boş, blink
  durur): klavyenin nereye gittiğini söyleyen tek sinyal. Pencere key
  kalıyor. *(Muhakeme'de düzeldi: iki ayrı bit — vurgu ve seçim yalnız
  pencere key değilse soluyor; `## Karar`.)*
- Panel açılış/kapanışta kısa bir belirme + kayma (AppKit animasyonu); süre
  bir tasarım sabiti, 030'un gözle bulunan 240 ms tabanından başlar ve
  gerçek pencerede ayarlanır. Hareketi Azalt'ta animasyonsuz. Vurgunun
  kendisi animasyonsuz — yazdıkça anında.

## Karar 8: Hangi yüzeyler aranır?

- **Izgara + defter**: evet — aramanın konusu.
- **Doldurma bandı ve kesrin tepe satırı**: vurgu **evet**. İkisi gerçek
  geçmiş satırlarını çiziyor ve 017'nin dersi burada da geçerli: bant ikinci
  bir yüzey, ızgaradan türeyen her şeyi ayrıca kazanmak zorunda. Bandın kendi
  listeleri ve `setViewport`'u var.
- **Dock**: hayır. Dock ZLE'nin `BUFFER`'ını gösteriyor — yazılmakta olan
  komut, geçmiş değil — ve indeks uzayı ayrı (karakter, satır değil).
- **Bastırılan giriş satırı** (dock'un çizdiği, ızgarada gizli satırlar):
  vurgu, sayım ve gezinme dışında. Görünmeyen bir satıra "3 of 17" demek,
  ⏎'nin pencereyi boş bir yere götürmesi olurdu.
- **Alternatif ekran**: arama açılır ve görünür ızgarada arar (alternatif
  ekranın defteri yok); sayım görünür eşleşmeler, gezinme kaydırmaz, vurgu
  içerik karesiyle güncellenir. Ekran geçişinde dizin baştan kurulur.

## Karar 9: Akan çıktı

- Vurgu her içerik karesinde yeniden hesaplandığı için çıktıyla birlikte
  kayar.
- *(Muhakeme'de düzeldi.)* Dizin defter değişince baştan sayar, sürerken
  etiket "…"; geçerli eşleşme `display_offset` ya da `history_size` farkıyla
  içeriğine yapışık kalır, ikisi de tutmazsa en yakın eşleşmeye geçer.
- Geçerli eşleşme doymuş defterin tepesinden düşerse geçerli, kalan en eski
  eşleşmeye geçer.
- Pencere kaydırılmışsa alacritty onu yeni çıktıya karşı zaten sabit tutuyor;
  dipteyse dibi izliyor. Arama bu davranışa dokunmuyor.

## Karar 10: Klavye ve menü

- Edit ▸ Find ▸ Find… (⌘F), Find Next (⌘G), Find Previous (⇧⌘G), Use
  Selection for Find (⌘E) — menü öğesi; `keyDown:`'ın Cmd izin listesi üç
  tuşta kalır (⌘A emsali).
- Seçiciler **kendi adlarımız**, `performFindPanelAction:` değil: alan
  odaktayken first responder AppKit'in alan düzenleyicisi (`NSTextView`) ve o
  seçiciyi kendisi uygulayıp yutardı.
- Alandayken ⏎/⇧⏎ ve Esc alanın komut kancasından
  (`control:textView:doCommandBySelector:`); `NSSearchField`'ın varsayılan
  Esc'i (metni silmek) kapatmayla değiştirilir.
- Alandayken ⌘A/⌘C/⌘V/⌘X alanın kendisine gider (hedefsiz menü eylemi first
  responder'dan başlıyor) — bugünkü menü öğeleri değişmeden.

## Karar 11: Düz metin ve regex

- Varsayılan **düz metin**; `.*` anahtarı regex'i açar.
- Düz metin, desenin metakarakterleri kaçırılarak aynı `RegexSearch`'e
  gider. Kaçırma fonksiyonu `bt-core`'da küçük ve sınamalı; `regex-syntax`'ı
  doğrudan bağımlılık yapmak `Cargo.lock`'a kenar eklerdi.
- `Aa` kapalıyken **akıllı** (alacritty'nin kendi kuralı: büyük harf yoksa
  duyarsız), açıkken her zaman duyarlı (`(?-i)` öneki).
- Geçersiz regex panik değil durum (Karar 3'ün etiketi); karmaşıklık
  sınırını aşan desen alacritty'de zaten `None`.
- Boş eşleşme (`^`, `a*`) vurgulanmaz; alacritty onları kendisi atlıyor.

## Karar 12: Kapsam dışı

- Dock'ta arama, sekmeler arası arama, bul-ve-değiştir (terminalde anlamsız).
- Son aramalar menüsü, anahtarların kalıcılığı, ayar anahtarı.
- Eşleşmeler arası "hepsini seç", URL/yol algılama.
- Doldurma bandında seçim çizimi (017'nin borcu; Karar 5'in bilinen sınırı).

## Muhakeme (2026-09-25)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU — çıpalı dizin ve ana kuyruk sürücüsü ölçümden önce kurulmuş; gezinme dizin istemiyor (`search_next`); bant vurgusu yeni bir liste |
| Codebase-fit | SORUNLU — `row_identity` dizinin çıpası olamaz; odak biti iki anlam taşıyor; `&mut RegexSearch` yaprak kilit kuralıyla çatışıyor |
| İşletme | SORUNLU — çıpa ve "yeni düşen satır" sayısı sessizce bozuluyor; odak biti vurguları soldurur; parça boyu ölçüme bağlanamaz (ölçüm kapı değil) |

Hücre içi KIRMIZI yok. Üç mercek de Karar 1-A'yı (AppKit paneli, PTY
sabit) ve Karar 2-B'nin vurgu yarısını (kare yolunda, görünür satırlarla
sınırlı) sağlam buldu.

**Kabul edilen itirazlar → plan değişikliği:**

- **Dizinin çıpası `row_identity` olamaz** (üç mercek). Gözlem doğrulandı:
  `row_identity`'nin doc'u (`session.rs:1706-1718`) ölçünün yalnız ardışık
  iki karede güvenli olduğunu söylüyor — doymuş defterde en eski satırın
  tamponu yeni dip satır olarak yeniden kullanılıyor, yani uzun tutulan bir
  çıpa kaybolmuyor, **başka satıra geçiyor**; `scrolled_rows` kaydırılmış
  pencerede zaten ölçmüyor (`session.rs:3828-3832`) ve `Cursor::scrolled`'ın
  tavanı bir ekran. Çıkarımı karşılamanın iki yolu sayıldı: (a) yeni bir
  monoton kaynak kurmak — alacritty böyle bir sayaç vermiyor, `history_size`
  doyuyor, yani "kodu açmak" burada güvenilir bir kaynak **yok** demek; (b)
  dizini çıpasız yapmak: sorgu değişince ve defter değişince **baştan**,
  parça parça. Seçilen (b). Kullanıcının gördüğü fark: akan çıktı sürerken
  sayım etiketi "…" gösteriyor ve çıktı durulunca oturuyor — sayım yine
  bütün defterin sayımı, kısılan bir şey yok. → Karar 2 ve 9 aşağıda
  güncellendi.
- **Geçerli eşleşmenin kaymasının kaynağı kesin olanlar:** pencere
  kaydırılmışken `display_offset` farkı (alacritty pencereyi yeni çıktıya
  karşı sabitliyor, fark tam olarak kayan satır sayısı), dipteyken ve defter
  doymamışken `history_size` farkı. İkisi de tutmuyorsa (dipte, doymuş
  defter) geçerli eşleşme yeniden sayımda pencereye en yakın eşleşmeye
  geçiyor. Bekçisi: doymuş defterde geçerli eşleşme yanlış satırı
  göstermiyor.
- **Gezinme dizin istemiyor** (Sadelik): ⏎/⌘G alacritty'nin
  `Term::search_next`'iyle geçerli eşleşmeden bulunuyor ve sarıyor; dizin
  yalnız sayım ve sıra numarası için.
- **Odak iki bit** (Codebase-fit, İşletme): `focused` bugün hem caret'in içini
  boşaltıyor hem seçim rengini soldurıyor (`link.rs:1124-1132`,
  `1300-1305`). Alan odaktayken tek bit düşürülseydi vurgular tam yazarken
  sönerdi. Karar: caret "pencere key **ve** klavye terminalde" değilse
  odaksız, vurgu ve seçim yalnız "pencere key değil"se soluk. İki sonucun
  kullanıcıya ayrışan yanı bariz beklentiye kapanıyor (yazarken vurgu tam
  renkli, terminal caret'i klavyenin orada olmadığını söylüyor); soru
  yapılmadı.
- **Yaprak kilit kuralı** (Codebase-fit): `RegexIter` `&mut RegexSearch`
  istiyor; "arama kilidi → `Term`" sırası modülün sözleşmesini
  (`session.rs:14-17`) çiğnerdi. Karar: derlenmiş desen `Clone`; kare yolu ile
  dizin sürücüsü **kendi kopyalarına** sahip, kopya `Term` kilidinden önce
  yaprak yuvadan alınıp sonra nesli tutuyorsa geri konuyor — `Term`
  altında hiçbir kilit alınmıyor.
- **Parça boyu tasarım sabiti** (İşletme): ölçüm kapı değil ve kilit altında
  tarama süresini ölçen kanca yok. Sabit `GUTTER_PT` emsali bir tasarım
  sabiti, doc'u "ölçülmedi" der ve iddia `docs/OLCUMLER.md` → `## Bekleyen
  iddialar`'a girer; güvenliği sınırlı parça ve iptal veriyor, sayı değil.
- **Tuş olaylarının parçalar arasına girmesi doğrulanmamış bir varsayım**
  (İşletme): phase'in gözle kontrol maddesi — uzun defterde yazarken tuş
  gecikmesi hissediliyor mu.
- **Tetik:** defterin değiştiği haberi `Wake` üzerinden yüksüz ve kenarda
  (`title_changed` emsali), yalnız arama açıkken; kare yoluna bağlı değil,
  yani arka sekmede de işliyor.
- **Uyum notları plana işlendi:** kapsayıcı `setWantsLayer(true)` almalı;
  `sync_geometry` ölçüyü `window.contentView()`'dan değil `BateriView`'dan
  okumalı (`window.rs:1701-1703`); `TerminalWindow` Find öğeleri için
  `validateMenuItem:` kazanıyor; köşeler her eşleşmenin kendi diliminden
  (`selection_corners` satır başına tek koşu varsayıyor, `frame.rs:266-273`);
  içerik view'ının kapsayıcıya dönmesi kendi phase'i (davranış değişmez,
  `make duman`).
- **Bilinen sınır adıyla:** alan odaktayken `mouseMoved:` alan düzenleyicisine
  gidiyor, fare kipi 1003'ün hareket raporları o süre duruyor.
- **Vurgunun durma koşulu adıyla:** görünür tarama yalnız panel açık ve sorgu
  geçerli, boş değilken ve yalnız içerik karesinde.

**Reddedilenler:**

- **"Önce ölç, Karar 2'yi sonuca bağla; yeşilse tek tarama"** (Sadelik) —
  ölçüm projede kapı değil (`duzen.md` → Ölçüm) ve kilit süresi kancası yok;
  tasarım sayıdan bağımsız güvenli olmalı. Parçalı sayım ile tek tarama
  arasındaki fark bir `const` ve sürücü döngüsü; iptal yeni sorguda zaten
  gerekiyor.
- **Bant vurgusunu kaldırmak** (Sadelik) — çıkarım doğru (bant yeni bir liste
  istiyor), ama iki yoldan kısmak kullanıcıya bantta görünen eşleşmeyi
  vurgusuz gösterir; 017'nin kullanıcı bildirimiyle öğrenilen dersi tam bu
  sınıf. Boşlukta kodu açmak.
- **Tema rolleri yerine `accent` türetmesi** (Sadelik) — kullanıcı rolleri
  açıkça istedi ve "vurgu rengini ayarlamak" temanın işi; maliyeti iki
  anahtar ve belge.
- **Find panosunu atmak** (Sadelik) — macOS'un uygulamalar arası ⌘E→⌘G
  normu; bedeli tek bir okuma. Öncelik kuralı tek cümle: sekmenin sorgusu,
  yoksa pano.
- **Uzak sıçramada düz snap** (Sadelik) — kullanıcı gezinmenin süzülmesini
  istedi ve "son ekranı süz" `scroll_in`'in yerleşik emsali, yeni bir dal
  değil.

## Karar (2026-09-25, otonom akış)

- **Seçilen:** Karar 1-A — `NSSearchField`'lı, temaya boyanmış, sağ üstte
  yüzen AppKit paneli; içerik view'ı kapsayıcı, `BateriView` onun çocuğu,
  PTY boyutu değişmiyor; `bt-shell/Cargo.toml`'a `objc2-app-kit`'in
  `NSSearchField`/`NSSearchFieldCell`/`NSTextFieldCell`/`NSActionCell`/
  `NSAnimationContext` (gerekirse `NSSegmentedControl`) özellikleri —
  bağımlılık kararı değil, `Cargo.lock` değişmiyor (ölçüldü, `context.md`). Gerekçe: metin girişinin doğruluğu (ölü tuş, IME,
  pano, geri alma, VoiceOver) bedava, `Cargo.lock` değişmiyor, 032'nin "PTY
  sabit" kuralıyla tutarlı.
- **Seçilen:** Karar 2-B, Muhakeme'nin düzeltmeleriyle — vurgu içerik
  karesinde görünür satırlarla (ızgara, doldurma bandı, kesrin tepe satırı)
  sınırlı; sayım **çıpasız** bir dizin, dipten yukarı parça parça, sorgu ve
  defter değişince baştan, sürücüsü ana kuyrukta, tetiği `Wake` haberi;
  gezinme `Term::search_next`; desen kopyaları kilitsiz sahipli.
- **Karar 3–12** yazıldığı gibi, şu düzeltmelerle: Karar 7'nin odak sinyali
  iki bit (caret: key ve klavye terminalde; vurgu/seçim solması: key); Karar
  9'un geçerli eşleşme kayması `display_offset` / `history_size` farkından,
  ikisi de tutmazsa yeniden sayımda en yakına; akan çıktı sürerken etiket
  "…".
- **Kullanıcı tarafına kapanan ürün soruları** (soru yapılmadı, bariz
  beklenti): Esc'te pencere yerinde kalır ve eşleşme seçim olur; ⏎ yukarı
  (daha eski) gider ve numaralandırma en yeniden; eşleşme görünürse pencere
  oynamaz, değilse ortalanır ve süzülür; bant satırları da vurgulanır; dock
  aranmaz; alternatif ekranda görünür ızgara aranır; alan odaktayken vurgu
  tam renkli, terminal caret'i odaksız; akan çıktıda sayım "…" ile oturur.
- **Reddedilen:** Karar 1-B (Metal overlay) — metin düzenlemenin sıfırdan
  yazılması; 1-C (içeriği iten şerit) — her ⌘F'de PTY yeniden boyutlanır;
  1-D (çocuk pencere) — key kaybı. Karar 2-A — ölçülmemiş süre kadar okuyucu
  ve kare yolu durur; 2-C — tarama için boşuna içerik karesi; 2-D — aynı
  kilit sınırı, üstüne thread ömrü. Çıpalı dizin — güvenilir çıpa yok.
