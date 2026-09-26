# ssh modunun ikinci turu — Tartışma

Birbirinden bağımsız karar noktaları. Kullanıcının 036'nın sohbetinde
verdiği kararlar (desen listesi, sırayla eşleşme, menüde anlam, addan tahminin
reddi) `docs/YOL-HARITASI.md`'nin 037 satırında ve burada yeniden
tartışılmıyor. Karar 7 (Finder) ürün kararı ve kullanıcıya gidiyor; geri
kalanı teknik ya da bariz kullanıcı beklentisi.

## Karar 1: uzak hedefin modeli → ✅ yoklama argv'yi de taşıyor, `bt-core` hedefi bütün olarak tutuyor

Dört maddenin üçü host'tan fazlasını istiyor: ⌘T ve yeniden bağlanma aynı
komutu yeniden koşturmalı (port, `-i`, `-J` olmadan ikinci bağlantı kurulamaz),
yükleme ssh'ın seçeneklerini `scp`'ye taşımalı. Bugün yoklama argv'yi okuyup
atıyor (context.md → Mevcut Durum).

- `jobs::Probe::Remote` host yerine bir hedef taşıyor: host (bugünkü gibi
  yazıldığı hâliyle), tür (ssh / mosh) ve **yeniden koşturulacak argv**.
  argv'nin kuralı Karar 5'te.
- `Session::set_remote` hedefi bütün olarak alıyor; `bt-core` onu
  `DockContext`'te, host'un bugünkü yuvasının yanında tutuyor ve `C`/`D`/`A`
  kuralı değişmiyor. Yazılacak **satır** (argv'nin kabuk için kaçırılmış
  hâli) `bt-shell`'de `quote::shell_quote`'tan üretilip hedefle birlikte
  veriliyor: `bt-core` kaçırma kuralını ikinci kez yazmıyor, yalnız dizgiyi
  saklıyor.
- **Satır okunur kalmalı**, çünkü kullanıcı onu yeni sekmenin dock'unda ve
  geçmişte görüyor. `shell_quote`'un beyaz listesi damla için dar (`@`, `:`,
  `=` kaçıyor) ve olduğu gibi kullanılsa satır `ssh deploy\@prod -o
  User\=x` okunurdu. Bu çağıran için liste genişliyor: `@ : , +` her yerde,
  `=` sözcüğün başı dışında (zsh'in `EQUALS`'ı `=cmd`'yi yalnız sözcük başında
  açıyor); kalanı bugünkü kural. Genişleme ayrı bir giriş noktası, damlanın
  kuralı değişmiyor.
- Satır **süreçten**, kullanıcının yazdığından değil: `alias s=ssh` ile
  `s prod` yazan kullanıcı yeni sekmede `ssh prod` görüyor. Doğru komut ve
  036'nın "takma adı görür" kararının doğal sonucu.
- `bt-core` yine ne pid ne `libc` görüyor; taşınan şey dizgi.

## Karar 2: host'un işareti → ✅ sıralı bir dizi, el yazması glob, `user@` yalnız istenirse

**Biçim.** Yol haritası "sırayla, ilk eşleşen kazanır" diyor; TOML'da
tablonun anahtarları anlamca sırasız, yani sıra ancak dizide dürüst:

```toml
[remote]
hosts = [
  { host = "prod-*", mark = "production" },
  { host = "*.staging.example.com", mark = "staging" },
  { host = "vm", mark = "#c678dd" },
]
```

**Değerler.** `production`, `staging`, `development`, `none` ya da `#rrggbb`.
`none` "işaretsiz" demek (bugünkü `info`) ve eşleşmeyi orada **bitiriyor** —
bir globun yakaladığı tek bir host'u işaretsiz bırakmanın tek yolu bu.
**Doğrudan renk kabul ediliyor** (kullanıcı lehine, maliyeti bir ayrıştırma
kolu — tema dosyasının hex ayrıştırıcısı zaten var). Bedeli adıyla: o renk
temayla değişmiyor, açık temada okunur olacağını kimse denetlemiyor; menü onu
hiç yazmıyor, yalnız dosyayı elle düzenleyen seçer.

**Desen.** `*` (boş dahil herhangi bir dizi, nokta dahil) ve `?` (tek
karakter); büyük/küçük harf duyarsız (host adları öyle). Sınıf (`[a-z]`) ve
küme (`{a,b}`) yok — onlar ayrı bir glob kütüphanesi demek ve yeni bağımlılık
pahalı karar sınıfı. Eşleştirici `bt-core`'da saf, yirmi satırlık bir fonksiyon.

**Eşleşmenin girdisi** 036'nın gösterdiği host (Karar 3: kullanıcının yazdığı
gibi, `ssh://` şeması ve port atılmış). Desende `@` yoksa girdinin **son
`@`'ten sonraki** kısmı eşleşiyor — `deploy@prod` ile `prod` aynı makine ve
kullanıcı onları host adıyla düşünür. Desende `@` varsa girdinin tamamı:
`root@*` kuralı yazılabiliyor. `~/.ssh/config`'in `HostName`'i çözülmüyor
(036'nın kapsam dışı kararı); takma adla bağlanan kullanıcı deseni takma ada
yazar.

**Hata.** Bozuk bir girdi (bilinmeyen `mark`, `host`'suz satır) anahtarın
tamamını reddediyor ve önceki değer kalıyor, tanı alt başlıkta —
`Settings::parse_keeping`'in bugünkü kuralı, istisnası yok. Varsayılan boş
dizi; eski bir anahtar yok.

**Çözümün yeri.** Desen `set_remote` ve ayar değişiminde (`Session`'a yeni bir
setter, `set_theme` emsali, canlı) bir kez çözülüyor ve sonuç (işaret)
hedefin yanında duruyor. Kare yolu desen görmüyor: her karede eşleştirmek
pahalı karar sınıfının "kare yolunda CPU" kalemi olurdu ve cevap yalnız iki
kenarda değişiyor.

## Karar 3: renk → ✅ anlam temanın rolüne, `warning` çizilmeye başlıyor

Production → `error`, Staging → `warning`, Development → `success`, `none`
ve eşleşmeyen → bugünkü `info`, `#rrggbb` → kendisi. Yol haritasının önerdiği
eşleme; anlam ile renk ayrık kaldığı için açık/koyu tema geçişi işareti
kendiliğinden taşıyor.

`warning` dokuz rolün çizilmeyen sonuncusu. Değeri `info`'nun emsaliyle
temanın kendi ANSI sarısı: `bateri`'de `#d6b16a`, `bateri-light`'ta
`#8f6a00`. Tema dosyasında opsiyonel anahtar, eksikse gömülü tabandan.
`the_info_role_reads_on_the_ground` bekçisi `warning`'i de kapsıyor (zeminde
3:1 — `⇄ host` metin). `cursor` altın ve paletin sarısından bilerek ayrık
(`color.rs`'in yorumu), yani yeni rol imleçle karışmıyor.

Rengi alan üç yer yol haritasındaki gibi: `⇄ host`, dock'un üst saç çizgisi
ve sekme (Karar 4).

## Karar 4: sekmede renk → ✅ `accessoryView`'da küçük bir nokta, yalnız işaretli host'ta

AppKit verebiliyor (context.md → Kanıt). İki yoldan `setAttributedTitle`
**reddedildi**: pencerenin başlığını eziyor, yani 036 Karar 5'in reddettiği
ikinci başlık yazarı. Seçilen `setAccessoryView`: başlığın yanında küçük,
dolu, yuvarlak bir nokta, işaretin renginde; çizimi 033'ün panel emsaliyle
`NSBox` (katman yoluyla `CGColor` istemek `objc2-core-graphics` kenarı
eklerdi). Özellik bayrağı `NSWindowTab` `bt-shell`'in `objc2-app-kit`
listesine ekleniyor — yalnız başlık bayrağı, `Cargo.lock` oynamıyor (029/033
emsali).

**Nokta yalnız işaretli host'ta** (production/staging/development/doğrudan
renk). İşaretsiz uzak sekme zaten başlığında `⇄` taşıyor; her ssh sekmesine
bir `info` noktası koymak noktayı "ortam" anlamından "uzak" anlamına indirir
ve prod'un kırmızısını sulandırırdı.

**Sınır:** nokta yalnız sekme çubuğu görünürken var — tek sekmeli pencerenin
başlık çubuğunda karşılığı yok; oradaki gösterge dock'un üst çizgisi. Gerçek
pencerede görünüşü ölçülmedi, gözle kontrol ilgili phase'de.

Tazeleme kenarları başlığınkiyle aynı (`set_remote` ana thread'de, `D`/`A`'nın
silmesi `title_changed`'den), artı ayar ve tema değişimi.

## Karar 5: menüden işaretleme → ✅ Shell ▸ "Mark “{host}” as ▸", dosyaya tek düzenleme

Shell menüsünde dinamik başlıklı bir alt menü: Production / Staging /
Development / None, **geçerli çözümün** yanında onay işareti (glob'dan gelse
de). Uzak olmayan sekmede öğe "Mark Host as" adıyla gri. Başlıktaki host
`user@`'siz kısım (Karar 2'nin eşleşme girdisi).

Menü yalnız **yazar**, uygulayan dosyayı okuyan yol (Theme ▸ emsali, 007);
yazım `Settings::with_edit` + yeni bir `SettingsEdit` kolu, biçim ve yorum
korunarak, ayrıştırılamayan dosyaya yazmadan. Yazımın kuralı:

- Tam bu host'u (büyük/küçük harf duyarsız eşit desen) yazan bir girdi
  varsa işareti **yerinde** değişiyor — kullanıcının koyduğu sıra bozulmuyor.
- Yoksa girdi dizinin **başına** ekleniyor: kullanıcı "bu makine prod" dedi
  ve o cümle bir globun arkasında kalıp etkisiz görünmemeli.
- **None** tam girdiyi siliyor; silindikten sonra hâlâ bir glob eşleşiyorsa
  başa `mark = "none"` girdisi yazıyor. Seçilen zaten geçerli çözümse no-op.

Ayar penceresine (029) bir liste düzenleyicisi eklenmiyor: menü tek host'u,
dosya deseni yazıyor; liste düzenleyicisi ayrı bir UI işi ve istenmedi.

## Karar 6: ⌘T aynı host'a → ✅ yerel kabuk doğar, ilk girdisi aynı komut; ⌘N yerel, ⌥⌘T "New Local Tab"

Uzak sekmede ⌘T (ve aynı yoldan giden sekme çubuğunun `+`'sı) yeni sekmeyi
etkin sekmenin **yerel** dizininde yerel bir kabukla doğuruyor ve kabuğun ilk
girdisi olarak aynı ssh/mosh komutunu yazıyor. Kullanıcı yeni sekmede
`ssh -p 2222 prod`'un koştuğunu bir komut bloğu olarak görüyor, geçmişte de
duruyor, ve `exit` onu yerel kabuğa döndürüyor — bugünkü bir ssh sekmesiyle
aynı davranış. Uzak durum 036'nın yoklamasıyla kendiliğinden kuruluyor; yeni
bir algılama yolu yok.

**Taşınan argv'nin tamamı**, üç istisnayla: `-L`/`-R`/`-D` (değerleriyle) —
ikinci oturumda aynı yerel portu bağlamaya çalışıp uyarı basar ya da
`ExitOnForwardFailure`'da hiç bağlanmaz; `-M` (ikinci bir ControlMaster);
`-f` (arka plana düşen ssh zaten etkileşimli sayılmıyor). `-J`, `-p`, `-i`,
`-l`, `-F`, `-o` ve `-t`'li uzak komut (`ssh -t prod tmux attach`) aynen —
"aynı yere ikinci bir kapı" beklentisi o. `-o LocalForward=…` biçimleri
ayıklanmıyor (bilinen sınır). mosh: `mosh` + betiğin argümanları; yoklama
yalnız `mosh-client`'ı gördüyse onun `-#` satırından.

**İlk girdinin teslimi `bt-core`'da**: `SessionOptions`'a yazılacak bir satır.
Sarmalayıcının kurulduğu oturumda (`[shell] integration`'ın iki kademesi de;
`blocks` dock'suz ama işaretli) bizim ilk kimlikli `A`'mızda — kabuk
prompt'a vardı, rc dosyalarının okuduğu stdin'i (oh-my-zsh'in güncelleme
sorusu) yemiyor; sarmalayıcısız oturumda (zsh değil, entegrasyon kapalı)
doğumda, kabuğun typeahead'i olarak. Yazım `send_input`'un yolundan.

**Bilinen sınır:** sarmalayıcı kurulu ama kimlikli `A` hiç gelmiyorsa (bozuk
bir `.zshrc`, rc'nin sonunda `exec fish`) satır hiç gitmiyor ve yeni sekme
yerel kabukta bekliyor — komut görünmüyor, bağlanmıyor. Zaman aşımıyla
doğuma düşmek ölçülmemiş bir sayı olurdu; kabuk zaten bozuk bir hâlde ve
kullanıcı satırı elle yazabiliyor.

**Yerel sekme yolu:** Shell ▸ **New Local Tab** (⌥⌘T, `menu.rs`'te boş) her
zaman yerel; yerel sekmede ⌘T ile aynı şey. **⌘N yerel kalıyor**: istek
sekme içindi ve yeni pencere yeni bir çalışma alanı; ikinci bir kaçış yolu
olarak da iş görüyor. 026'nın dizin mirası ikisinde de aynen.

**Reddedilen:**
- *ssh'ı doğrudan PTY çocuğu yapmak* — `exit` sekmeyi kapatır, kabuk
  entegrasyonu (blok, dock, işaret) olmaz ve "bir pencere = bir kabuk"
  modeli (`bt-shell`'in doğum yolu, 028'in süreç ağacı) ikinci bir şekil
  kazanır.
- *İlk komutu ortam değişkeniyle betiğe vermek* — `assets/shell/`'e dokunur,
  pahalı karar sınıfı (üç kabuk); `bt-core`'un teslimi aynı sonucu betiksiz
  veriyor.
- *Yalnız host'u taşımak* — `-p`/`-i`/`-J` ile bağlanan kullanıcının ikinci
  sekmesi bağlanamaz; beklenti "aynı yere".

## Karar 7: Finder damlası ssh'ta → ⏳ ürün kararı, kullanıcıya gidiyor (öneri: yükleme)

Bugün uzak kabuğun satırına **yerel** yol yazılıyor (kusur). İki yol
kullanıcının gördüğünde ayrışıyor:

### Seçenek A: damlayı reddetmek

Uzak oturumda `draggingEntered:` `None` dönüyor (imleç "+" göstermiyor),
`performDragOperation:` `false`. Yerel sekmede bugünkü gibi.

**Maliyet:** küçük — iki metotta bir soru, bir sınama. `draggingEntered:`'ın
doc'undaki "eleme kayıtta, oturum sorulmuyor" gerekçesi değişiyor (artık
soruyor) ve yeniden yazılıyor. **Kullanıcının gördüğü:** dosya bırakılamıyor,
neden bırakılamadığını söyleyen bir şey yok; uzak makineye dosya taşımak için
yerel sekmeye geçip `scp` yazmak gerekiyor.

### Seçenek B: uzak dizine yüklemeyi önermek

Uzak oturumda bırakılan dosya bir sayfa açıyor: "Upload 2 items to
prod:/var/www/app?" — **Upload** / **Cancel**. Onayda `bt-shell` arka planda
`scp` koşturuyor; bitince **uzak yollar** kaçırılıp uzak kabuğun satırına
yapıştırılıyor (bugünkü damlanın anlamı korunuyor: "bu dosyanın buradaki
yolu"). Hata olursa sayfa `scp`'nin son hata satırını gösteriyor.

**Gerçekçi maliyet ve sınırlar:**
- **Parola girişi çalışmıyor.** GUI'den doğan `scp`'nin kontrol terminali yok;
  parola soramaz. `BatchMode=yes` ile anında ve açık bir hatayla düşüyor
  ("needs key-based login"), asılı kalmıyor. Anahtar ve `ssh-agent` ile
  giren kullanıcıda (ssh kullanıcılarının çoğu) çalışıyor.
- **Seçeneklerin çevirisi**: ssh'ın `-p`'si `scp`'de `-P`, `-l` → `-o User=`,
  `-S` → `-o ControlPath=`; `-i`/`-J`/`-F`/`-o`/`-4`/`-6` aynen; kalanı
  düşüyor. Saf bir fonksiyon, sahte argv'yle sınanır. mosh'ta ssh argv'si yok:
  yalnız host'la (varsayılan ssh ayarlarıyla) yükleniyor.
- **Hedef dizin** yalnız uzak kabuk OSC 7 bastıysa biliniyor (036 Karar 4);
  basmıyorsa uzak ev dizini (`host:`) ve yapıştırılan yol `~/ad`. Sayfa hedefi
  yazdığı için kullanıcı nereye gittiğini görüyor.
- **Üzerine yazma sessiz** (`scp`'nin davranışı); sayfa bunu söylemiyor.
- **İlerleme göstergesi yok**: büyük dosyada sayfa kapandıktan sonra yolun
  yapıştırılmasına kadar görünür bir şey olmuyor. Pencere başına tek yükleme;
  sürerken gelen damla reddediliyor.
- Yükleme bitmeden ssh kapanırsa yollar **yapıştırılmıyor** (uzak oturumun
  nesli tutmuyor; yerel kabuğa uzak yol düşmesin).
- Kod: süreç doğurma (`std::process::Command`, yeni bağımlılık yok), bir
  sayfa, bir çeviri fonksiyonu — bir phase.

### Öneri

**B.** `CLAUDE.md` → "Boşlukta kullanıcı tarafı seçilir": A kodu olduğu gibi
bırakıp özelliği kısıyor, B özelliği koruyor; ve damlanın kullanıcı için
anlamı "bu dosyayı buraya getir". A'nın tek üstünlüğü ucuzluk ve parolalı
host'ta B'nin de düşeceği yer açık bir hata, sessiz bir yanlış değil. Bu
bölüm kullanıcının cevabıyla kapanır; seçim tek bir phase'i (phase-5)
değiştirir, geri kalan phase'ler iki seçenekte de aynı.

## Karar 8: bağlantı kopunca → ✅ ssh 255'te dock'un boş giriş satırında teklif, ilk tuşta kalkar; mosh'ta yok

**Tetik:** uzak oturum etkinken ve tür **ssh** iken bizim (kimlikli) `D`'miz
255 taşıyor. Uzak durum `D`'de bugünkü gibi siliniyor; hedef (host, işaret,
satır) ayrı bir yuvaya — **teklif** — geçiyor. Kapı 036'nın "uzak oturumu
yalnız bizim işaretimiz bitirir" kuralının arkasında: uzak kabuğun
kimliksiz `D`'si teklif doğurmuyor.

**Görünüş:** dock'un giriş satırı boşken, satırın yer tutucusu olarak
(öneri gibi, caret'ten sonra): `⇄ prod  Connection lost · ⏎ reconnect` —
`⇄ prod` işaretin renginde, kalanı `dim`. Bağlam satırı bugünkü yerel
hâlinde. Satıra bir şey yazılınca yer tutucu zaten görünmüyor.

**⏎:** giriş satırı boş, dock satırın sahibi ve teklif varken düz ⏎ teklifin
satırını ve `\r`'yi `send_input`'tan gönderiyor — yazılmış gibi, geçmişe
giriyor. Yeni bir `DockKey` kolu; `Session::dock_key` teklif yokken **ilk
satırda** `false` dönüyor, yani bugünkü Enter yolu bayt bayt aynı. Kapı
düzenleme widget'ına (`8133;w`, `can_edit_dock`) **bağlı değil** — giden şey
yazılmış baytlar, widget komutu değil: teklif + dock caret'in sahibi + ayna
taze + `BUFFER` boş.

**Ömür: ilk tuşta** (ilk komutta değil). Herhangi bir girdi (`send_input`:
tuş, yapıştırma, damla) ve bir sonraki `C` teklifi kaldırıyor; `A` ve
`precmd`'in işaretleri kaldırmıyor (teklif tam o prompt'ta doğuyor). Yer
tutucu gibi davranıyor: kullanıcı başka bir şey yazmaya başladıysa niyeti
bağlantı değil, ve silip boşaltmak teklifi geri getirmiyor.

**Metin tek** — "Connection lost" — ve bu bir ölçümün sonucu: 255 kopmayı
başarısız bağlantıdan ayırmıyor (context.md → Kanıt). Teklifin eylemi ikisinde
de doğru ("aynı komutu yeniden dene": VPN'i açıp tekrar denemek gerçek bir
kullanım), ayıran bilgi ssh'ın hemen üstte duran kendi hata satırı. İki ayrı
metin için ayırt edecek bir veri yok; ızgaranın metnini ayrıştırmak kırılgan
ve reddedildi.

**mosh'ta teklif yok:** mosh kopan bağlantıda çıkmıyor, kendi şeridiyle
bekleyip yeniden bağlanıyor; çıkışı kullanıcının ya da sunucunun kararı ve
çıkış kodları ölçülmedi.

**Bilinen sınırlar:**
- **Hızlı başarısızlıkta teklif zamanlamaya bağlı**: ssh yoklama ana kuyruğa
  varmadan çıkarsa uzak durum hiç kurulmamış olur ve teklif doğmaz
  (context.md → Kanıt). Pratikte host çözülemediğinde teklif çoğu zaman yok,
  zaman aşımında ve kimlik reddinde (ssh o sürede ön planda) var. Kapatmak
  yoklamayı `C`'nin önüne almak demek; bu setin işi değil.
- Kullanıcının `~.` ile kasten kopardığı oturum da teklif gösteriyor (255);
  ilk tuşta kalkıyor.
- Uzak kabuk `exit 255` ile çıkarsa teklif yanlış alarm.
- `[shell] integration = "blocks"` pencerede dock yok, teklif de yok.

## Karar (2026-09-26, otonom akış)

Karar 7 dışındakiler teknik karar ya da bariz kullanıcı beklentisi (sürücü
otonom; yol haritası satırındaki kullanıcı kararları korunarak). **Karar 7
ürün kararı ve açık**: öneri B, kullanıcıya sürücü aracılığıyla soruluyor;
cevap A olursa yalnız phase-5 değişir.

- **Seçilen:** yoklama argv'yi taşıyor, hedef `bt-core`'da bütün (Karar 1);
  sıralı `[remote] hosts` dizisi, el yazması `*`/`?` glob, `user@` yalnız
  desende `@` varsa, doğrudan renk kabul, çözüm kenarda (Karar 2); anlam →
  rol, `warning` = temanın ANSI sarısı (Karar 3); sekmede `accessoryView`
  noktası, yalnız işaretli host'ta (Karar 4); Shell ▸ "Mark … as ▸", tam
  girdi yerinde ya da başa (Karar 5); ⌘T yerel kabuk + ilk girdi olarak aynı
  komut, ileri yönlendirmesiz argv, ⌥⌘T New Local Tab, ⌘N yerel (Karar 6);
  ssh 255'te boş giriş satırında "Connection lost · ⏎ reconnect", ilk tuşta
  kalkar, mosh'ta yok (Karar 8).
- **Reddedilen:** TOML tablosu (sırasız) — yol haritasının "sırayla" kararını
  taşıyamaz; glob crate'i — yeni bağımlılık, pahalı sınıf, `*`/`?` yetiyor;
  kare yolunda eşleşme — iki kenarda değişen cevap için kare başına CPU;
  `setAttributedTitle` — ikinci başlık yazarı; her uzak sekmeye nokta —
  işaretin anlamını sulandırır; ssh'ı doğrudan PTY çocuğu yapmak, ilk komutu
  betiğe ortamla vermek, yalnız host'u taşımak (Karar 6); iki ayrı kopma
  metni ve ızgaradan hata satırı ayrıştırmak — ölçülen kod ayırmıyor, metin
  kırılgan (Karar 8).

Panel koşmadı: pahalı karar sınıfına dokunan yol seçilmedi — yeni crate yok
(glob el yazması, `scp` `std::process`), betik değişmiyor (ilk girdi
`bt-core`'dan), `Cell` değişmiyor, kare yolunda yeni hesap yok (desen kenarda
çözülüyor), `bt-core` platformsuz (dizgi alıyor). Pahalı sınıfa giren iki
seçenek (glob crate'i, betikten ilk komut) yukarıda reddedildi. `NSWindowTab`
bayrağı mevcut bir bağımlılığın başlık bayrağı, `Cargo.lock` oynamıyor.
