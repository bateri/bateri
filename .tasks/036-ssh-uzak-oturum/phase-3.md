# Phase 3 — ssh/mosh algılama ve sözleşme

## Özet

`bt-shell` `C` kenarında ön plan grubunu yoklayıp ssh/mosh hedefini
`Session::set_remote`'a veriyor; özellik uçtan uca açılıyor. Sözleşme ve yol
haritası güncelleniyor.

_Requirements: R6.1, R6.2, R6.3, R7_

## Değişiklikler

- **`crates/bt-shell/src/jobs.rs`**
  - `ProcessTable`'a `args(pid) -> Option<Vec<String>>`; `Libproc` gövdesi
    `sysctl(KERN_PROCARGS2)`: tampon `kern.argmax` boyunda (boş tamponlu
    sorgu gerçek boyu değil tavanı veriyor), düzen argc (4 bayt) + exec yolu +
    NUL dolgusu + argv. Yalnız aday adlı üyeler için okunuyor (`ssh`, `perl`
    ve benzeri yorumlayıcılar, `mosh-client`). Okunamayan argüman "tanınmadı".
  - Saf karar `remote(parent, child, table) -> Probe` (`Undecided` /
    `Local` / `Remote(String)`), `foreground`'ın yanında ve aynı başarısızlık
    dilinde: kabuğun grubu ön plandaysa ya da grubun bütün üyeleri kabuğun
    adını taşıyorsa `Undecided` (Karar 2); aksi hâlde **atası grupta tanınan
    bir süreç olmayan** ilk tanınan süreç (Karar 3), yoksa `Local`.
    **Okunamayan tablo `Local`**, `Undecided` değil: `foreground`'ın "adsız
    koşuyor" kolunun burada karşılığı yok ve kararsız sayılsaydı sistematik
    bir okuma hatası komut boyunca her `wake`'te yoklama doğururdu. 028'in
    tersine burada güvenli yön göstergenin **olmaması**.
  - Saf ayrıştırıcılar: ssh argv'si → hedef ya da "etkileşimli değil"
    (Karar 3'ün seçenek listesi, `-t`/`-T`, komut), mosh betiğinin argv'si,
    `mosh-client`'ın `-#`'i. Host yazıldığı gibi; `ssh://` şeması ve port
    atılır.
  - Modül doc'u yoklamanın ikinci tüketicisini ve bilinen sınırları
    (Karar 2) sayıyor.
- **`crates/bt-shell/src/window.rs`**
  - `ShellWake::command_started`: yoklamayı **silahlar** ve ana kuyruğa en
    çok bir yoklama işi atar (`title_pending` örüntüsü). `ShellWake::wake`
    silah kuruluysa aynı işi atar — kilit altında çağrıldığı için yalnız bir
    atomik okuma ve bir `dispatch`; `Waker`'a giden bugünkü yolu değiştirmez.
  - Ana kuyruktaki iş (`TerminalWindow`'da, `foreground()`'ın yanında):
    `session.running_command()` → yoksa silahı indir; `jobs::remote(...)` →
    `Undecided` silahlı bırakır, `Local` indirir, `Remote(host)` →
    `set_remote(nesil, host)`, değiştiyse `refresh_title`, silahı indirir.
    Okuyucu bitmişse yoklama yok (`foreground()`'ın kuralı).
- **`crates/bt-shell/src/child.rs`** — `SilentWake::command_started` boş
  kalıyor (süreli koşu algılamaz).
- **`CLAUDE.md`** — sözleşmeye kural + tek cümle gerekçe + işaretçi:
  `bt-core`'un tarayıcı kolu (OSC 7 yetkisiyle, uzak yuva), başlığın `⇄`
  kolu, bağlam satırının uzak biçimi ve üst çizgi, sıfır giriş satırı,
  `Wake::command_started`, `jobs`'un ikinci tüketicisi (katman tablosunun
  `bt-shell` satırı: `libc`'nin `sysctl`'ı), tema rolleri ("Bugün yedisi
  tüketiliyor" → `info` dahil sekiz; kalan tek durum rolü uyarı).
- **`docs/YOL-HARITASI.md`** — 036 satırı ve kapsam dışı iki satır set
  açılırken yazıldı; yalnız uygulamada sapma olduysa 036 satırı tazelenir.

## Kabul

- `jobs` saf sınamaları (sahte tablo): kabuk ön planda → `Undecided`;
  çatallanmış `zsh` çocuğu → `Undecided`; `ssh prod` → `Remote("prod")`;
  `ssh -J jump prod` (çocuk `ssh -W … jump`) → `prod`; `ssh prod uptime` →
  `Local`; `ssh -t prod tmux` → `Remote`; `ssh -N -L …` → `Local`;
  `ssh://deploy@h:2222` → `deploy@h`; `perl …/mosh prod` + bootstrap ssh
  çocuğu → `Remote("prod")`; `mosh-client -# 'prod' …` → `prod`;
  `cat` → `Local`; okunamayan grup → `Local`.
- `KERN_PROCARGS2` gövdesi gerçek bir çocukla (bilinen argv'li `sleep`)
  argv'yi aynen döndürüyor — mevcut gerçek PTY sınamasının kalıbı.
- Yoklama silahı: kararsız cevap sonraki `wake`'te tekrar; kesin cevaptan
  sonra `wake` yoklama atmıyor (sayaçlı sahte kuyrukla ya da saf durum
  makinesiyle).
- `make duman` yeşil (süreli koşu algılamıyor, jetonlar bugünkü).

## Checklist

- [x] `ProcessTable::args` ve `KERN_PROCARGS2` gövdesi
- [x] `jobs::remote` ve ssh/mosh ayrıştırıcıları
- [x] `ShellWake::command_started`/`wake` silahı ve ana kuyruk işi
- [x] `set_remote` → `refresh_title`
- [x] `CLAUDE.md` (yol haritası satırı gerekiyorsa tazelenir)
- [x] (phase-2'den devralınan, `/code-review` bulgusu) `CLAUDE.md`'nin
  phase-2'yle çelişen cümleleri: devrin "üç ön koşul"u → dört (uzak oturum,
  tutmadan önce, `ShellLog::caret`); "çizilen bant `Cursor::input_rows` giriş
  satırı + bağlam satırı" ve "fark ızgaranın yukarı ötelenmesiyle kapanıyor"
  → uzakta `input_rows == 0`, bant PTY payından kısa, ızgara aşağı ve şeridi
  doldurma bandı kaydırılmış pencerede de kapatıyor (`grid_lowered`)
- [x] (phase-1'den devralınan, `/code-review` bulgusu) **Uzak kabuğun kendi
  OSC 133 işaretleri** (fish 4, iTerm2/WezTerm/kitty entegrasyonu) ssh'ın
  içinden aynı PTY'ye geliyor: uzak `A` safhayı `Prompt`'a çekip uzak durumu
  siliyor, uzak `C` yeni bir `Running` kenarı (nesil +1, yeni yoklama). Karar:
  bilinen sınır mı (belgele), yoksa `A`/`D`'de silmeyi bizim kimliğimize
  (`bt_block`) bağlamak mı — saatin "yalnız BİZİM `D`'miz" emsali; gözle
  kontrolde 133 basan bir uzak kabukla sına.
  **Orkestratör kararı (2026-09-26): bağla, belgeleme.** Uzak durumu silen
  ve nesli ilerleten kenarlar yalnız bizim kimliğimizi taşıyan işaretler
  olacak. Uzakta fish 4 ya da kitty/iTerm2 entegrasyonu yaygın ve bilinen
  sınır olarak bıraksaydık gösterge o kullanıcılarda ilk uzak prompt'ta
  kaybolurdu (CLAUDE.md → "Boşlukta kullanıcı tarafı seçilir"). Bir
  sınamayla bağla: ssh sürerken yabancı `A`/`C`/`D` uzak durumu silmiyor.
- [x] Test: yukarıdaki Kabul maddeleri
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Gözle kontrol (devir mesajının cümlesi): `ssh <gerçek bir host>` —
  **dock**: giriş satırı süzülerek kalkıyor, tek satırda `⇄ host` camgöbeği,
  uzak kabuk OSC 7 basıyorsa yanında yol, üst çizgi camgöbeği; **ızgara**
  aşağı iniyor, **doldurma bandı** tepedeki şeridi geçmişle dolduruyor (uzakta
  `clear`'dan sonra şerit boş — beklenen); **başlık/sekme** `⇄ host` ya da
  `⇄ {uzak başlık}`; uzakta `vim` açınca dock kalkıyor, `⇄` başlıkta kalıyor;
  `exit` → bant süzülerek geri, yerel yol ve dal geri, başlıktan `⇄` gidiyor.
  `ssh host uptime` dock'u oynatmıyor. Varsa `mosh host` da aynı. SF Mono
  kurulu bir makinede bağlam satırındaki `⇄` kutu değil.

## Uygulama Notları

- **Yabancı 133 kapısı uzak oturuma ve "kimliğimizi gördük"e bağlı**
  (`ShellLog::apply`, yapışkan `ours`): bizim `C`'miz kimliksiz, yani
  "kimlikli işaret" kuralı ancak uzak oturumu **bitirmeye** uygulanabiliyor —
  uzak oturum etkinken kimliksiz `A`/`B`/`C`/`D` yok sayılıyor. İlk sürüm
  kapıyı `Running`'e bağlamıştı; `/code-review` `exec fish`/`exec zsh`'in
  (kimliğimizi bir daha basmayan kabuk) `Running`'i sonsuza kadar tuttuğunu
  buldu (saat, boşta kare). Yoklamadan önce aynı okumada gelen uzak `A`'nın
  yarışını ayrı bir bit kapatıyor: `command_open` (bizim `C`'mizle açılır,
  bizim kimlikli `D`/`A`'mızla kapanır) `running_command()`'ı safha
  `Prompt`'a dönmüş olsa da `Some` tutuyor. `ours` olmadan kapı
  entegrasyonsuz kabuğun kendi 133'ünde uzak durumu hiç silmezdi.
  `end_and_prompt_clear_the_remote_state`'in işaretleri kimlik taşıyor.
  **Bilinen sınır:** `exec fish` sonrası fish'in içinden açılan ssh'ın
  göstergesi fish'in `D`'sini göremiyor ve bir sonraki kimlikli işarete
  (sekme kapanana dek) kalıyor.
- **`mosh-client`'ın `-#`'i bütün komut satırı** (`"-# {argv} |"`, seçenekler
  dahil), "ilk sözcük" değil: değer mosh'un kendi ayrıştırıcısından geçiyor.
  mosh bu makinede kurulu değil; biçim mosh.pl'nin `exec`'inden, sınama
  sahte tabloyla.
- `probe_remote` `Local`'de `set_remote(None)` çağırmıyor: uzak durum `C`'de
  zaten silindi. Süreli koşu `ShellWake::command_started`'da `timed` koluyla
  yoklamıyor (`SilentWake` yalnız sınamanın).
- **Silahın sırası**: iş silahı yoklamadan **önce** indiriyor, kararsızda
  geri kuruyor — yoklama sürerken gelen yeni `C`'nin silahını eski komutun
  kesin cevabı ezmesin (`RemoteProbe`, saf sınama).
- Yol haritasının 036 satırı tazelenmedi (sapma yok).
- `/code-review` (set kapısı, high) sekiz bulgu. **Düzeltilen:** yukarıdaki
  `exec` kabuk kilidi; hedeften sonraki seçenekler (`ssh prod -p 2222`,
  OpenSSH onları yeniden ayrıştırıyor); `-o RequestTTY=…`/`SessionType=…`;
  tırnaklı `--ssh="…"`'in `mosh-client` satırında host sanılması (host'ta
  olamayacak `/ = ~`'lu sözcük atlanıyor); yarı `exec` etmiş boru hattı
  (`ssh prod | tee`) artık `Undecided` — kural "bütün üyeler" değil "hiçbir
  şey tanınmadı ve bir üye kabuğun adını taşıyor"; boru hattında
  etkileşimli ssh etkileşimsizden önce geliyor; `kern.argmax` bir kez
  soruluyor. **Waive:** kabuğun kendi döngüsü ya da `zsh betik` komut boyunca
  `Undecided` kalıp çıktı kenarı başına yoklama doğuruyor — Karar 2'nin adıyla
  kabul ettiği bedel ("ana kuyruk turu başına en çok bir yoklama"); sınır
  koymak ölçülmemiş bir sayı olurdu.
