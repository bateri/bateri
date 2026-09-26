# ssh'ta uzak oturum hissi — Tartışma

Birbirinden bağımsız karar noktaları; her biri çözüldü. Kullanıcıya giden iki
görsel karar (renk, işaret) kullanıcının onayıyla, geri kalanı teknik karar
olarak `## Karar`'da kayıtlı.

## Karar 1: ssh'ı kim, nereden algılıyor → ✅ `bt-shell` süreç tablosundan, `bt-core` setter'la öğreniyor

İki yol vardı:

- **Kabuk betiği** `preexec`'in `$1`'inden (yazılan komut satırı) hedefi
  okur ve OSC ile bildirir. Takma adı, fonksiyonu ve sarmalayıcı betiği
  göremez (`alias s=ssh` metin olarak `s prod`), ve betiğe dokunmak pahalı
  karar sınıfında (`proje.md` → üç kabuk birden).
- **Süreç tablosu:** 028'in `jobs.rs`'i ön plan grubunu zaten buluyor; eksik
  olan argümanlar (`KERN_PROCARGS2`, `libc`'nin Apple yarısında var — yeni
  bağımlılık yok). Takma ad ve fonksiyon sorun değil, çünkü bakılan şey
  gerçekten `exec` edilmiş süreç.

Seçilen ikincisi. Katman sınırı değişmiyor: süreç tablosu `bt-shell`'de
(platform), `bt-core` yalnız sonucu bir setter'la alıyor
(`Session::set_remote(command, host)`), `set_theme` emsali. `bt-core` ne pid
ne `libc` görüyor.

## Karar 2: yoklamanın zamanı → ✅ `C` kenarında, karar verilemezse sonraki çıktı kenarında; ilk kesin cevap `D`'ye kadar kilitli

Kare başına yoklama yok. Tetik `C`'nin **kenarı** (safhanın `Running`'e
geçişi; aynı komutta ikinci bir `C` — iTerm2 entegrasyonu — kenar değil).
Yeni bir `Wake` çağrısı (`command_started`) okuyucu thread'den haber veriyor,
`bt-shell` ana kuyruğa tek bir yoklama işi atıyor (`title_changed`'in
örüntüsü: kenarda, yüksüz, en çok bir bekleyen iş).

Yarış (context.md → Kanıt): `C` fork'tan önce basılıyor. Yoklamanın üç cevabı
var:

- **Kararsız** — kabuğun kendi grubu hâlâ ön planda, ya da ön plan grubunun
  bütün üyeleri kabuğun kendi adını taşıyor (çatallanmış, henüz `exec`
  etmemiş çocuk). Yoklama **silahlı kalır** ve PTY'den gelen bir sonraki
  çıktının `wake`'inde tekrar koşar (bekleyen iş en çok bir). ssh bağlanınca
  her hâlde bir şey basıyor, yani uzak bir kabuk için tekrar garanti.
- **Yerel** — başka bir program ön planda ve ssh/mosh değil. Silah iner.
- **Uzak** — `set_remote`. Silah iner.

Kesin cevap `D`'ye (ya da `A`'ya) kadar geçerli; sonraki çıktılar yoklamaz.
Böylece akan bir `cat`'in maliyeti tek yoklama, kararsız kalabilen tek hâl
(kabuk içi bir döngünün çıktısı) ana kuyruk turu başına en çok bir yoklama.

**Bayat cevap kapısı:** yoklama ana thread'de, `D` okuyucu thread'de; arada
`D` gelirse uzak durum bir sonraki prompt'a sızardı. `ShellLog` `Running`'e
her **geçişte** bir komut nesli artırıyor; `bt-shell` yoklamadan önce
`Session::running_command() -> Option<u64>` ile nesli alıyor ve
`set_remote`'a geri veriyor; nesil değişmişse ya da safha `Running` değilse
çağrı no-op. Uzak durum `bt-core`'da `C`, `D` ve `A`'da **kendiliğinden**
siliniyor — bitiş için gidiş-dönüş yok.

**Bilinen sınırlar:** (a) ssh'ı sonradan başlatan bir sarmalayıcı betik
(`./deploy.sh` içinde `ssh prod`) ilk yoklamada "yerel" kilitleniyor;
(b) `exec ssh prod` `C`/`D` üretmiyor (028'in `exec vim` sınırının aynısı);
(c) ssh'ın `~^Z` ile askıya alınması kabuğa prompt bastırıyor (`D`), `fg`
yeni bir `C` ve yoklama — gösterge doğru olarak kalkıp geri geliyor.

## Karar 3: hangi süreç, hangi host → ✅ grubun en üstteki tanınan süreci; host yazıldığı gibi; yalnız etkileşimli oturum

`jobs::names` bilerek **yaprakları** seçiyor (sarmalayıcı lider → program).
ssh için ters yön gerekiyor: `ssh -J jump prod` aynı grupta
`ssh -W prod:22 jump` diye bir çocuk doğuruyor ve yaprak jump host'u verirdi.
Seçim: ön plan grubunda, **atası grupta tanınan bir süreç olmayan** ilk tanınan
süreç.

Tanınan süreçler ve hedefleri (hepsi saf bir ayrıştırıcıda, sahte tabloyla
sınanır):

- **`ssh`** — ilk seçenek-olmayan argüman hedef (`[user@]host` ya da
  `ssh://[user@]host[:port]`); argüman alan seçenekler (`-B -b -c -D -E -e -F
  -I -i -J -L -l -m -O -o -P -p -Q -R -S -W -w`) değerlerini yutuyor.
  **Etkileşimli değilse uzak sayılmıyor:** `-N -W -O -Q -G -V -T`'den biri
  varsa, ya da hedeften sonra bir komut varsa ve `-t` yoksa. `ssh prod uptime`
  bir saniyelik bir komut ve dock'u kısıp geri açmak tam da kullanıcının uzun
  yerel komutlarda reddettiği sıçrama olurdu; `ssh -t prod tmux attach`
  etkileşimli.
- **`mosh`** — Perl betiği, süreç adı `perl`; argv'de betiğin yolu
  (`…/mosh`). Tanıma betiğin basename'inden, hedef ilk seçenek-olmayan
  argüman. Bootstrap `ssh … mosh-server new`'i onun **çocuğu**, yani "en
  üstteki tanınan" kuralı mosh'u ssh'tan önce buluyor ve bootstrap'in uzak
  komutu kararı "yerel"e çekmiyor. mosh betiği sonra `mosh-client`'a `exec`
  ediyor (aynı pid); kilitli cevap geçerli kalıyor. `mosh-client`'ın kendisi
  de tanınıyor (`-#` argümanının ilk sözcüğü) ki kilitlenmeden önce `exec`
  olmuşsa da bulunsun.

Host **kullanıcının yazdığı gibi** gösteriliyor (`prod`, `deploy@10.0.0.5`);
`~/.ssh/config` çözülmüyor, `ssh://` biçiminde şema ve port atılıyor.

## Karar 4: uzak dizin → ✅ uzak oturum sürerken her OSC 7 uzak yuvaya; yabancı yetkili OSC 7 her zaman uzak yuvaya

Tarayıcı reddetmek yerine olayı yetkisiyle birlikte veriyor
(`ScanEvent::Cwd` + yerel mi). `ShellLog`:

- **Uzak oturum etkinken** gelen **her** OSC 7 uzak yuvaya gidiyor — yetkisi
  boş olsa da. ssh ön plandayken yerel kabuk bloklu; o an basılan OSC 7 uzak
  tarafın. Aksi hâlde `file:///path` basan bir uzak kabuk yerel dizini
  ezerdi.
- **Etkin değilken** yerel yetki bugünkü gibi `context.cwd`'ye, yabancı yetki
  uzak yuvaya — böylece OSC 7'nin yoklamadan önce gelmesi sonucu değiştirmiyor.
- Uzak yuva `C`, `D` ve `A`'da siliniyor; yalnız uzak oturum etkinken
  okunuyor.

**Bilinen sınır:** yoklama sonuçlanmadan önce gelen **boş yetkili** bir uzak
OSC 7 yerel dizini ezer. Pencere yoklamanın `C`'den sonraki ilk turu; bir ssh
bağlantısının ilk baytından çok kısa.

Uzak kabuk OSC 7 basmıyorsa bağlam satırı yalnız host'u gösteriyor. Pencere
başlığı ayrıştırılmıyor (kullanıcı kararı).

## Karar 5: başlık ve sekme → ✅ `⇄ {OSC başlığı}`, yoksa `⇄ {host}`

`shell::title_of` saf ve bir argüman kazanıyor. Uzak oturum etkinken:
uygulamanın OSC 0/2 başlığı varsa `⇄ ` önekiyle, yoksa `⇄ {host}`. Önek
koşulsuz, çünkü uzak kabukların çoğu başlığa `user@host: dir` basıyor ve o
başlıkta da kullanıcı sekmeler arasında uzağı ayırt etmeli; alternatif ekranda
(uzakta vim) dock kalktığı için göstergeyi yalnız başlık taşıyor.

Kullanıcının örneği `prod-web-1 — bateri` idi; bugünkü kural başlığa hiç
`— bateri` eklemiyor (yerelde başlık `proj`, `proj — bateri` değil — 026 Karar
7), yani eşdeğeri dizin adının yerine host. Sekme başlığı pencere başlığının
ta kendisi (native sekme), `⇄` ikisinde de. Sekmede renk yok: AppKit'in
sekme başlığı düz metin ve renklendirmek için `NSWindowTab`'ın ayrı başlığını
kurmak ikinci bir başlık yazarı demek.

Haber iki kenarda: `set_remote` ana thread'de ve `bt-shell` başlığı
hemen tazeliyor; `D`/`A`'daki temizlik okuyucu thread'de ve
`apply_scan_answering`'in "başlığın girdisi değişti" dönüşü onu da
kapsıyor.

**Bilinen sınır (bugünden):** uzak kabuğun bastığı OSC 0/2 başlığı ssh
bitince yerinde kalıyor, yerel kabuk başlık basmıyorsa. `D`'de temizlemek
çare değil: kullanıcının kendi `precmd` kancaları (oh-my-zsh'in
`termsupport`'u) bizimkinden **önce** koşuyor ve taze yazdıkları yerel
başlığı silerdik.

## Karar 6: renk → ✅ yeni `info` tema rolü (kullanıcı onaylı)

Temanın dokuz rolünden henüz çizilmeyen **bilgi** rolü bu setle çiziliyor:
`bateri`'de `#79b3b3`, `bateri-light`'ta `#23787f` — ikisi de kendi temasının
ANSI camgöbeği. Açık temanın değeri zeminde `color::tests`'in 3:1 ölçütünü
geçiyor; bir sınama ikisini de bağlıyor. Tema dosyasında opsiyonel `info`
anahtarı, eksikse gömülü tabandan (kuralın istisnası yok). `accent`
kullanılmıyor: koşan komutun şeridi zaten o ve ssh da koşan bir komut — aynı
renk iki anlam taşırdı ("bir şey koşuyor" / "uzaktasın").

Rengi alanlar: bağlam satırında `⇄` ve host, dock'un **üst** saç çizgisi.
İkinci saç çizgisi zaten yok (tek satırlık bant). Yol bugünkü iki kademede
(son bileşen `dim`, üst dizinler `quiet`).

## Karar 7: işaret → ✅ `⇄` (U+21C4) fonttan, sıradan karakter; kutuysa `↔` (kullanıcı onaylı)

İşaret yordamsal değil, bağlam satırının sıradan bir hücresi. Aynı karakter
başlıkta ve sekmede. Bağlam satırı küçük boy sınıfında ve orada yordamsal kapı
kapalı (021), glyph yedeği ise açık (019: boy sınıfının kendi fontu). İlgili
phase glyph'in Menlo'da (adıyla; makineden bağımsız kapı) küçük sınıfta
**kutu çıkmadığını** bir atlas sınamasıyla doğruluyor, SF Mono kuruluysa
koşullu sorgu ve gözle kontrol; kutuysa `↔` (U+2194) yedek, ikisi de kutuysa iş durur
ve raporlanır.

Seçimin bedeli adıyla: işaret kullanıcının fontuyla değişiyor (prompt
chevron'unun aksine, 012 phase-9) ve cascade'den geniş gelen bir aday
kutuya dönüşür. Ölçülen: Menlo'da hücrenin içinde; SF Mono ölçülmedi
(context.md → Kanıt).

## Karar 8: ssh boyunca giriş satırı → ✅ sıfır giriş satırı, yalnız uzak oturumda

Kullanıcı kararı (sohbet): uzakta dock tek satırlık bir durum çubuğuna iniyor
— renkli üst çizgi + bağlam satırı; uzun yerel komutlarda **yapılmıyor**
(süre eşiği okurken ekranı oynatır, bitişte ters sıçrama). Mekanizma:

- PTY payı (`DOCK_ROWS`) değişmiyor, kabuk SIGWINCH görmüyor.
- `Session::frame` uzak oturumda `Cursor::input_rows = 0` veriyor (dock'lu
  pencere, alternatif ekran değil); `.max(1)` bekçileri sıfırı taşıyacak
  biçimde açılıyor.
- Bandın fazlası tek formülden ve **kesirli**:
  `(band_px(input_rows) − dock_px(DOCK_ROWS)) / cell_h`; sıfırda negatif ve
  bir hücre artı satır arası boşluk. Izgara o kadar aşağı çiziliyor, tepede
  açılan şeridi doldurma bandı kapatıyor (`set_grid_top` zaten pozitif
  tepeyi uzatıyor). Kayma `Motion`'ın bant `Slide`'ında, iki yönde; ssh
  bitince satır süzülerek geri geliyor.
- Uzak oturum `caret_in_dock`'un dördüncü ön koşulu ve Dock→Grid
  tutmasından (`HANDOVER_HOLD`) önce uygulanıyor: `set_remote` tutmanın
  içine düşerse caret giriş satırı olmayan bir banda, bağlam satırına
  otururdu.
- Dock'a tık giriş satırı yokken hiçbir şey yapmıyor (caret taşıma, seçim
  yok); bağlam satırı zaten seçilemiyor.
- Alternatif ekranda dock bugünkü gibi tümden kalkıyor.

## Karar (2026-09-26, otonom akış)

Karar 6 ve 7 kullanıcı onaylı (sürücü aracılığıyla, "önerini yap"); Karar
8'in yönü ve kapsam kullanıcı kararı (sohbet); geri kalanı teknik karar.

- **Seçilen:** süreç tablosundan algılama + `bt-core`'a setter (Karar 1);
  `C` kenarında yoklama, kararsızsa çıktı kenarında tekrar, nesil kapısı
  (Karar 2); grubun en üstteki tanınan süreci, ssh ve mosh, yalnız
  etkileşimli (Karar 3); uzak yuvalı OSC 7 (Karar 4); `⇄` önekli başlık
  (Karar 5); `info` rolü (Karar 6); `⇄` fonttan, `↔` yedek (Karar 7); uzakta
  sıfır giriş satırı (Karar 8).
- **Reddedilen:** betikten algılama — takma adı görmez, pahalı sınıfa
  dokunur; kare başına ya da periyodik yoklama — boşta sıfır kare ruhuna ve
  "yalnız kenarda" disiplinine aykırı, gereği yok; yaprak süreçten host —
  `ssh -J`'de jump host'u verir; `accent` rengi — koşan komutla anlam
  çakışması; yordamsal işaret — kullanıcı fonttan istedi; başlığı `D`'de
  temizlemek — kullanıcının taze yerel başlığını siler; uzun yerel komutta da
  giriş satırını kısmak — kullanıcı reddetti.

Panel koşmadı: seçilen yol pahalı karar sınıfına dokunmuyor — yeni bağımlılık
yok (`KERN_PROCARGS2` `libc`'de), betik değişmiyor, `Cell` değişmiyor,
`bt-core` platformsuz kalıyor (setter), kare yolunda yeni hesap bir alan
okuması. Reddedilen tek pahalı yol (betik) zaten elendi.
