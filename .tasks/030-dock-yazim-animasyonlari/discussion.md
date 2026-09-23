# Dock'ta yazma ve silme animasyonları — Tartışma

Karar-listesi biçimi: birbirinden bağımsız yedi karar noktası. Karar 1, 4 ve 5
pahalı karar sınıfına dokunuyor (katman yönü, kare yolu, shader) ve panelden
geçti; aşağıdaki metin panelden **sonraki** hâl, değişenler `## Muhakeme`'de.

## Karar 1: Eklenen ve silinen glyph nerede bulunuyor → ✅ `bt-core`, `Session::dock`'ta, çağıranın tamponuna karşı

`Session::dock` (`crates/bt-core/src/session.rs`) her içerik karesinde yeni
aynayı çağıranın tamponuna (`into`, `bt-gpu`'nun `LinkIvars::dock`'u) kopyalıyor
ve kopyadan **hemen önce** `into` tam olarak son çizilen aynadır. Fark orada,
aynı yaprak kilit turunda alınıyor:

- **Kapı damga eşitliği:** `into.answers == yeni.answers` ise fark alınmıyor —
  yeni girdi yoksa canlanacak bir düzenleme de yok (Karar 2), yani olağan
  içerik karesinin (koşan komutun çıktısı, sayaç) bedeli tek bir tamsayı
  karşılaştırması. Fark yalnız damga ilerlediğinde, `clone_from`'un zaten
  ödediği O(n)'in yanında bir önek/sonek taraması.
- **Yalnız `BUFFER`:** `PREDISPLAY` ve `POSTDISPLAY` farka girmiyor —
  zsh-autosuggestions'ın önerisi her tuşta toptan değişiyor (önerili satırda `l`
  yazmak görüntü dizgisini hiç değiştirmiyor bile: `ls -la` → `l` + `s -la`).
  `CURSOR` belirsizliği çözüyor: ekleme koşusu yeni caret'te biter, geri silme
  koşusu yeni caret'te başlar.
- **Aynı kareye düşen iki ayna kendiliğinden tek koşu:** taban son *çizilen*
  ayna olduğu için ara aynalar atlanıyor ve iki tuş tek bir iki glyph'lik ekleme
  olarak çıkıyor.
- **Hayaletin rengi eski `into.highlights`'tan:** silinen glyph'in vurgu stili
  henüz ezilmemiş tamponda.
- **Yeni taraf `Live` olmak zorunda; eski taraf `Live` ya da `Idle`** (`Idle`
  boş satır sayılıyor). `Unavailable`, `Multiline`, `Control` iki taraftan
  birindeyse `Reset`. `Idle`'ın kabulü şart: Enter'dan sonra `line-finish`
  aynayı `Idle`'a indiriyor ve `line-init`'in boş aynası çizilmeden gelen ilk
  tuşun tabanı o — kural "iki taraf da `Live`" olsaydı **her komutun ilk harfi**
  canlanmazdı.
- **`Idle` aynası da damgalı:** `apply_dock`'un `End` kolu `reset()`'ten sonra
  o anki nesli yazıyor (`DockState::answers`). Bugün `reset()` damgayı sıfırlıyor
  ve sıfır damgalı taban girdi sayısı sınırını boşa düşürürdü — prompt'taki ilk
  eylem bir yapıştırmaysa `yeni.answers − 0` her glyph sayısını kabul eder ve
  yapıştırma harf harf canlanırdı. Mevcut sınamanın cümlesi ("kapanmış ayna eski
  damgayı taşımamalı") eski damga için doğru kalıyor; taşınan damga güncel.

`ShellLog`'a durum **eklenmiyor**, paylaşılan duruma yeni bir yazar doğmuyor;
okuyucu thread'e tek değişiklik `End` kolunun damgası.

**Bilinen sınır:** alternatif ekranda `Session::dock` hiç çağrılmıyor (dock
kalkıyor), yani dönüşte taban vim'den önceki son çizilen ayna. Dönüşün aynası
ondan tek bitişik ekleme olarak çıkmıyorsa `Reset` — olağan hâl; tesadüfen
çıkarsa en kötü sonuç bir harfin gereksiz yere canlanması. Yönü güvenli.

**Reddedilen:** fark `ShellLog::apply_dock`'ta, bekleyen düzenleme + ayrı taban
kopyası + "boşaltınca taban ilerler" sözleşmesi (panel öncesi öneri — aynı
davranışa üç parça fazlasıyla gidiyordu ve paylaşılan duruma bir yarış yüzeyi
ekliyordu). **Reddedilen:** `bt-gpu`'da kareler arası `dock_glyphs` farkı —
"bu glyph'i kullanıcı yazdı" terminal semantiği, üstelik `Frame::clear` listeyi
her içerik karesinde siliyor ve sütun farkı yapıştırmayı yazımdan ayıramaz.

## Karar 2: Toplu değişim — ne canlanıyor, ne canlanmıyor → ✅ girdi sayısı sınırı, canlanmayan değişim sıfırlar

Kural tek cümle: **yalnız tek bir bitişik ekleme ya da tek bir bitişik silme
canlanır ve glyph sayısı tabandan beri gönderilen girdi sayısını aşamaz.**
Girdi sayısı yeni bir sayaç değil: aynanın taşıdığı nesil damgasının
(`DockState::answers`, 025) tabandakinden farkı. Kullanıcının gördüğü:

| ne yaptı | ne görür |
|---|---|
| harf yazdı, hızlı yazdı (iki tuş tek karede) | her harf kendi efektiyle gelir |
| Backspace, basılı tutulan Backspace | her silinen harf kendi efektiyle gider |
| Cmd-V (kısa ya da uzun, bracketed ya da `can_be_typed`) | metin anında belirir — tek girdi, çok glyph |
| ↑ / Ctrl-R ile geçmiş | satır anında değişir — ekleme ya da silme değil, değiştirme |
| Ctrl-U, Ctrl-W, Option+⌫ | satır/kelime anında gider — tek girdi, çok glyph |
| Tab tamamlama, öneriyi → ile kabul | anında — tek girdi, çok glyph; tek karakterlik tamamlama (`/`) yazım gibi canlanır |
| ölü tuş bileşimi (`Option+ü` + boşluk → `~`) | canlanır — iki girdi, bir glyph |
| IME'nin çok karakterli onayı | anında — tek girdi, çok glyph (bilinen sınır) |
| girdisiz ayna (prompt yenilemesi, öneri değişimi) | hiçbir şey |
| Enter | satır ızgaraya geçer, efekt yok (`line-finish` → `Idle` → sıfırlama) |

Canlanmayan bir değişim **o ana kadar uçuştaki bütün efektleri bitirir**
(glyph'ler son hâline oturur, hayaletler kalkar): satır toptan değiştiyse eski
satırın hayaleti yeni satırın üstünde yersiz kalırdı. Aynı sıfırlama ayna
`Live` dışına çıkınca da (satır ızgaraya düştü, alternatif ekran, `Multiline`)
ve pencereleme kaydığında da koşuyor (Karar 3).

**Reddedilen:** sabit bir "en çok N glyph" eşiği — ölçülmemiş bir sayı olurdu
ve kısa bir yapıştırmayı yazım sanırdı; girdi sayısı bunu sayı uydurmadan
ayırıyor. **Reddedilen:** yapıştırmaya kademeli bir animasyon — referans iki
efekti de "yazarken" ve "Backspace ile" diye tanımlıyor, yüzlerce glyph'in
yağmuru okunan metni geciktirir.

## Karar 3: Sınırdan ne geçiyor → ✅ ikinci sink, `Cell`'e alan yok, sütun defteri yok

`bt_core::Cell`'e alan eklenmiyor. `Session::dock` ve `dock::render` **ikinci
bir sink** alıyor ve karede en çok bir `DockEdit` basıyor:

- `Arrive { col, cells }` — gelen glyph'lerin çözülmüş hücreleri ve ekran
  sütunu (caret'in solundaki `width` sütun). Hücreler **normal sink'e de**
  gidiyor; hangisinin çizileceği boyamanın kararı.
- `Erase { col, ghosts }` — silinen glyph'lerin çözülmüş hücreleri (karakter,
  renk, biçim, `wide`), sütunu caret'in sütunu.
- `Reset` — canlanmayan her değişim (Karar 2) **ve pencerelemenin kayması**:
  `render` saf olduğu için eski tamponun `skip`'ini de hesaplayabiliyor; kaydıysa
  düzenleme `Reset`'e dönüyor. Sınıra `skip` alanı eklenmiyor.

Sütun çözümü `render`'ın kendi pencereleme hesabından, ikinci bir kopya değil.

**Uçuştaki efektin sütunu kaydırılmıyor.** Tek kural: yeni bir düzenlemenin
sütunu uçuştaki bir gelişin sütunundan küçük ya da eşitse o geliş bitirilir
(yerine oturur); hayaletler yerinde kalır. Normal yazımda geliş caret'in
solunda doğuyor ve sonraki tuş onun sağına ekliyor, basılı Backspace'in
hayaletleri sola doğru sıralanıyor — ikisinde de kaydırılacak bir şey yok.
Kullanıcının göreceği tek fark: caret'i efekt süresi içinde uçuştaki bir
glyph'in soluna taşıyıp yazarsa o glyph erken oturur.

## Karar 4: Animasyonun durumu, saati ve durma koşulu → ✅ `Motion`'ın dışında kendi tipi, blink emsali

`GlyphFx` (`bt-gpu/src/glyph_fx.rs`, saf, ObjC'siz): uçuştaki girdilerin sabit
kapasiteli listesi — hücre (sütun, karakter, renk, yüz, `wide`), tür (geliş /
hayalet), efekt, geçen süre. Link'te **ayrı bir `RefCell` ivar'ı**:
`Motion` `Copy` ve `Cell<Motion>` içinde yaşıyor (`link.rs` → `LinkIvars::motion`'ın
doc'u), bir liste onu ya `Copy`'den çıkarır ya da her `get`/`set`'te
kopyalatırdı.

- **Saat:** `advance(dt)`, öteki animatörlerle aynı `dt`. Süresi dolan girdi
  düşer; **boş liste = yerleşik**. Tavan `FX_MAX` (tasarım sabiti), dolunca en
  eski girdi bitirilir.
- **Uyku testine adlı bir terim** (blink'in "üçüncü soru"su emsali, `link.rs`'in
  "hasar yok" dalı): `motion.settled() && fx.is_empty()`. Kare talebi hareketin —
  `Waker::wake`'e dokunulmuyor, hasar dikilmiyor, `icerik=` sayılmıyor.
- **`finish()` kapsıyor:** `Motion::finish`'i çağıran üç yer (örtülen pencere,
  `snap`'e geçen ayar, senkron çizim hatası) fx'i de bitiriyor; yoksa arka
  sekmede donan efekt geri gelince görülmemiş bir fazdan devam eder ya da
  kalıcı hatada hareket karesi dönmeye devam ederdi.
- **İçerik karesi:** düzenlemeler bir tampona akıyor (doldurma bandının
  emsali, `LinkIvars::fill` — iki sink aynı çağrıda `frame`'i ödünç alamaz),
  `render` dönünce `GlyphFx`'e işleniyor, sonra `Frame` uçuştaki gelişlerin
  statik glyph'ini `dock_glyphs`'ten **çıkarıyor** (sütun ve karakter
  eşleşirse; eşleşmezse girdi bitirilir, statik glyph kalır). Böylece `fade`
  çift çizilmiyor.
- **Hareket karesi:** `move_caret` emsali — dock'un statik listeleri korunuyor,
  yalnız fx listesi `GlyphFx`'ten yeniden basılıyor.
- **Sayaçlar:** `hareket=` ve `kayma=` anlamını koruyor (imleç / öteleme);
  yalnız fx'in sürdüğü kare `kare`'yi artırır, `icerik`'i artırmaz.

**Reddedilen:** `Motion`'ın içinde dördüncü animatör (panel öncesi öneri —
`Copy` sözleşmesiyle kavga ediyordu). **Reddedilen:** instance'a başlangıç
damgası + shader'a `now` uniform'u — uyku kararı yine CPU'da ve iki saat alanı
ayrışabilirdi.

## Karar 5: GPU tarafı → ✅ kardeş pipeline `glyph_fx`, kendi instance'ı

`FxInstance` (`#[repr(C)]`, iki tarafta assert'li): `pos`, `uv0`, `rgba`,
`float4 fx` = {ilerleme `t`, efekt kimliği + düzlem (maske/renk) + yarı
(tek/sol/sağ) tek `u32`'de paketli, tohum, yedek}. Dörtlü hücreden **efekt
payı kadar şişiyor**, fragment noktayı efektin ters dönüşümüyle glyph uzayına
çeviriyor ve **yuvanın dışını örneklemiyor** (`nearest` + sınır testi).

- **İki doku bağlı**, düzlem instance'tan: blend dört pipeline'da aynı
  (`renderer.rs`'in blend yorumu), emoji ayrı bir pipeline istemiyor.
- **Geniş glyph:** yelpazeleme `AtlasTexture::prepare`'in yolundan (tek yer,
  ikinci kopya yok); instance hangi yarı olduğunu taşıyor ve dönüşümün merkezi
  **iki hücrelik kutunun** merkezi — yoksa `pop`/`recede`/`iris` bir emojiyi
  ortadan ikiye ayırırdı.
- **Parçalı efektler** (`shatter`, `unravel`) geometri üretmiyor: fragment
  glyph'i k×k karoya (ya da şeride) bölüyor, her karo kendi tohumundan ötelenip
  dönüyor, karo sınırında kırpılıyor; fragment karoları gezip ilk isabeti alıyor.
- `CursorBlock` uniform'u okunuyor (blok caret'in altındaki glyph
  `cell_fragment`'teki gibi boyansın).
- **Encode sırası:** hayaletler dock glyph'lerinden **önce** (metnin altında —
  satır ortasında silinen harfin yerine kayan harf hayaletin üstünde durur),
  gelişler **sonra**.

**Reddedilen:** `GlyphInstance`'ı genişletmek — bütün glyph listelerinin
stride'ı animasyonlu bir avuç glyph için büyürdü (012'nin `>` dersi).
**Reddedilen:** CPU'da efekti hesaplayıp mevcut pipeline'a vermek — bugünkü
aritmetikle (`GlyphInstance`'ta boyut yok) ölçek, maske ve parça temsil
edilemiyor ve on yedi efektin çoğu kaybolurdu.

## Karar 6: Efektler → ✅ on yedi efekt, aşağıdaki tanımlarla

Süreler **tasarım sabiti**, ölçülmüş değil: `KEYPRESS_DURATION` (geliş) ve
`ERASE_DURATION` (hayalet). Parçalı iki efektin uzağa gitmesi süreyi değil
shader'daki eğriyi değiştiriyor — üçüncü bir süre kavramı yok. Eğri tek: çıkışta
yavaşlayan kübik; `pop` ve `drop`'un taşması kapalı formdan. Genlikler hücre
oranında (punto büyüyünce birlikte büyür). Her efekt `t = 1`'de **piksel piksel
statik glyph**'e iner (geliş) ya da tam saydam olur (hayalet) — son fx
karesinden statik çizime devirde sıçrama yok ve bu hermetik bir değişmez
(plan.md).

**Keypress** (glyph'in gelişi):

| ad | görünüş |
|---|---|
| `off` | glyph anında belirir (bugünkü hâl) |
| `fade` | glyph yerinde saydamdan tam renge belirir |
| `rise` | glyph hücrenin biraz altından yukarı kayarak yerine oturur, kayarken belirir |
| `pop` | glyph küçük doğar, bir an yerinden biraz büyür ve yerine oturur |
| `extrude` | glyph sol kenarından sağa doğru uzayarak çıkar, caret'in içinden itilmiş gibi |
| `heat` | glyph kızgın bir renkte (temanın `cursor`'ı) doğar ve kendi rengine soğur |
| `echo` | glyph yerinde belirir, üstünden büyüyerek sönen soluk bir kopyası halka gibi dağılır |
| `drop` | glyph hücrenin üstünden düşer, hafifçe sekip yerine oturur |
| `ink` | önce çizgilerin koyu çekirdeği görünür, mürekkep kenarlara yayılır gibi glyph dolar |
| `squeeze` | glyph yatayda sıkışmış ve dikeyde uzamış doğar, esneyerek kendi oranına açılır |

**Erase** (silinen glyph'in hayaleti):

| ad | görünüş |
|---|---|
| `off` | glyph anında kaybolur (bugünkü hâl) |
| `iris` | glyph'in üstünde dairesel bir diyafram merkezine doğru kapanır |
| `undertow` | glyph akıntıya kapılmış gibi aşağı ve caret'e doğru çekilerek söner |
| `echo` | glyph büyüyerek dışa doğru bir halka gibi dağılır ve söner |
| `bleed` | glyph'in mürekkebi dağılır: kenarlar yayılıp incelirken renk zemine akar |
| `unravel` | glyph yatay şeritlere ayrılır, şeritler sırayla yana kayıp çözülür |
| `recede` | glyph merkezine doğru küçülerek geri çekilir ve söner |
| `sublime` | glyph buharlaşır gibi yukarı süzülür, açılarak söner |
| `shatter` | glyph birkaç parçaya kırılır, parçalar hafif dönerek dağılıp düşer ve söner |

## Karar 7: Ayarlar, varsayılanlar ve indirgemeler → ✅ `fade` / `recede`, indirgeme `bt-gpu`'da

- `[motion] keypress` — `"off" | "fade" | "rise" | "pop" | "extrude" | "heat" |
  "echo" | "drop" | "ink" | "squeeze"`, varsayılan **`"fade"`**.
- `[motion] erase` — `"off" | "iris" | "undertow" | "echo" | "bleed" |
  "unravel" | "recede" | "sublime" | "shatter"`, varsayılan **`"recede"`**.
- Varsayılanların gerekçesi: kullanıcı animasyonu açıkça istiyor ve kutudan
  çıkınca görmeli; ikisi de kendi listesinin **en az yer değiştiren** efekti —
  glyph yerinden oynamıyor, yalnız belirip / küçülüp gidiyor. Gösterişli olanlar
  bir popup uzaklığında.
- **`cursor_motion = "snap"` ikisini de kapatır, Hareketi Azalt** gelişi
  `fade`'e indirir ve hayaleti kapatır. Gerekçe mevcut sözleşme: `snap`
  hareketi zaten kapatmış olanın beyanı ve erişilebilirlik ayarı animasyon
  *eklemez* (`docs/AYARLAR.md` → `[motion]`, 027'nin `smooth_scroll` emsali);
  belirme imlecin Hareketi Azalt kipinin kendisi (`Mode::Fade`), hayalet ise
  orada olmayan bir içerik. **İndirgeme `bt-gpu`'da, `Motion::mode`'un yanında**
  (`CLAUDE.md`: "İndirgemenin tek yeri `bt-gpu::motion`"): `bt-gpu` iki efekt
  adını ham alır, stil ve `reduce` zaten onda. `bt-shell`'in `smooth_scroll`
  birleşimi emsal değil — tekerleği tüketen `bt-shell`, efekti tüketen `bt-gpu`.
- **Adlar phase başına büyüyor:** enum ve `NAMES` yalnız çizilebilen efektleri
  taşıyor, yani popup'ta çizilmeyen bir ad hiçbir ara commit'te görünmüyor.
  Bugün `[motion] keypress = "pop"` "bilinmeyen anahtar" sınamasının tanığı
  (`settings.rs`); tanık başka bir sahte anahtara taşınır.
- Ayar penceresinin Motion bölmesine iki popup ("Keypress:", "Erase:"), tek
  kaynak `NAMES`; `snap` ya da Hareketi Azalt onları ezdiğinde satırlar
  **devre dışı ve açıklamalı** (029 Karar 6: gizlenmez).
- Kayıt anında uygulanır (`Changes::motion`'ın kolu).
- **Speed çarpanı kapsam dışı**: referansta bölümün bütün süreleriyle çarpılıyor
  (imleç, öteleme, süzülme dahil), yani bu setin değil bütün hareket
  altyapısının ayarı.
- `make duman`'a jeton **eklenmiyor**: süreli koşu `/bin/sh` koşturuyor ve dock
  almıyor, yani sayaç her koşuda sıfır olurdu; tanık hermetik sınamalar.

## Muhakeme (2026-09-23)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

**Kabul edilen itirazlar → plan değişikliği:**
- Sadelik: fark için `ShellLog`'da bekleyen düzenleme + ayrı taban gereksiz, son
  çizilen ayna `Session::dock`'un `into`'su → fark `clone_from`'dan önce, damga
  kapıyla (Karar 1); paylaşılan duruma yeni alan yok, okuyucu thread'deki tek
  değişiklik `End` kolunun damgası (mevcut yaprak kilit altında), yeni yarış
  yüzeyi yok.
- Sadelik: `bt-gpu`'daki sütun defteri ve sınırdan `Dock::skip` 150 ms'lik
  efektler için fazla → "yeni düzenleme sütununa eşit ya da sağdaki gelişler
  biter", pencereleme kayması `Reset` (Karar 3).
- Sadelik: `SHARD_STRETCH` üçüncü bir süre kavramı → uzama shader eğrisinde
  (Karar 6).
- Codebase-fit + İşletme: `Motion` `Copy` ve `Cell` içinde → `GlyphFx` ayrı
  `RefCell` ivar'ı, uyku testine adlı terim, `finish()`'in üç çağıranı fx'i de
  bitirir (Karar 4).
- Codebase-fit: iki sink aynı çağrıda `frame`'i ödünç alamaz → düzenlemeler
  tampona (doldurma emsali), susturma `render`'dan sonra (Karar 4).
- Codebase-fit + İşletme: geniş glyph iki yarıya bölünüyor → instance'ta yarı
  biti, dönüşüm merkezi iki hücrelik kutu, yelpazeleme `prepare`'in yolundan
  (Karar 5).
- Codebase-fit: indirgemenin yeri `bt-shell` değil `bt-gpu` (Karar 7).
- İşletme: ayar adları phase başına büyür, bilinmeyen anahtar tanığı taşınır
  (Karar 7); hermetik değişmezler kapı olarak plana yazılır (t = 1'de statik
  glyph'e piksel eşitliği, hayaletin t = 1'de düz zemini, komşu yuvanın hiç
  örneklenmemesi — on yedi efektin hepsi üzerinde); ara kareler yalnız gözle,
  adıyla yazılır; phase bölmesi beşe çıktı (ayarlar ayrı).

**Reddedilenler:**
- İşletme: phase-1 için `make test-yaris` ve yeni bir `race_` sınaması —
  itirazın dayandığı paylaşılan durum (bekleyen düzenleme) kabul edilen
  sadelik itirazıyla kalktı; fark mevcut yaprak kilit turunda, yalnız
  çağıranın kendi tamponuna karşı alınıyor.

## Karar (2026-09-23, otonom akış)

- **Seçilen:** Karar 1–7'nin ✅ satırları, panelden geçmiş hâlleriyle. Özü:
  hangi glyph'in geldiği/gittiği `bt-core`'da, `Session::dock`'ta son çizilen
  aynaya karşı ve yalnız `BUFFER` üzerinde bulunuyor; yalnız girdi sayısını
  aşmayan tek bitişik ekleme/silme canlanıyor, gerisi uçuştakileri bitiriyor;
  sınırdan ikinci bir sink ile `Arrive`/`Erase`/`Reset` geçiyor; zaman ve
  çizim `bt-gpu`'da — `GlyphFx` blink emsaliyle ayrı bir ivar ve uyku testinde
  adlı bir terim, beşinci pipeline `glyph_fx` şişen dörtlü ve ters dönüşümle;
  on yedi efekt tasarım sabiti sürelerle; varsayılan `fade` / `recede`, `snap`
  ikisini kapatır, Hareketi Azalt gelişi `fade`'e indirip hayaleti kapatır.
- **Reddedilen:** `ShellLog`'da bekleyen düzenleme + taban; `bt-gpu`'da
  kareler arası fark; `bt-gpu`'da sütun defteri; `Motion`'ın içinde dördüncü
  animatör; `GlyphInstance`'ı genişletmek; efekti CPU'da hesaplayıp mevcut
  pipeline'a vermek; sabit bir "en çok N glyph" eşiği; yapıştırmaya kademeli
  animasyon; indirgemeyi `bt-shell`'de birleştirmek; Speed çarpanı (bu set
  değil, bütün hareket altyapısı).
- Panel koştu (üç mercek SORUNLU, KIRMIZI yok); bütün kabul edilen itirazlar
  tasarımı ürün yüzünde değiştirmeden işlendi. Tek kullanıcıya dokunan yan
  etki (caret uçuştaki glyph'in soluna taşınıp yazılırsa glyph erken oturur)
  kod biçiminden doğan ve kullanıcının zor ayırt edeceği bir fark; ürün sorusu
  açılmadı. Yeni bağımlılık yok; `Cargo.lock` değişmemeli.
