# Input Dock ve prompt'un devri — Tartışma

Model sorusu (ayna mı asıl mı) **011'de kapandı: ayna (8b)**, gerekçesi ve
elenenler `context.md` → Kanıt. Bu dosya aynanın *nasıl* kurulacağını ve 011'in
bilerek bıraktığı dikişi tartışıyor.

## Karar 1: Bu set ne taşıyor?

Yol haritası 012'ye beş iş yazıyor (`YOL-HARITASI.md:54`): prompt'un devri,
`>`, dock'un kendisi, çıpanın `preexec`'e taşınması, tuş vuruşu/silme
animasyonları. Beşi "giriş satırı artık terminalin" başlığı altında **birbirine
kilitli** görünüyor, ama kilit her yönde aynı sıkılıkta değil.

- **1a — beşi birden.** Yol haritasının yazdığı hâl.
  - *Artısı:* ara durum yok; kullanıcı tek seferde bitmiş bir giriş satırı
    görüyor.
  - *Eksisi:* 011'in **iki turda daralmasının** sebebi tam olarak buydu. Beş
    işin üçü (ayna tesisatı, dock yerleşimi, `>`'in yerleştirilmesi) ayrı ayrı
    tasarım turu isteyen büyüklükte; animasyonlar ise `bt-gpu::motion`'ın
    **üçüncü** tüketicisi ve boşta sıfır kare kapısı onu ayrıca sınayacak.
- **1b — dörde in, animasyonları ayır.** Prompt devri + `>` + dock + çıpa bu
  sette; `keypress`/`delete_mode` sonraki sete.
  - *Artısı:* animasyonlar aynanın **üstüne** kuruluyor, yani onlar için
    doğru sıra zaten bu. Dock inmeden "bu harfi kullanıcı mı yazdı" sorusunun
    cevabı yok, yani ayırma bir şeyi bloke etmiyor.
  - *Eksisi:* dock animasyonsuz inerse ilk izlenim "sade bir satır"; referansın
    hissi eksik kalıyor.
- **1c — ikiye in: önce prompt'un devri, dock sonra.** Bu set yalnız prompt'u
  devralır (sıfır genişlik PS1/RPS1 + `>` + safha + çıpanın taşınması); ayna ve
  dock alanı bir sonraki sete.
  - *Artısı:* `>`'in yerleştirilmesi (Karar 4) tek başına bir `Frame` işi ve
    ayna tesisatından **bağımsız** doğrulanabiliyor.
  - *Eksisi:* 011'in tam olarak düştüğü tuzak: prompt'u devralıp yerine yalnız
    bir işaret koymak, kullanıcının prompt'unu alıp karşılığını **ödememek**
    oluyor. 011 bu yüzden 2a'yı buraya taşıdı; aynı gerekçe 1c'yi de vuruyor.

**Önerim: 1b.** 1c'yi 011'in kendi kararı eliyor. 1a ile 1b arasındaki fark
animasyonlar ve onlar aynanın üstüne kuruluyor — yani ayırmak sırayı bozmuyor,
yalnız seti bir tur küçültüyor.

## Karar 2: Dock alanı kalıcı mı, geri verilebilir mi?

Setin **en sert** kararı ve iki kayıt birbiriyle çelişiyor: 011 Karar 9a
"satırı kalıcı ayır" dedi (`rows = (height_px − dock_px)/cell_h`,
görünürlükten bağımsız; deponun yatayda verdiği cevabın dikey ikizi, 010
Karar 3), ama `ARASTIRMA.md:56` referansın dock alanını **geri aldığını**
söylüyor.

- **2a — kalıcı ayır (011 Karar 9a).** Dock satırı hep ayrılmış.
  - *Artısı:* resize yok. 9b'nin elenme gerekçesi ölçülüydü: görünürlüğe bağlı
    ayırma **komut başına iki resize** demek ve her birinin bedeli sayılı —
    `Term` kilidi (okuyucunun ayrıştırma lease'inin arkasında), ring
    rotasyonu, `TIOCSWINSZ`/SIGWINCH, tam yeniden çizim ve `link.rs` →
    `motion.rs` yoluyla **her komutta imleç snap'i**, yani 008'in ürün
    kararının komut başına iptali.
  - *Eksisi:* Claude/Codex/REPL koşarken dipte ölü bir şerit kalır —
    referansın bilerek kaçındığı şey.
- **2b — safhaya bağlı geri ver.** `ShellPhase` `Input` değilken dock kapanır,
  satır ızgaraya döner.
  - *Artısı:* referansın davranışı; ve bizde "davranışla tespit" **tahmin
    değil**: OSC 133 safhası elde. Claude da Codex de komut olarak koşuyor,
    yani `Running` safhası onları bedavaya kapsıyor.
  - *Eksisi:* 9b'nin ölçülmüş bedeli aynen geçerli — komut başına iki resize.
    Referansın bunu nasıl ödediği `ARASTIRMA.md`'de **yazmıyor**.
- **2c — kalıcı ayır, ama alternatif ekranı muaf tut.** Dock satırı hep
  ayrılmış; alternatif ekrana geçildiğinde (vim, htop, Claude TUI) ızgara
  **tam boya** dönüyor.
  - *Artısı:* "alanı geri alma"nın gözle görülen yarısını, resize'ı komut
    başına değil **ekran sahibi değiştiğinde** ödeyerek alıyor — o an zaten
    tam yeniden çizim oluyor ve 011 ötelemeyi orada snap'lemeye karar verdi,
    yani imleç snap'i **ek bir maliyet değil**.
  - *Eksisi:* alternatif ekran kullanmayan bir REPL (`python`, `node`) dock'u
    geri almıyor; o durumda dipte ölü şerit 2a'daki gibi kalıyor.

**Önerim: 2c.** 2a'nın ucuzluğunu alıyor, geri vermenin **pahalı olmayan**
yarısını ödüyor ve bizdeki tek net sinyale (alternatif ekran) yaslanıyor.
Referansın "davranışla tespit"i envanterde tanımsız olduğu için taklit
edilemiyor; 2b'yi seçmek ölçülmüş bir bedeli tanımsız bir kazanç için ödemek
olurdu.

## Karar 3: Aynanın görsel dikişi — ZLE'nin `BUFFER` olmayan çıktısı

011'in bu sete bıraktığı iş. Ayna `BUFFER`/`CURSOR` taşıyor; tamamlama
listesi, `menu-select`, `bck-i-search:`, `zle -M` ve `CORRECT`'in `[nyae]`'i
aynada **yok** ve ızgaraya düşüyor — yani kullanıcı dock'ta yazıyor, Tab'a
basınca liste **başka yerde** beliriyor.

- **3a — dikişi kabul et, adıyla belgele.** Dock `BUFFER`'ı gösterir, ZLE'nin
  geri kalanı ızgarada belirir.
  - *Artısı:* sıfır ek tesisat; ve ZLE'nin o çıktıları zaten **geçici**
    (listeden seçince kayboluyor).
  - *Eksisi:* iki giriş yüzeyi aynı anda görünür — referansın "sabit ayrı satır
    editörü" hissi tam orada kırılıyor.
- **3b — ZLE özel kipteyken dock'u devre dışı bırak.** Tamamlama/arama
  başlayınca dock soluklaşır ya da gizlenir, ızgara devralır.
  - *Artısı:* iki yüzey aynı anda **iddia sahibi** olmuyor; kullanıcı nereye
    bakacağını biliyor.
  - *Eksisi:* "özel kip"in sinyali yok. `zle -M` ve tamamlama ZLE içinde ayırt
    edilebilir ama bunu terminale bildiren bir kanal yok — yeni bir OSC daha,
    yani Karar 5'in yükü büyüyor.
- **3c — aynayı genişlet: ZLE'nin post-display'ini de taşı.**
  - *Elenir:* ZLE'nin çizdiği her şeyi yakalamak ZLE'yi yeniden uygulamaktır;
    8c'nin (asıl tampon) elenme gerekçesi buraya da geçiyor ve `PAYLOAD_LIMIT`
    bu hacmi zaten taşımıyor.

**Önerim: 3a, ama bir şartla** — dock'un **görünürlüğü** dikişi yumuşatacak
biçimde tasarlanır: dock ızgaranın devamı gibi değil, ayrı bir yüzey gibi
durursa (kendi zemini/ayracı) kullanıcı listeyi "ızgarada" görmeyi yadırgamaz.
3b'yi kapatmıyoruz; sinyali doğduğunda (Karar 5'in kanalı zaten açılıyor)
üstüne eklenebilir.

## Karar 4: `>` nereye çizilir?

`context.md` → Kanıt: payda **dikdörtgen** çizilebiliyor, **glyph**
çizilemiyor; eksik olan sprite değil yerleştirme.

- **4a — dock kendi alanı olduğu için sorun konusuz.** Dock ızgara değilse
  kendi yerleşimi var; `>` orada sıradan bir glyph.
  - *Artısı:* `pos_at`'e ikinci uzay açmak, `GlyphInstance`'a alan eklemek ve
    iki `stride 32` assert'ini taşımak **hiç gerekmiyor**.
  - *Eksisi:* dock'un kendi metin yerleşimini yazmayı gerektiriyor — ama Karar
    1'de dock zaten bu sette.
- **4b — payı genişlet, `pos_at`'e ikinci uzay aç.** *Elenir (bu sette):* 4a
  bedava verirken `pos_at`'in "tek yer" sözleşmesini açmak gereksiz risk.
- **4c — `GlyphInstance`'a `size`.** *Elenir:* iki tarafta `stride 32` assert'i
  kırılır; kazancı yalnız `>` olan bir değişiklik için fazla.

**Önerim: 4a.** Karar 1'de dock bu sette olduğu için `>` bedavaya çizilebilir
hâle geliyor — bu, 1b'yi 1c'ye tercih etmenin ikinci gerekçesi.

## Karar 5: `BUFFER` terminale nasıl taşınır?

Tarayıcı bugün OSC `133`'e kilitli (`shell.rs:486`), `PAYLOAD_LIMIT` 256 bayt
(`shell.rs:372`) ve çıplak `ESC` diziyi bitiriyor (`shell.rs:536`).

- **5a — yeni OSC numarası + base64 + kendi sınırı.** Tarayıcıya ikinci kol.
  - *Açık kalemler:* hangi numara (çakışmayan, `ARASTIRMA.md:42`'nin listesi
    dışında), sınır kaç bayt (uzun bir komut satırı 256'yı rahat aşar),
    sınırı aşan `BUFFER` ne olur — **sessizce düşmemeli**, dock "gösteremiyorum"
    diyebilmeli.
- **5b — OSC 133'ü genişlet.** *Elenir:* 133 bir **standart**; kendi alt
  komutumuzu eklemek başka terminallerde tanımsız davranış üretir ve
  `TERM` sözleşmesinin aynı sınıfı (`proje.md` → tuzaklar).

**Önerim: 5a.** Numara ve sınır plan aşamasında sayıyla konur; sınır aşımının
**görünür** olması şart (sessiz düşüş bu deponun yasakladığı belirti).

## Karar 6: ZLE kancası hangisi?

011 ölçtü: `zle -N` **değil** `add-zle-hook-widget line-pre-redraw` —
zsh-syntax-highlighting ve zsh-autosuggestions aynı widget'ı istiyor ve
`zle -N` onları düşürür. Yanına `line-finish`.

Açık kalem: bugünkü betik ZLE'ye **hiç** dokunmuyor (`bateri.zsh`'te `zle`
yok), yani bu tesisat sıfırdan yazılacak ve `precmd`/`preexec`'in idempotan
kurulum nöbeti (`bateri.zsh:212-221`) burada da gerekir. Kullanıcının rc
dosyasına yazma yasağı (`make denetim`) değişmiyor.

## Karar 7: p10k/starship kullanıcısı ne görecek?

Prompt devralınınca kullanıcının teması **çizilmiyor** ve geri dönüş bugün
hep-ya-hiç (`shell.integration = "off"`, üstelik sonraki oturumda).

- **7a — ayrı anahtar** (`prompt = "terminal" | "shell"`). Ayar şeması büyür.
- **7b — hep-ya-hiç kalsın, belgelensin.** 011'in kararı buydu ama o sette
  prompt devralınmıyordu; bu sette bedel **gerçekleşiyor**.

**Önerim: 7a**, ama şemayı büyütme kararı kullanıcınındır — UX'te "prompt'umu
geri ver" makul bir istek ve tek çıkışın entegrasyonu büsbütün kapatmak olması
(blokları da öldürerek) orantısız.

## Karar Noktaları

1. **Kapsam:** animasyonlar bu sette mi (1a) yoksa sonraki sette mi (1b)?
2. **Dock alanı:** kalıcı (2a), safhaya bağlı (2b) yoksa kalıcı + alternatif
   ekran muafiyeti (2c) mi?
3. **Dikiş:** tamamlama listesi ızgarada kalsın mı (3a), yoksa dock o anlarda
   çekilsin mi (3b)?
4. **Prompt geri dönüşü:** ayrı anahtar (7a) mı, hep-ya-hiç (7b) mi?

## Muhakeme (2026-09-17)

Üç jüri paralel koştu (`opus`); yedi karar noktasını ve `context.md`'nin
kanıtlarını gördü.

| Mercek | Verdict |
|---|---|
| Sadelik | **KIRMIZI** (şartlı: Karar 1'de 1a seçilirse SORUNLU) |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

**Referans doğrulaması (Codebase-fit):** `context.md` ve `discussion.md`'deki
bütün `dosya:satır` referansları kodda tutuyor. İki off-by-one
(`view.rs:448` → `session.write` aslında `:449`; `link.rs:838` doc satırı,
`origin_target` `:840`) ve bir imprecision (`shell_state()`'in "iki çağıranı"
— aslında ~10 çağrı yeri, hepsi `#[cfg(test)]`; yük taşıyan iddia ayakta).
`shell.rs:88`'in `(014)` kayması doğru yakalanmış (014 bugün emoji seti).

### Kabul edilen itirazlar → tasarım değişikliği

1. **8b'nin üçüncü bedeli 012'de hiçbir kararın konusu değil — üç jüri
   birden, blokaj.** 011 `discussion.md:334-340` aynanın bedelini üç katmanda
   saymış; (i) ZLE kancası Karar 6'ya, (ii) OSC kolu Karar 5'e taşınmış,
   **(iii) "ZLE aynı metni ızgaraya da çiziyor, yani bastırma tutamağı
   gerekiyor" düşmüş.** Sıfır genişlik PS1 *prompt'u* gizliyor, ZLE'nin
   `BUFFER` **yankısını** değil. İki kol da bugün sahipsiz:
   - *Bastırılmazsa:* kullanıcı komutunu **iki yerde** görür (ızgarada +
     dock'ta) — 8c'nin elenme gerekçesi geri kapıdan giriyor.
   - *Bastırılırsa:* mekanizma yok. Kabukta bastırmak ZLE'yi susturmak
     demek (Tab/geçmiş/Ctrl-R ölür); terminalde bastırmak iki yeni tesisat
     ister — safha `shell` yaprak kilidinde, sink `Term` kilidi altında ve
     ikisi hiç iç içe girmiyor (`session.rs:26`, `:1572-1576`), yani `Theme`
     gibi **kilit öncesi** bir kopya gerekir; ve sıfır genişlikli prompt'tan
     sonraki sütun aralığı sink'e bilinmiyor. Üstelik bastırılacak hücreler
     **çıpayı taşıyan** hücreler (`session.rs:1425`), yani naif bastırma blok
     şeridini de öldürür.
   → **Karar 3 bu karar verilmeden cevaplanamaz:** 3a "geri kalanı ızgarada
   belirir" derken ızgara satırının görünür olduğunu varsayıyor.
2. **1b ile 8b birbirini iptal ediyor (Sadelik, KIRMIZI'nın kaynağı).** 8a'nın
   kayıtlı **tek** eksisi "tuş vuruşu animasyonları yapılamaz"dı
   (`011/discussion.md:327-329`); 1b tam o animasyonları sonraki sete atıyor.
   Yani 1b içinde ayna ile 8a arasında kullanıcının göreceği fark **kalmıyor**
   ve üç katman tesisat kullanılmayan bir kabiliyet için ödeniyor. Tutarsızlık
   **yeni**: 011'in paneli 1b'yi hiç görmedi, 8b animasyonların **dahil
   olduğu** kapsamda seçildi.
3. **2c elenir (Sadelik + Codebase-fit).** `alt_screen` `pub` sınırını
   geçmiyor; 2c hem yeni bir sınır gerçeği hem geometriden bağımsız bir resize
   tetiği istiyor ve o tetik display link callback'inden `Term` kilidine
   iner — `session.rs:2151-2158`'in kendi uyarısı ve "render yolu bloklanmaz"
   kuralı. `split_into_grid`'in saflığı (`app.rs:388-390`) da bozuluyor: yön
   (pencere geometrisi → grid → resize) ters çevriliyor. Ürün tarafında da
   zayıf: kazandırdığı vim/htop/less, buna karşılık her `git log`/`man`/pager
   iki resize ödüyor ve `ARASTIRMA.md:56`'nın **adıyla saydığı** üç uygulama
   (Claude, Codex, REPL) satır içi çizdiği için 2c'nin muafiyetine **hiç
   girmiyor**.
   → Yerine **2d (İşletme): ayırmayı oturum doğarken karara bağla.**
   "Entegrasyon kuruldu mu" spawn anında zaten biliniyor
   (`child::zsh_wrapper_dir()`); koşu boyunca değişmediği için 9b'nin ölçülmüş
   bedeli **hiç doğmuyor**. Yan kazanç: `/bin/sh` koşan duman reçetesi dock
   almaz, yani `smoke_shell` ve ona bağlı üç sınama **dokunulmadan** kalır.
4. **Sıra tek yönlü ve plana çivilenmeli (İşletme).** Dock **önce**, sıfır
   genişlik PS1/RPS1 **sonra**. Ters sıra promptsuz bir terminal bırakır;
   doğru sıra en kötü çift prompt, yani gürültülü ve zararsız.
5. **Prompt'un devri hiçbir kapının görmediği tek iş (İşletme).** `make duman`
   reçetesi `/bin/sh` (`session.rs:506`), entegrasyon yok; depoda
   `bateri.zsh`'i **koşturan** sınama yok (`app.rs:2953-3103` yalnız demeti
   kuruyor, `child.rs:381` yalnız dosya adına bakıyor); `make kur` betiği
   `cmp` ile karşılaştırıyor, **çalıştırmıyor**. Yani sıfır genişlik inip dock
   inmezse üç kapı da yeşil ve terminal promptsuz.
   → Plan bunu **yazılı** kabul etmeli: ya üçüncü bir yük (dock tanığı) ya da
   "prompt yolu kapısızdır, elle koşu ve `/measure` ile tutulur".
6. **Karar 3'ün listesi eksik (İşletme).** `zsh-autosuggestions`
   (`POSTDISPLAY`) ve `zsh-syntax-highlighting` (`region_highlight`) listede
   yok, oysa en yaygın kurulu iki eklenti onlar ve ikisi de `BUFFER` **değil**,
   yani aynada yoklar. `BUFFER` bastırılırsa ikisi büsbütün kaybolur.
7. **4a'nın "hiç gerekmiyor"u iki kalemi atlıyor (Codebase-fit).** (a) Öteleme
   emsali sink'te kullanılamaz: `set_origin` `sync`'ten **sonra** çağrılıyor
   (`link.rs:779-780`, `:803-805`) ve `Frame::clear` `origin_px`'i sıfırlıyor
   (`frame.rs:297`), yani sink içinde `- origin_px` sıfırla çarpılır — dock
   içeriği sink'ten **sonra** basılmak zorunda. (b) Dock'un kendi caret'i
   bedava değil: `CursorBlock` liste değil **alan** (`frame.rs:249-257`).
   → Temiz çare (Codebase-fit sketch'i): mevcut üç encode'dan **sonra** ikinci
   bir `setViewport` (kimlik) + dock'un **ayrı listeleri**. "Dock ikinci
   koordinat uzayı" aritmetik değil **yapısal** olur; `move_cursor`'ın
   `truncate(bg_count)`'u (`frame.rs:502`) dock listelerini hiç görmez.
8. **Ölçüm çift borç (İşletme).** Aynanın tek gerçek perf riski her tuşta
   `BUFFER`'ın tamamının base64'lenip dönmesi — **tuş başına O(n) bayt**. Net
   etki bir kayıp olmayabilir (gidiş-dönüş yankınınkiyle aynı) ama ölçülmedi
   **ve ölçecek kanca yok** (`BT_INPUT_LATENCY_SAMPLES` `CLAUDE.md`'de borç).
   → "ölçüm bekliyor" tek başına yetmez; **"araç da borç"** diye çift
   etiketlenir, yoksa kapanmayacak bir kutu açılır.
9. **Entegrasyonsuz oturum kapsanmıyor (Codebase-fit + İşletme).** Karar 6
   yalnız zsh; Karar 7 yalnız p10k/starship'i soruyor. bash, fish, SSH,
   `integration = "off"`, iç içe `zsh` ve ilk karakterde bağlantı açan tema —
   hepsinde dock **dolmaz**. Kalıcı ayırma seçilirse dipte ölü şerit kalır ve
   kapatacak anahtar yok; `ARASTIRMA.md:100` referansın `input_dock`
   anahtarını adıyla listeliyor, 012 hiç tartışmıyor.
10. **p10k instant prompt sette hiç geçmiyor (İşletme).** `.zshrc`'miz
    kullanıcının dosyasını top-level `source` ediyor, yani instant prompt
    kancalarımızdan **önce** koşup ilk kareyi kendi prompt'uyla çiziyor.
    Bilinen sınır olarak adlandırılmalı.

### Reddedilenler

- **"1a'ya dön, animasyonları sete al" (Sadelik'in şartlı çıkışı)** — itiraz
  geçerli ama çözüm değil: 1a aynayı hak ettiriyor, ama itiraz 1'in bastırma
  boşluğunu **büyütüyor** (animasyonlar bastırılmış bir ızgara satırının
  üstüne kuruluyor). Kapsamı büyüterek kapanmıyor.
- **"`ARASTIRMA.md:56`'yı taklit et" (örtük)** — envanter "davranışla tespit"in
  kuralını **vermiyor**; taklit edilecek bir şey yok, bkz. `context.md` →
  Motivasyon.

### Panelden sonra: blokajı çözen bulgu (2026-09-17)

Panel "bastırma tutamağı yok" diye bloke etti ve üçü de haklıydı — **ama
sorulan soru yanlıştı.** Panele "ayna `BUFFER` taşır" diye soruldu; oysa ZLE'nin
çizdiği şey `BUFFER` değil, **beş değişken**: `PREDISPLAY` + `BUFFER` +
`POSTDISPLAY`, renklendirmesi `region_highlight`, caret'i `CURSOR`. Beşi de
zsh 5.9'da okunabilir (doğrulandı: `zsh -fc` ile beşi de ZLE bağlamında
tanımlı; `add-zle-hook-widget` yüklenebilir; macOS 14 tabanı zsh 5.9 getiriyor).

Beşini birden taşıyan bir ayna itiraz 1'i ve itiraz 6'yı **birlikte** kapatıyor:

- **Bastırma bilgi kaybı değil.** Dock, ZLE'nin çizeceğinin aynısını çiziyor.
- **En sert itiraz düşüyor:** "bastırma autosuggestions ve syntax
  highlighting'i öldürür" — öldürmüyor, çünkü autosuggestions zaten
  `POSTDISPLAY`, syntax highlighting zaten `region_highlight`; ikisi de aynada
  **geliyor**.
- **Bastırma aralığı hesaplanabilir:** sıfır genişlikli PS1'de giriş
  `prompt_row`'dan `cursor_row`'a, bütün sütunlar. Çıpa taraması glyph
  üretiminden bağımsız (`session.rs:1425` `cell.hyperlink()`), yani blok şeridi
  bastırmadan etkilenmiyor.

**Gerçekten açık kalan iki kalem** (ikisi de plana yazılır, erteleme
gerektirmez): ZLE'nin özel kipleri (`bck-i-search`, `menu-select`, `[nyae]`,
`zle -M`) bu beşin dışında çiziyor ve bastırmanın o anlarda bırakılması
gerekiyor; ve tuş başına O(n) bayt ölçülmedi, ölçecek kanca da yok.

**Kayda değer:** "yapmayalım/erteleyelim" sentezi bu bulgudan **önce**
yazılmıştı. İşi problem bulmak olan üç rapordan bir "vazgeç" kararı çıkarmak
sentezin hatasıydı; kullanıcı itiraz edip sorguladığı için düzeldi.

### Doğrulama notu

Sadelik jürisinin "Claude Code ve Codex satır içi çiziyor, alternatif ekran
kullanmıyor" iddiası **gözlemle** doğrulandı: bu oturum Claude Code'un içinde
koşuyor ve çıktısı terminalin scrollback'inde kalıcı — alternatif ekran
kullanan bir uygulamanın çıktısı çıkışta silinirdi. Codex için aynı gözlem
yapılmadı; iddia o tarafta **doğrulanmamış** kalıyor.

## Karar (2026-09-17, kullanıcı)

Kullanıcı panelin "ertele" sentezine **itiraz etti** ("biz en sonunda
Metalterm'deki dock'a kavuşacak mıyız, yoksa sürekli erteleme ile görevi
yapılmayacak kıvama mı getiriyoruz") ve itiraz haklı çıktı: dock zaten iki kez
ertelenmişti (sıra 2026-09-16'da "Input Dock'a hızlı varmak" diye kısaltılmış,
sonra 011'den 012'ye ayrılmıştı) ve üçüncü erteleme örüntüyü kalıcılaştırırdı.
Sorgulama beş değişkenli ayna bulgusunu doğurdu, o da blokajı çözdü.

- **Karar 8b duruyor ve bu sette iniyor: gerçek ayna dock.** Gerekçe panelin
  itirazından sonra **değişti**: ayna artık animasyonlarla değil **dock'un
  kendisiyle** hak ediliyor — ayrı bir yüzeyde giriş satırını çizmek onun
  içeriğini bilmeyi gerektiriyor, yani ayna dock'un ön şartı. Sadelik
  jürisinin "1b içinde ayna hiçbir şey almıyor" itirazı bu yüzden **düşüyor**:
  o itiraz "dock = kroma" varsayımına dayanıyordu, kullanıcı gerçek yüzeyi
  seçti.
- **Ayna beş değişken taşır:** `PREDISPLAY`, `BUFFER`, `POSTDISPLAY`,
  `region_highlight`, `CURSOR`. Reddedilen: yalnız `BUFFER` taşımak — bastırmayı
  bilgi kaybına çevirir ve en yaygın iki eklentiyi (autosuggestions, syntax
  highlighting) öldürürdü.
- **Karar 1 → 1b.** Tuş vuruşu ve silme animasyonları (`keypress`,
  `delete_mode`) **bu sette değil**. Aynanın gerekçesi artık onlara bağlı
  olmadığı için ayırma seti hollow bırakmıyor.
- **Karar 2 → dock alternatif ekranda KALKAR** (kullanıcı, çizim üzerine:
  "alternatif ekranını sevmedim, vim modunda falan dock kalkmalı").
  2a/2c/2d'nin hepsi elendi: üçü de dock'u vim'de yerinde bırakıyordu.
  - Bedeli kabul edildi ve adıyla duruyor: dock **gerçekten satır alıyor**,
    yani kalkması `TIOCSWINSZ` + SIGWINCH + tam yeniden çizim demek. Panelin
    9b itirazının *ürün* yarısı düşüyor (bedel **komut başına değil, alternatif
    ekrana giriş/çıkış başına**), *mekanik* yarısı tasarımla karşılanıyor:
    resize render yolundan **çağrılmaz**, `dispatch2` ana kuyruğundan bir
    sonraki turda koşar — depodaki emsali `child_exit` ve OSC 52'nin pano işi.
  - **Reddedilen: tam boy ızgara + overlay** (hiç resize istemiyordu). Kabuk
    pencerenin tamamına yazdığını sanır ve dolu ekranda en alttaki dock satırı
    kadar çıktı **sessizce kaybolurdu**; bu deponun yasakladığı belirti sınıfı.
- **Karar 2 eki → dock yalnız entegrasyonlu zsh oturumunda var.** bash, fish,
  SSH ve `integration = "off"` oturumunda pencere **tamamen ızgara**; dizin ve
  dal satırı da onunla gider. Kullanıcı: "şimdilik ok, sonra bakarız."
  Ayırma kararı oturum doğarken veriliyor (`child::zsh_wrapper_dir()` spawn
  anında biliyor), yani koşu boyunca oynamıyor ve `/bin/sh` koşan duman
  reçetesi dock **almıyor** — `smoke_shell` ve ona bağlı üç sınama dokunulmadan
  kalıyor.
- **Karar 3 → 3a**, listesi **genişletilerek**: dock ZLE'nin beş değişkenini
  çiziyor, yani `POSTDISPLAY` ve `region_highlight` aynada. Dışarıda kalanlar
  (tamamlama listesi, `menu-select`, `bck-i-search`, `zle -M`, `[nyae]`)
  ızgarada belirir ve o anlarda **bastırma bırakılır** — tetiği plan aşamasında
  adlandırılır.
- **Karar 4 → 4a**, panelin düzeltmesiyle: dock kendi yüzeyi olduğu için `>`
  sıradan bir glyph. Yerleşim **ikinci bir `setViewport`** ile yapısal olarak
  ayrılır (Codebase-fit sketch'i); dock listeleri ayrı, yani
  `move_cursor`'ın `truncate(bg_count)`'u onları görmez ve dock içeriği
  sink'ten **sonra** basılır (`Frame::clear` `origin_px`'i sıfırladığı için
  sink içinde öteleme emsali kullanılamıyor).
- **Karar 5 → 5a.** Yeni OSC numarası + base64 + kendi sınırı; sınır aşımı
  **görünür** olmak zorunda (bugünkü `Skip` kolu çağırana sinyal vermiyor).
  Tarayıcı okuma yolunda olduğu için bu phase `make test-yaris` tetikler ve
  **riskli phase**tir.
- **Karar 6 → `add-zle-hook-widget line-pre-redraw` + `line-finish`**, `zle -N`
  değil (zsh-syntax-highlighting ve autosuggestions aynı widget'ı istiyor).
  Kurulum PS1'inki gibi idempotan nöbetli.
- **Karar 7 → 7a: ayrı anahtar.** Prompt'u geri isteyen kullanıcının tek
  çıkışının entegrasyonu büsbütün kapatmak (blokları da öldürerek) olması
  orantısız.
- **Kapsam büyüdü: dock iki satır** (kullanıcı). İkinci satır sol altta
  **`[tam klasör yolu] | [git dalı]`**, yan yana. İki sonucu var ve ikisi de
  kabul edildi:
  - **Dizin OSC 7 demek** ve bugün hiç bağlı değil (`Event::Title`/`ResetTitle`
    sessizce düşüyor). 011 Karar 12 dizini tam bu yüzden kapsam dışı
    bırakmıştı; o karar **bilerek** geri alınıyor.
  - **Dal kabuktan gelir, terminalden değil:** `precmd` zaten koşuyor ve dalı
    aynanın kanalından gönderir. Bedeli **prompt başına bir fork**
    (`git rev-parse --abbrev-ref HEAD`); büyük depoda hissedilir (p10k'nın
    `gitstatusd`'si bu yüzden var). Hızlandırma ayrı bir iş.
  - **Taşma kuralı:** yol **soldan** kısaltılır (kuyruk daha bilgilendirici),
    dal asla kısalmaz.
- **Sıra kısıtı plana çivilenir** (İşletme itirazı 4/5): **dock önce, sıfır
  genişlik PS1/RPS1 sonra.** Ters sıra promptsuz bir terminal bırakır ve üç
  kapı da yeşil kalır — prompt yolunu hiçbir kapı görmüyor (`smoke_shell`
  `/bin/sh`, `bateri.zsh`'i koşturan sınama yok, `make kur` `cmp`'liyor).
- **Ölçüm çift borç:** tuş başına O(n) bayt ölçülmedi **ve** ölçecek kanca yok
  (`BT_INPUT_LATENCY_SAMPLES` borç). Phase "ölçüm bekliyor + araç da borç" diye
  çift etiketler; `/measure` bugün kapatamaz.
- **Bilinen sınır olarak adlandırılacaklar:** p10k instant prompt (kullanıcının
  `.zshrc`'si top-level `source` edildiği için kancalarımızdan önce koşup ilk
  kareyi kendi prompt'uyla çiziyor); `psvar[9]`'un düşmesi (artık bedeli şerit
  değil, prompt devredilmişken kimliğin kaybı); `zle -I` iş bildiriminin çıpa
  satırını kaydırması.
