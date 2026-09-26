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

- [ ] `ProcessTable::args` ve `KERN_PROCARGS2` gövdesi
- [ ] `jobs::remote` ve ssh/mosh ayrıştırıcıları
- [ ] `ShellWake::command_started`/`wake` silahı ve ana kuyruk işi
- [ ] `set_remote` → `refresh_title`
- [ ] `CLAUDE.md` (yol haritası satırı gerekiyorsa tazelenir)
- [ ] Test: yukarıdaki Kabul maddeleri
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Gözle kontrol (devir mesajının cümlesi): `ssh <gerçek bir host>` —
  **dock**: giriş satırı süzülerek kalkıyor, tek satırda `⇄ host` camgöbeği,
  uzak kabuk OSC 7 basıyorsa yanında yol, üst çizgi camgöbeği; **ızgara**
  aşağı iniyor, **doldurma bandı** tepedeki şeridi geçmişle dolduruyor (uzakta
  `clear`'dan sonra şerit boş — beklenen); **başlık/sekme** `⇄ host` ya da
  `⇄ {uzak başlık}`; uzakta `vim` açınca dock kalkıyor, `⇄` başlıkta kalıyor;
  `exit` → bant süzülerek geri, yerel yol ve dal geri, başlıktan `⇄` gidiyor.
  `ssh host uptime` dock'u oynatmıyor. Varsa `mosh host` da aynı. SF Mono
  kurulu bir makinede bağlam satırındaki `⇄` kutu değil.
