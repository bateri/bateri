# Fareyle seçim: ızgara ve dock — Tartışma

Karar-listesi biçimi. Karar 1 ve 2 birden çok yaklaşım taşıyor ve ikisi de
pahalı karar sınıfında (`proje.md`): Karar 1 shell betiğine, Karar 2 kare
yoluna (`Session::frame`) ve `#[repr(C)]` ↔ `.metal` düzenine dokunuyor.
Kalanlar tek bariz yolu olan, ama yazılmazsa uygulayana kalacak ürün ya da
sözleşme kararları.

## Karar 1: Dock seçiminin gerçeği nerede, düzenleme ZLE'ye nasıl gidiyor?

Dock bir ayna: satırın sahibi ZLE, terminal onun görüntüsünü çiziyor. Seçim
ve seçimi silme/yerine yazma bu sınırı bir yönde geçmek zorunda.

### Seçenek A: Seçim terminalde, düzenleme özel bir widget'a sayıyla gider

Seçim `bt-core`'da yaşar (`BUFFER`'ın karakter indeksleriyle bir çapa + uç,
hangi `BUFFER`'a ait olduğunu bilir). ZLE seçimi **hiç görmez**; yalnız
eylem anında tek bir komut alır. Sarmalayıcı dock'lu kademede bir widget
kurar ve onu `emacs` ile `viins` keymap'lerine özel bir CSI dizisine bağlar
(`\e[8133~` — OSC 8133 aynasının numarası, kabuktan terminale değil terminalden
kabuğa). Widget yükünü `read -k` ile BEL'e kadar okur ve **yalnız sayı**
taşır:

- `c;N;L` — caret'i `N`'e koy (tıklama, ←/→ ile seçimi daraltma),
- `d;S;E;L` — `[S, E)` aralığını sil ve caret'i `S`'e koy.

`L` beklenen `${#BUFFER}`; tutmazsa widget hiçbir şey yapmaz. Yerine yazma
ve seçimin üstüne yapıştırma `d` + **olağan** yazma/yapıştırma yolu
(`Session::write`, `Session::paste`): metin tele hiç girmez, yani betik
tarafına base64 **çözücü** yazılmaz ve yazılan harf yine `self-insert`'ten,
030'un Arrive efektinden ve `can_be_typed`'ın kapısından geçer.

**Artıları:**
- ZLE'nin davranışına dokunmaz: `self-insert`, `backward-delete-char` ve
  kullanıcının bağlamaları olduğu gibi kalır. Bağlanan tek şey kullanıcının
  hiçbir tuşunun üretmediği bir dizi.
- Ölçüldü (`context.md` → Ölçüm): `vicmd` ve çok baytlı metin dahil çalışıyor,
  değişiklik tek geri alma birimi.
- Seçim, kopya, Cmd-A ve görünüş ZLE'ye hiç gitmeden çalışır — `vicmd`'de ya
  da düzenleme kapısı kapalıyken bile kopyalamak mümkün.
- Tel sıralı (PTY): kapı "ayna son girdinin cevabı" (025) dediğinde gönderilen
  indeksler ZLE'nin gördüğü `BUFFER`'a ait; `L` bunun üstüne ucuz ikinci
  kemer.

**Eksileri:**
- Betiğe yeni bir widget ve iki `bindkey` (pahalı sınıf).
- `read -k -t` BEL gelmezse ZLE'yi zaman aşımı kadar bekletir (bilinen
  sınır; terminal BEL'siz dizi göndermiyor, yani ancak bozuk bir tel).
- Hooks'tan **önce** `bindkey -N` ile yaratılmış kullanıcı keymap'i bağlamayı
  miras almaz — terminal o keymap'te dizi göndermez (kapı `INSERT_KEYMAPS`),
  yani yanlışın yönü "düzenleme yok".

### Seçenek B: Seçim terminalde, düzenleme sentez tuşlarla

Tıklama için ← / → baytları (kitty ve Ghostty'nin "click to move cursor"
emsali), silme için caret'i uca götürüp N kez Backspace.

**Artıları:**
- Betiğe dokunmaz; kabuk bağımsız (bash/fish betikleri doğduğunda da çalışır).

**Eksileri:**
- Baytın anlamı kullanıcının bağlamasına bağlı: zsh-autosuggestions `→`'yi
  satır sonunda **öneriyi kabul** olarak bağlıyor — caret'i sona taşıyan bir
  tıklama öneriyi satıra yazar. `vicmd`'de `←` ve `⌫` başka widget'lar.
- N tuş = N ayna turu; uzun seçimde dock aradaki her ara hâli çizer ve 030'un
  Erase efekti N kez koşar.
- `CURSOR` karakter indeksi, ←'nün adımı ise ZLE'nin `COMBINING_CHARS`
  kararına bağlı; iki sayım ayrışırsa caret yanlış yere iner ve belirti
  sessizdir.

### Seçenek C: Seçim ZLE'de (`MARK` / `REGION_ACTIVE`)

Tıklama/sürükleme widget'la `MARK`/`CURSOR`'ı kurar, ayna iki yeni gövde
taşır, silme ZLE'nin kendi bölge komutu.

**Artıları:**
- Tek gerçek; ZLE'nin kill-ring'i ve bölge komutları seçimi görür.

**Eksileri:**
- zsh'te etkin bölge **yazmayla değişmiyor** ve Backspace onu silmiyor
  (`self-insert` bölgeyi yalnız kapatıyor): istenen davranış için
  `self-insert` ve `backward-delete-char`'ı sarmak gerekir — kullanıcının ve
  eklentilerinin (autosuggestions, syntax-highlighting da sarıyor) bağladığı
  widget'ları ezmek.
- Her sürükleme adımı bir PTY turu; seçim görüntüsü ayna gecikmesine bağlanır.
- Aynaya iki gövde (tel biçimi + çözücü + yük bütçesi).

## Karar 2: Vurgu nasıl çiziliyor?

İstek: tema rengi, yuvarlak köşe, çok satırlı seçimde birleşik şekil.

### Seçenek A: Satır koşuları + kendi pipeline'ı (SDF, köşe başına yarıçap)

`frame()` hücre başına ters çevirmeyi bırakır ve seçimi **satır koşusu**
olarak verir: satır başına (ilk sütun, son sütun), en çok `rows` tane. `bt-gpu`
komşu satırların koşularından her köşenin kaderini çıkarır — açıkta kalan
köşe dışbükey yuvarlak, komşu koşuyla birleşen köşe kare, iki koşunun
basamak yaptığı yerde **içbükey** köşe küçük bir dolgu parçasıyla — ve bunları
yeni bir instance tipiyle (dikdörtgen + dört yarıçap) altıncı bir pipeline'da,
yuvarlak dikdörtgenin imzalı mesafesiyle (`caret_fragment`'in `rounded_box_sdf`'i)
çizer. Çizim sırası: zemin → **seçim** → glyph'ler. Aynı liste dock'un kendi
`setViewport`'unda da kullanılır.

**Artıları:**
- İstenen şeklin tamamı; koşular ızgaranın listelerinde, yani 027'nin kesirli
  ötelemesi ve 011'in yumuşak kayması bedavaya geliyor.
- Kare yolunun ek işi hücre döngüsünde satır başına iki sayı tutmak; koşuların
  geometrisi GPU tarafında ve yalnız seçim varken.
- Seçim opak, yani üst üste binen parçalar koyulaşmaz.

**Eksileri:**
- Altıncı pipeline ve yeni bir `#[repr(C)]` ↔ `.metal` çifti (`make shader`,
  riskli phase).

### Seçenek B: Ters videoyu koru, rengini temadan al

Hücre başına zemin `selection` rolü olur.

**Eksileri:**
- Köşe yok, istenenin yarısı. Reddedilmeye aday.

### Seçenek C: Her koşuya bir draw call, `caret_fragment`'in uniform'larıyla

**Eksileri:**
- Seçili satır başına bir draw call; birleşik şeklin içbükey köşeleri yine yok.

## Karar 3: Seçili metnin rengi

Seçimin zemini `selection` rolü. Metin **hücrenin kendi ön planıyla**
çizilir; ters video çözülür (seçili ters videolu hücre, vim'in durum satırı,
normal ön planıyla okunur — bugünkü `^` kuralının metin yarısı). İkinci bir
`selection_foreground` rolü yok: macOS'un ve Terminal.app'in normu renkli
metni koruyor, sözdizimi renkleri seçimde kaybolmuyor ve rol başına kontrast
hesabı doğmuyor. Bilinen sınır: seçim rengine çok yakın bir ANSI rengi
(koyu temada mavi metin) seçimde zayıf okunur; yanlışın yönü "metin durur".

## Karar 4: "İçerik yaratmaz" kuralının satır koşusundaki hâli

Koşu satırın **ilk çizilir seçili hücresinden son çizilir seçili hücresine**
uzanır, **aradaki boşluklar köprülenir**. Ölçüt (çizilirlik: mürekkep, zemin,
kural ya da spacer) değişmiyor, uygulandığı birim değişiyor: hücre değil
satır. Boş ekranda sürükleme yine hiçbir şey boyamaz, satır sonundaki boşluk
yine vurgusuz; ama kelime arası boşluk artık vurgulu — `selection_to_string`
onu zaten kopyalıyor, yani göz ile pano bugünkünden **daha yakın**. Boş bir
ara satır koşu üretmez ve şekil orada bölünür.

## Karar 5: Kelime nedir?

Tek tanım `bt-core`'da bir sabit: **ayırıcılar** — boşluk, sekme,
`` ` ' " ``, `│ | ; ,`, `=`, parantezlerin altısı (`()[]{}<>`) ve kalan ASCII
noktalama (`! # $ % & * + ? \ ^`). Geriye kelime olarak harf, rakam, ASCII
olmayan her karakter (`│` hariç) ve `_ - . / ~ : @` kalır: yol (`~/src/a-b.rs`),
`user@host`, `host:8080`, `dosya.rs:42`, sorgusuz bir URL tek çift tıkla
seçilir; `KEY=value`'da `value` tek başına. Aynı sabit iki yüzeyi besler:
ızgarada alacritty'nin `semantic_escape_chars`'ı, dock'ta kelime sınırı
arayan saf fonksiyon — iki uygulamayı aynı satır üzerinde karşılaştıran bir
sınama bağlar. (Muhakeme sonrası: davranışın sahibi alacritty'nin `Semantic`'i,
parantez eşleme ve ayırıcı üstünde çift tıklama kuralı dahil; dock onu
kopyalıyor.) **Ayar anahtarı yok**: istek bir varsayılan istiyor, ayar
istemiyor; gerekirse anahtar sonradan eklenir, sabit adı o gün kaynağı olur.

## Karar 6: Tıklama sayısı ve Shift

`clickCount` 1 → `Simple`, 2 → `Semantic`, 3 → `Lines` (alacritty; sarılmış
mantıksal satırı bütün alır, `WRAPLINE`). Sürükleme tipi korur, yani çift
tıklayıp sürüklemek kelime adımıyla büyür. **Shift+tıklama** ızgarada bir seçim
varsa ucunu taşır (tip korunur), yoksa tıklanan noktadan başlar — iki kipte
aynı kural; fare kipinde "baştan başla" yolu herhangi bir tuş (`send_input`
seçimi temizler) ya da seçimsiz Shift+tıklama. Fare kipi
açıkken Shift'in anlamı 020'deki gibi "terminal geri alır" — Shift+tıklama
orada da seçimi uzatır, çünkü fare kipindeki seçimin tek yolu zaten Shift.
Jest defteri `NSEvent` görmeyen bir struct'a taşınır ve sınanır — yol
haritasının "Farenin jest durumu sınanamıyor" kalemi burada kapanır.

## Karar 7: Pencere başına tek seçim; Cmd-C / Cmd-X / Cmd-A

Izgara ya da dock — ikisinden biri. Birinde seçim başlamak ötekini
temizler; Cmd-C sahibin metnini kopyalar. **Edit menüsüne iki öğe:** Cut
(⌘X) ve Select All (⌘A). Cut yalnız dock seçimi varken ve düzenleme kapısı
açıkken etkin (ızgarada kesilecek bir şey yok). Select All dock caret'in
sahibiyken dock'un `BUFFER`'ını, değilse ızgaranın bütün geçmişini seçer
(Terminal.app'in normu). **Cmd izin listesi değişmiyor**: menü öğesi
`performKeyEquivalent:`'la `keyDown:`'dan önce yakalanıyor — 026'nın sekme
kısayollarının yolu.

## Karar 8: Dock'ta tuşların seçimle ilişkisi

Seçim varken ve **düzenleme kapısı** açıkken:

| tuş | davranış |
|---|---|
| ⌫, ⌦ (fn-⌫) | seçimi siler (`d`) |
| yazılan metin (`insertText:`) | `d` + metin olağan yoldan |
| ⌘V | `d` + olağan yapıştırma (sarma kararı `paste`'in) |
| ⌘X | kopya + `d` |
| ← / → | seçim kalkar, caret seçimin başına / sonuna (`c`) |
| ⇧← / ⇧→ | seçimin ucu bir karakter oynar, seçim yoksa caret'ten başlar (terminalde; ZLE'ye gitmez) |
| başka her tuş | seçim kalkar, tuş bugünkü yolundan gider |

Düzenleme kapısı üç koşul, üçü de var olan yüklemler: dock satırın sahibi
(`suppressed_input`), ZLE ekleme keymap'inde (`insert_keymap`) ve ayna son
girdinin cevabı (`answers` güncel nesil). Kapı kapalıyken seçim kopyalanabilir
ama tuşlar bugünkü yolundan gider ve seçim kalkar — `vicmd`'de, çok satırlı
(`Multiline`), gösterilemeyen (`Unavailable`) ya da bayat aynada ZLE'ye hiçbir
komut gitmez. Tek tıklama (sürüklemesiz) caret'i taşır (`c`), aynı kapıyla.
Seçim `BUFFER` değişince kalkar (kullanıcı başka yoldan yazdı). `PREDISPLAY`
ve öneri (`POSTDISPLAY`) seçilmez; öneriye tıklamak caret'i satır sonuna koyar.

## Karar 9: Odaksız pencere

Seçim silinmez, **soluklaşır**: renk `dim_toward` ile zemine doğru üçte bir
(`dim`'in ve adlı renklerin sönüğünün kuralı, üçüncü uygulaması). İki renk
`bt-core`'dan hazır gelir, hangisinin çizileceğini odak bilen `bt-gpu` seçer
— odak `bt-core`'a girmiyor (CLAUDE.md). Geçiş animasyonsuz: emsali yok ve
istenmedi; odak değişimi zaten bir kare istiyor.

## Karar 10: Köşe yarıçapı

`bt_core::CURSOR_RADIUS` × hücre yüksekliği — terminalin köşe dili, caret'in
varsayılanıyla aynı sayı; kırpma koşunun yarım boyuna (SDF ön koşulu).
Kullanıcının `cursor_radius` ayarı seçimi **etkilemez**: anahtar imlecin.

## Karar 11: Kapsam dışı

- Doldurma bandının seçilebilmesi — kendi kalemi var (`docs/YOL-HARITASI.md`
  → "Doldurma bandının satırları seçilemiyor").
- Izgarada tıklayınca ZLE caret'ini taşımak (iTerm'in Option-tıklaması):
  giriş satırı dock'ta.
- Kelime ayırıcı ayarı, seçim animasyonu, dörtlü tıklamayla akıllı seçim
  (URL), dock'un bağlam satırında seçim.
- bash/fish: betikleri henüz yok; dock zsh'e bağlı (CLAUDE.md → Shell
  entegrasyonu). O betikler doğduğunda widget'ın karşılığı orada da yazılır.

## Muhakeme (2026-09-24)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU (hafif) — Karar 1-A en az parçalı yol; fazlalık üç küçük yerde |
| Codebase-fit | SORUNLU — katman düzeni uyuyor; açık bırakılan üç yerleştirme sessizce kırılır |
| İşletme | SORUNLU — bağlamanın yokluğu **ölçüldü** ve satırı bozuyor; phase 2 tek parça geri alınamaz |

**Kabul edilen itirazlar → plan değişikliği:**

- **Bağlama yoksa dizi satırı bozuyor** (İşletme, ölçüldü: `bindkey -e`'de
  sondaki BEL `send-break`, satır ölüyor ve ardından yazılan harf yeni satırda
  komut olarak koşuyor; `bindkey -v`'de `ESC` `vicmd`'ye geçip `~ c ;` komut
  oluyor, tampon bozulup çalışıyor) ve kapının üç koşulu bağlamanın varlığını
  sormuyor (Codebase-fit: `main`'e bağlı kullanıcı keymap'i). → Bağlama
  `main`, `emacs`, `viins`'e ve **her `line-init`'te yeniden** kuruluyor
  (yerleşik, fork yok; ertelenmiş eklentinin sıfırlamasını her prompt'ta
  onarıyor ve `main`'e bağlamak `bindkey -A mymap main` yapan kullanıcının
  keymap'ini de kapsıyor). Kurulumdan hemen sonra ayna **yeteneği** bildiriyor
  (`8133;w`); terminal kapısının dördüncü koşulu bu prompt'ta yeteneğin
  görülmüş olması (`line-finish` siler). Betiği eski ya da hiç olmayan oturum
  dizi göndermez — yanlışın yönü "düzenleme yok".
- **İki komut gereksiz** (Sadelik): `c;N;L`, `d;N;N;L`'nin ta kendisi → tek
  komut `d;S;E;L`; boş aralık = caret'i taşı, `BUFFER` değişmediği için geri
  alma kaydı da doğmuyor.
- **Yeni `#[repr(C)]` çifti gereksiz** (Sadelik): yarıçapın iki değeri var
  (0 ya da tek sayı), yani köşe bilgisi dört bitlik bir maske; renk çizim
  başına tek. → Var olan 32 baytlık `Instance` aynen, `rgba` yuvası köşe
  maskesini taşıyor, renk ve yarıçap çıplak uniform (`caret_fragment`'in
  hizalama kaçışı). Kullanıcının gördüğü değişmiyor.
- **Phase 2 tek parça** (İşletme): rol + koşular + pipeline aynı commit'te,
  revert hepsini götürür. → İkiye bölündü: önce rol + koşular **düz**
  dörtgenlerle (`cell_bg`, shader değişmez), sonra köşeler (riskli phase);
  ikincisi geri alınırsa düz vurgu kalır. Yeni liste sayaçlardan muaf
  (`hucre=`/`glif=`/`kural=` oynamaz) ve boyadığını temadan bağımsız ara
  tonlu bir offscreen bekçi görür (`MIDTONE` emsali).
- **Yerleştirme** (Codebase-fit): dock seçimi `DockState`'in **içine**
  konamaz — `dock::change`/`diff` onu karşılaştırır ve her sürükleme adımı
  030'un efektlerini `Reset`'lerdi; `DockState::reset` de silerdi. → Seçim
  aynanın **yanında** (`DockContext`, `answers` emsali), çizilen hâli
  `Dock::selection` (`Dock::caret` emsali). Izgara koşuları `Cursor`'a
  giremez (`Copy`) → çağıranın sahip olduğu `&mut` tampon (`Blocks`
  emsali). Koşu takibi atlama kapısından **önce**: ters çevirme kalkınca
  varsayılan zeminli spacer kapıya takılır ve seçili CJK'nın sağ yarısı
  koşudan düşerdi (014'ün kapattığı kusur).
- **alacritty'nin `Semantic`'i Karar 5'i yazıldığı gibi vermiyor**
  (Codebase-fit, kaynak `alacritty_terminal-0.26.0`): parantez üstünde çift
  tıklama eşleşen paranteze kadar seçiyor (`range_semantic` →
  `bracket_search`), ayırıcı üstünde çift tıklama iki yandaki kelimeyi
  ayırıcıyla birlikte seçiyor (`ab cd ef` → `ab cd`); `:` varsayılan
  ayırıcılarda. → Tanımın sahibi alacritty'nin davranışı: ızgara onu aynen
  kullanır, `semantic_escape_chars` `term_config`'te sabitten yazılır
  (`term_config_keeps_every_other_field` aynı phase'te değişir), dock'un saf
  fonksiyonu **iki kuralı da** kopyalar ve sınama ikisini parantez ve ayırıcı
  vakaları dahil aynı dizgilerde karşılaştırır. Karar 5'teki `│` çelişkisi
  düzeldi. `pub` API alacritty tipi göstermez → `bt-core`'un kendi
  `SelectKind { Simple, Word, Line }`'ı.
- **Sütun→karakter eşlemesinin tek sahibi yok** (Sadelik + Codebase-fit):
  yürüyüş `render`'ın döngüsüne gömülü. → Yürüyüş `render`, `diff` ve
  isabet testinin ortak tükettiği bir iteratöre çıkıyor; isabet testi
  **çizilen** aynaya karşı (son çizilen pencerenin kayması ve `BUFFER`
  uzunluğu yaprak bir yuvada — `bt_gpu::Origin` emsali), canlı olana değil.
- **Tek huni** (Codebase-fit): `send_input` ızgara seçimini zaten temizliyor
  ve nesli artırıyor → dock seçimini de orada temizler; widget komutu da o
  huniden geçer (tazelik kapısı bedavaya).
- **Cut'ın etkinliği** `validateMenuItem:` ister ve depoda yok → phase'e
  adıyla girdi.
- **Belge yükü** (İşletme): CLAUDE.md'de "dokuz rol", "Pipeline dört" (zaten
  bayat: `glyph_fx` beşinci), "Seçim içeriği vurgular" paragrafı, Edit menü
  listesi, `keyDown:` arbitrajı; `docs/AYARLAR.md` Biçim tablosu ("Altı rol")
  ve iki gömülü tablo; betiğin tel başlığı (ilk kez terminalden kabuğa bir
  yön). Her phase kendi cümlesini aynı commit'te düzeltir.
- **`read -k -t`'nin süresi** gerekçeli bir adlı sabit olarak yazılır,
  çıplak sayı değil.

**Reddedilenler:**

- **İçbükey köşe dolgusu kalksın** (Sadelik, ürün sorusu olarak getirdi) —
  istek "border radius ile güzel gösterme" ve "temiz UI/UX işçiliği";
  basamaktaki keskin iç köşe gözün gördüğü bir fark. Boşlukta kullanıcı
  tarafı: dolgu kalıyor, maliyeti aynı `Instance`'ın maskesinde bir değer ve
  fragment'te bir kol.
- **⇧←/⇧→ çıksın** (Sadelik, ürün sorusu) — çıkarmak yerine **tamamlandı**:
  seçim yokken ⇧← caret'ten seçim başlatır (caret'in yeri aynada), yani
  "klavye seçim başlatamıyor" kusuru kalkıyor. Metin alanı beklentisi.
- **Açık kullanıcı temasında eksik `selection` temanın kendi renklerinden
  türesin** (İşletme, ürün sorusu) — tema dosyasının kuralı istisnasız
  (CLAUDE.md → Tema; 014 aynı gerekçeyle `cursor`'ın istisnasını kaldırdı).
  Gömülü `bateri-light`'ı gölgeleyen dosya tabanını ondan alıyor; adı başka
  olan açık tema koyu tabanın seçim rengini alır ve metin kendi rengiyle
  durur — `dim`'in kabul edilmiş sınırının aynısı, belgeye yazılır.
- **Geçmişe kaydırılmışken dock'a tıklamak ızgarayı dibe atar** (Codebase-fit,
  ürün sorusu) — tıklama caret'i taşıyan bir düzenleme ve yazmak da bugün dibe
  atıyor; aynı huni, aynı davranış. Yalnız seçmek (sürükleme, çift tıklama)
  ZLE'ye hiçbir şey göndermez ve pencereyi oynatmaz.

## Karar (2026-09-24, otonom akış)

Panelden geçmiş öneri; KIRMIZI yok, ikinci tur gerekmedi. Ürün sorusu olarak
gelen dört bulgu `## Muhakeme` → Reddedilenler'de boşlukta kullanıcı tarafı
ilkesiyle kapandı.

- **Seçilen — Karar 1: Seçenek A**, panelin iki düzeltmesiyle. Seçim
  `bt-core`'da, aynanın yanında; ZLE'ye yalnız eylem anında **tek** komut
  gider: `\e[8133~d;S;E;L\a` (`[S,E)`'yi sil, caret `S`'e; `S == E` caret'i
  taşır; `L` tutmazsa no-op). Yazılan/yapıştırılan metin olağan yoldan.
  Widget `main`/`emacs`/`viins`'e her `line-init`'te yeniden bağlanır ve
  ayna yeteneği bildirir (`8133;w`); düzenleme kapısı dört koşul: dock
  sahibi, ekleme keymap'i, ayna güncel nesle cevap, yetenek bu prompt'ta
  görüldü. Gerekçe: ZLE'nin davranışına ve kullanıcının bağlamalarına
  dokunmayan tek yol; ölçüldü (`context.md` → Ölçüm), bağlamasız hâli de
  ölçüldü ve kapıyla kapandı.
- **Reddedilen — Karar 1-B** (sentez tuşları): autosuggestions'ın `→`'si
  öneriyi kabul eder, `vicmd`'de tuşların anlamı başka, N tuş N ayna turu.
- **Reddedilen — Karar 1-C** (ZLE bölgesi): istenen davranış `self-insert` ve
  `backward-delete-char`'ı sarmayı, yani kullanıcının ve eklentilerinin
  widget'larını ezmeyi gerektiriyor.
- **Seçilen — Karar 2: Seçenek A**, iki aşamada. Satır koşuları `frame()`'den
  çağıranın `&mut` tamponuna (takip atlama kapısından önce); önce düz
  dörtgen (`cell_bg`), sonra yuvarlak şekil: var olan `Instance`, köşe maskesi
  `rgba` yuvasında (dışbükey, kare, içbükey dolgu), renk ve yarıçap uniform,
  `rounded_box_sdf`. Gerekçe: istenen şeklin tamamı, yeni `#[repr(C)]` çifti
  yok, ikinci aşama geri alınırsa düz vurgu kalır.
- **Reddedilen — Karar 2-B** (renkli ters video): köşe yok. **2-C** (koşu
  başına draw call): içbükey köşe yok, satır başına draw call.
- **Karar 3–11** yazıldığı gibi, Muhakeme'nin düzeltmeleriyle: kelime
  davranışının sahibi alacritty'nin `Semantic`'i ve dock onu kopyalıyor
  (sabit `bt-core`'da, `semantic_escape_chars`'a yazılıyor, ayar yok);
  `SelectKind` `bt-core`'un kendi tipi; tek huni `send_input`; ⇧←/⇧→ seçimi
  caret'ten başlatabiliyor; Cut `validateMenuItem:` ile; eksik `selection`
  tabandan (istisnasız kural); odaksız renk `bt-core`'dan hazır, seçimi
  `bt-gpu` yapıyor; yarıçap `caret_radius_px(cell_px, CURSOR_RADIUS)`.
