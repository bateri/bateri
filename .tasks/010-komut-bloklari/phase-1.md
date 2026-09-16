# Phase 1 — Çıpa basılır, kimlik okunur, defter doğar

## Özet

Kabuk her prompt'a artan bir blok kimliği basar ve onu ızgaraya bir OSC 8
bağlantısıyla iliştirir; tarayıcı kimliği işaretten çeker ve `bt-core`
kimliği çıkış koduna bağlayan defteri tutar.

_Requirements: R1, R1.1, R1.2, R1.3, R2, R2.1, R2.2, R2.3_

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — `__bateri_hooks` bir blok sayacı
  kurar; `__bateri_precmd` sayacı artırıp `psvar`'a yazar ve `A` ile `D`'yi
  `bt_block={N}` alanıyla basar. PS1 eki **sabit** kalır: önek
  `%{\e]8;;bateri://block/%9v\a%}`, sonek mevcut `B` ekinin yanında
  `%{\e]8;;\a%}`. Sabitlik zorunlu — 161. satırdaki "içeriyorsa dokunma"
  nöbeti ancak sabit bir dizgeyle çalışıyor ve kimliği URI'ye gömmek onu
  çökertirdi; `%9v` prompt genişletmesi `prompt_subst` istemiyor.
  Önek PS1'in **başına** girer: temanın kendi OSC 8'i iç içe geçmiyor
  (`vte` yeni URI ile öncekini değiştiriyor), yani önce basılan kazanır.
  Sayaç `__bateri_restore`'un `unset` listesinden sağ çıkar — o fonksiyon
  yükleyiciyi siliyor, kancaların oturum durumunu değil.
  Hiçbir kol ölümcül değil (009 sözleşmesi); kullanıcının `psvar`'ını
  ezmemek için yüksek bir indeks seçilir.
- **`crates/bt-core/src/shell.rs`** — `Mark`'ın dört varyantı kimlik alanı
  kazanır (`PromptStart` ve `CommandEnd` taşır, kalanı taşımaz);
  `parse_mark` `bt_block=` alanını okur — `aid=` **değil**, gerekçesi
  Uygulama Notları'nda. Çerçeveleme değişmiyor: yabancı alanlar bugün de
  tolere edilip düşürülüyor. Modülün "bu modül **saf**" başlığı korunur:
  saat okunmaz, kilit görülmez, kare istenmez.
- **`crates/bt-core/src/session.rs`** — defter: `blok kimliği → çıkış kodu`,
  yaprak kilitte, `ShellState` yuvasının emsaliyle. Halkanın tavanı
  `scrollback`'ten türüyor; gerekçesi satır başına en çok bir prompt düşmesi.
  Tahliye üstüne yazmayla:
  alacritty çıpanın düştüğünü **yayınlamıyor** (`context.md` → Kanıt 2), bu
  yüzden sinyal beklenmiyor. Yazan yalnız okuyucu thread'i
  (`TappedPty::read`), okuyan phase-2'de `frame()`.

## Kabul

- `Scanner` `A;bt_block=7` ve `D;130;bt_block=7` yüklerinden kimliği çıkarır;
  kimliksiz yük ve okunamayan kimlik işareti **düşürmez** (mevcut
  "kod okunamazsa komut yine bitti" kuralının aynısı).
- Defter halkası tavanı aşınca en eski kaydı düşürür ve yeni kayıt okunur.
- Aynı kimliğin ikinci kez açılması o kaydı **ezer** ve yalnız ardındakileri
  düşürür; çakışma diye bir durum doğmaz.
- Şartnamenin `aid=` alanı **yoksayılır**: onu kimlik sanmak yabancı bir
  entegrasyonun defteri silmesine ya da bloğu yanlış renklendirmesine yol
  açardı.
- `make kur` betiği pakete kopyalar ve `cmp` ile denetler; envanter beş
  dosya olarak kalır.
- **Elle doğrulama** (`make hepsi` göremez, kullanıcı koşturur): gerçek bir
  zsh oturumunda prompt'un hücreleri `bateri://block/{N}` taşır, `N` her
  prompt'ta artar ve prompt hizası bozulmaz (`%{…%}` sıfır genişlik).
  Koşulamazsa `[~]` ve gerekçe — waive değil, atlanmış doğrulama.

## Uygulama Notları

- **Durum ve defter tek yaprak kilidin altında birleşti** (`ShellLog`).
  Phase iki ayrı kayıt öngörüyordu; ikisini de aynı işaret akışı besliyor ve
  aynı kare okuyacak, yani ayrı kilitler aynı kareyi bir işaretin iki yarısı
  arasında yakalayabilirdi. Halka (`BlockLog`) `shell.rs`'te **saf** bir tip
  olarak duruyor, `Arc<Mutex<…>>` yuvası `session.rs`'te — modülün "ne
  `Session`, ne kilit" başlığı korunuyor.
- **`BlockLog::get` bu phase'de `#[cfg(test)]`.** Üretim tüketicisi kare yolu
  ve o phase-2'de; `allow(dead_code)` ile açık bırakmak aynı susturmayı
  sonraki gerçek ölü koda da miras bırakırdı.
- **`typeset -gr` reddedildi.** Çıpa dizgilerini salt okunur global yapmak
  ilk denemeydi; `bateri.zsh` dört başlangıç dosyasının **her birinden**
  `source` ediliyor ve ikinci geçişte "read-only variable" hatası basardı —
  betiğin "hiçbir kolda ölümcül değil" kuralına aykırı. Dizgiler `precmd`'de
  yerel.
- **`aid=` KULLANILMIYOR, alan adı bize özel: `bt_block=`** (`/code-review`,
  bu phase). İlk uygulama kimliği `aid=`'ye yazıyordu; `aid` semantic-prompts
  şartnamesinde tanımlı ve "uygulama kimliği" demek, genellikle **pid**
  taşıyor — yani oturum boyunca *sabit*, bizim sayacımız gibi artan değil.
  Bedeli somuttu: şartnameye uyan herhangi bir entegrasyon (kullanıcının
  kendi rc'si, iç içe bir REPL, SSH'ın öte yakası) her prompt'ta aynı değeri
  basar, defter onu bitişiksiz görüp **kendini silerdi**; aralığa denk düşen
  bir `D;kod;aid=pid` ise bizim bloğumuzun rengini başkasının koduyla ezerdi
  — tam da A′'nın reddedilme sebebi olan "yanlış renk". Yabancı `aid` eskisi
  gibi yoksayılıyor ve bunu bir regresyon bekçisi tutuyor
  (`a_foreign_aid_is_ignored`).
- **Halkanın tavanı sabit değil, `scrollback`'ten türüyor** (`/code-review`,
  bu phase). İlk uygulama 4096'lık bir sabit koyuyordu ve gerekçesi
  yanlıştı: varsayılan `scrollback` 10 000, yani hâlâ geçmişte duran binlerce
  blok rengini kaybederdi. Tavan artık plandaki cümlenin kendisi ("blok
  başına en az bir satır") ve taban `BLOCK_LOG_FLOOR` yalnız `scrollback = 0`
  için.
- **Kimliğin yeniden açılması defteri silmiyor**, yalnız o kimlikten
  sonrasını düşürüyor (`/code-review`, bu phase). Phase'in kabul ölçütü
  "eski kaydı ezer" diyordu, ilk uygulama halkanın tamamını temizliyordu.
  Aynı bulgu bir belge hatasını da ortaya çıkardı: koda yazdığım "`exec zsh`
  sayacı sıfırlar" gerekçesi **ulaşılamaz** — `.zshrc` ilk prompt'tan önce
  `__bateri_restore` çağırıyor, yani yeniden doğan kabuk kullanıcının
  `ZDOTDIR`'ını miras alıyor ve sarmalayıcıyı hiç yüklemiyor. Kol savunma
  kolu olarak duruyor, gerekçesi düzeltildi.
- **Elle doğrulama pencere yerine PTY'de yapıldı.** Sarmalayıcı gerçek bir
  `zsh -l -i` oturumunda, gerçek `ZDOTDIR` takasıyla koşturuldu ve baytlar
  okundu: üç prompt → `A;bt_block=1..3`, `true` → `D;0;bt_block=1`,
  `false` → `D;1;bt_block=2`, üç `bateri://block/N` çıpası ve üç kapanış. Pencereden gözle
  bakmaktan daha güçlü ve tekrarlanabilir; **kapsamadığı tek şey** prompt'un
  görsel hizası, onun dayanağı üç ekin de `%{…%}` içinde olması (009'dan
  beri sevk edilen `B` ekiyle aynı mekanizma).

## Yayın Etkisi

- **shell entegrasyonu** — zsh betiği değişiyor. bash (`--rcfile`) ve fish
  (`vendor_conf.d`) bu sette **yok**: 009'un bıraktığı yerde duruyorlar ve
  çıpa satırı onlar doğduğunda yazılacak. Kullanıcının rc dosyasına
  yazılmıyor (`make denetim` kapısı). Eski betikle açılmış oturumlarda çıpa
  yok, yani şerit yok — hata değil, geri düşüşün kendisi.
- **`make kur` zorunlu** (`assets/shell/*` değişti): kopya ve içerik
  denetimi bu phase'in kapısının parçası.
- **app bundle** — betik `Contents/Resources/shell` altına aynen gidiyor;
  `Info.plist`, entitlement ve imza tarafında değişiklik yok.
- **`CLAUDE.md`** — "komut durumu … ürün yüzeyi (blok, dock) henüz yok"
  cümlesi bu phase'de **henüz** doğru; phase-4'te düzelir.
- Ayar şeması, tema biçimi, terminfo/`TERM`, shader: değişiklik yok.
- Yeni bağımlılık yok.

## Checklist

- [x] Betik: blok sayacı, `psvar`, sabit PS1 önek/soneki, `A`/`D`'de
      `bt_block=`
- [x] `Mark` kimlik taşır, `parse_mark` `bt_block=` okur, `aid=` yoksayılır
- [x] Defter: yaprak kilitte sabit halka, okuyucu thread yazar
- [x] Test: kimlikli/kimliksiz/bozuk yükler ve yabancı `aid`; chunk
      sınırında hayatta
      kalma (mevcut `sequence_split_at_every_byte_survives` örüntüsü)
- [x] Test: halka tavanı aşınca tahliye; aynı kimliğin yeniden gelmesi ezer
- [x] Elle (PTY): çıpa, kimlik akışı ve çıkış kodu eşleşmesi —
      görsel hiza kullanıcının ilk koşusunda görülecek (Uygulama Notları)
- [x] Doğrulama geçti (`make hepsi` + `make kur` + `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu, üç bulgu da giderildi
- [x] Yayın etkisi yazıldı
