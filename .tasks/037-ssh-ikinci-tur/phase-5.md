# Phase 5 — Finder damlası ssh'ta ve sözleşme

## Özet

Uzak oturumda Finder'dan bırakılan dosya artık uzak kabuğa yerel bir yol
yazmıyor: kullanıcının Karar 7'deki seçimine göre uzak dizine yükleniyor (öneri,
B) ya da reddediliyor (A). Setin sözleşmesi `CLAUDE.md`'ye ve yol haritasına
iniyor.

_Requirements: R8, R9_

> **Bu phase Karar 7'nin cevabını bekliyor.** Aşağıdaki Değişiklikler B
> içindir; cevap A ise "Seçenek A seçilirse" bölümü onların yerine geçer ve
> Kabul'ün B maddeleri düşer. Seçim `discussion.md` → Karar 7'ye ✅ ile
> işlenmeden kodlanmaz.

## Değişiklikler

- **`crates/bt-shell/src/upload.rs`** (yeni modül) — saf yarı: uzak hedefin
  argv'sinden `scp` argv'si (Karar 7 → Seçenek B'nin çeviri tablosu:
  `-p`→`-P`, `-l`→`-o User=`, `-S`→`-o ControlPath=`, `-i -J -F -o -4 -6`
  aynen, kalan düşer; her zaman `-o BatchMode=yes`, dizin varsa `-r`); hedef
  yol (`remote_cwd` doluysa o, değilse `host:` ve `~/`); yapıştırılacak uzak
  yollar (`quote::shell_quote`'tan). mosh hedefinde yalnız host. Süreç yarısı:
  `std::process::Command` arka plan thread'inde, sonuç ana kuyruğa
  (`dispatch2`, `child_exit` emsali); stderr'in son satırı hata metni.
- **`crates/bt-shell/src/view.rs`** — `performDragOperation:` uzak oturumda
  yapıştırmıyor, pencereye yükleme isteğini veriyor; `draggingEntered:`
  pencerede yükleme sürüyorsa `None`. Doc'lar yeni davranışa göre
  yeniden yazılıyor (bugünkü "koşulsuz, eleme kayıtta" gerekçesi uzak kolda
  artık doğru değil).
- **`crates/bt-shell/src/window.rs`** — onay sayfası ("Upload N items to
  host:/path?" — Upload / Cancel; 028'in `NSAlert` sayfası emsali), pencere
  başına tek yükleme bayrağı, bitişte **uzak oturumun nesli hâlâ aynıysa**
  (`running_command()` yoklamanın aldığı nesle eşit ve uzak) uzak yolları
  `Session::paste`'e, değilse sessizce bitiş; hatada `scp`'nin satırıyla
  `NSAlert` sayfası.
- **`CLAUDE.md`** — tema: "Bugün sekizi tüketiliyor … kalan tek durum rolü
  (uyarı) sonraki setlerde" → dokuzu, `warning` host işaretinden; ssh
  paragrafı (036): host işareti ve renkleri, sekmenin noktası, teklif,
  ⌘T/⌥⌘T; ana menü listesi (Shell'de New Local Tab ve Mark … as ▸); Finder
  damlası cümlesi uzak kolla; ayar anahtarları listesine `[remote] hosts`;
  "Ayarlar" maddesinin "Dosyaya yazan üç yol" cümlesine dördüncü yol
  (Shell ▸ Mark … as ▸, `SettingsEdit::RemoteHostMark`; phase-2'den devir).
  Kural + tek cümle gerekçe + işaretçi (`.tasks/037-ssh-ikinci-tur/`),
  tarihçe değil.
- **`docs/YOL-HARITASI.md`** — 037 satırı set açılırken yazıldı; sapma
  olduysa tazelenir.

### Seçenek A seçilirse

- **`crates/bt-shell/src/view.rs`** — uzak oturumda `draggingEntered:`
  `None`, `performDragOperation:` `false`; `draggingEntered:`'ın doc'u artık
  oturuma sorduğunu ve neden sorduğunu söylüyor. `upload.rs` ve sayfa yok.
- `CLAUDE.md`'nin damla cümlesi "uzak oturumda damla reddedilir".

## Kabul

- (B) `upload` saf sınamaları: çeviri tablosu (`ssh -p 2222 -l deploy -J
  jump -i k prod` → `scp -P 2222 -o User=deploy -J jump -i k -o
  BatchMode=yes …`), dizinde `-r`, OSC 7'li ve OSC 7'siz hedef, yapıştırılacak
  yolların kaçırılması, mosh'ta yalnız host.
- (B) Nesil kapısı saf: yükleme sürerken uzak oturum bittiyse yapıştırma yok.
- (A ve B) Yerel sekmede damla bugünkü gibi yolu yapıştırıyor (mevcut
  sınamalar yeşil).
- `CLAUDE.md` setle çelişmiyor (dokuz rol, menü listesi, ⌘T, damla).

## Checklist

- [x] Karar 7'nin cevabı `discussion.md`'ye işlendi (B, genişletilmiş — "Kullanıcı kararı")
- [x] (B) `upload.rs` saf yarı ve süreç yarısı
- [x] (B) Onay sayfası, tek yükleme bayrağı, nesil kapılı yapıştırma, hata sayfası
- [x] (Kullanıcı kararı) Replace/Merge, `df` ön kontrolü, klasörde sayı ve boyut
- [x] (Kullanıcı kararı) ara arşivsiz `tar c | ssh … tar x`, sıralı kuyruk, `▴ list` (öğe başına ✕, Cancel All)
- [x] (Kullanıcı kararı) durum satırı, üst çizgide ilerleme, Dock simgesinde ilerleme
- [x] (Kullanıcı kararı) ⌘. ve ✕ ile iptal + yarım dosyanın silinmesi; disk dolu; ssh kapanması
- [~] (A) Uzak kolda ret ve doc — A seçilmedi
- [x] `CLAUDE.md` ve yol haritası (phase-2'den devralınan "dört yol" dahil)
- [x] Test: yukarıdaki Kabul maddeleri
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Gözle kontrol (devir mesajının cümlesi)

## Uygulama Notları

- **Kapsam kullanıcı kararıyla genişledi ve tek phase'te kaldı** (bölünmedi):
  B'nin `scp`'si yerine `tar c | ssh … tar x` (dosyada da — kullanıcının
  gördüğü aynı, ilerleme kesin). Yukarıdaki "Değişiklikler"in `scp` çeviri
  tablosu yerine `upload::ssh_argv`: seçenekler **süzülüyor** (bağlantıyı
  değiştirenler kalıyor, `-t -n -N -W -s -O -v` düşüyor) ve bizimkiler başta
  (`-T -o BatchMode=yes -o ControlMaster=no`) — ssh bir anahtarın **ilk**
  değerini alıyor. `ControlMaster=no` açık bir ana bağlantıyı kullanıyor ama
  ana olmuyor (arka plana düşen ana bağlantı borunun ucunu tutardı).
- **Uzak komut `sh -c '…'`, tırnak ters bölüsüz** (`'"'"'`): giriş kabuğu fish
  olabilir ve fish tek tırnağın içinde `\'`'yi kaçış sayıyor. Kalan delik
  adıyla: adında ters bölü ya da kontrol karakteri olan öğe (ya da uzak
  dizin) sayfada reddediliyor. Yoklama tek bağlantıda (`echo BT-UPLOAD`
  işaretiyle rc gürültüsünü atlıyor; adlar değil indeksler); sınaması betiği
  `/bin/sh`, `bash` ve `zsh` giriş kabuğu altında gerçekten koşturuyor.
- **Akışın sınaması bağlantısız:** ssh'ın yerine `/bin/sh -c` — uzakta koşacak
  betik ve akış bayt bayt aynı (`a_folder_travels_through_the_stream…`).
- **İlerleme `TarWatcher`'dan:** bsdtar'ın pax başlıkları (`path=`) ve 256
  tabanlı boy okunuyor; `COPYFILE_DISABLE=1` + `--no-mac-metadata --no-xattrs
  --no-acls` (yoksa uzakta `._ad` dosyaları ve GNU tar'da uyarı). Bayt
  sayacı içerik baytı (başlıklar hariç), yani `18.2 / 44.6 MB` dosyaların
  boyuna göre. Uzakta `tar -x -p -o` (root'a yüklenen dosya yerel uid'ye
  düşmesin).
- **İptal süreçleri öldürüyor** (`Shared::kill`, `libc::kill`): akış thread'i
  yavaş bağlantıda yazımda bloklu ve bayrağa bakmıyor. Yarım dosya akışın son
  başlığından (`TarWatcher::current`), siliniyor ayrı bir `ssh … rm -f`.
  Disk dolu stderr'deki `No space left on device` satırından (GNU tar hatadan
  sonra akışı yutmaya devam ediyor; satır görülünce akış hemen duruyor).
- **ssh kapanınca akan öğe kendi bağlantısıyla bitiyor**, yalnız bekleyenler
  iptal ("bekleyen öğeler iptal olur") ve yolu yapıştırılmıyor. Kenar
  `refresh_title` (`D`/`A`'nın `title_changed`'i). Sekme kapanınca
  (`begin_close`) kuyruk iptal.
- **Durum satırı `bt-core`'da çiziliyor, metni `bt-shell`'de doğuyor**
  (`DockContext::transfer`): gövde sağdan `…` ile kısalıyor, düğmeler
  kısalmıyor ve sığmazsa yok. Düğmelerin yeri çizim ile farenin tek formülü
  (`bt_core::transfer_controls_col`); tık bağlam satırında küçük sınıfın
  adımıyla sütuna iniyor (`bt_gpu::context_cols` `pub` oldu). Satırın ASCII
  dışı karakterleri `bt_core::UPLOAD_GLYPHS` sözlüğünde ve `bt-atlas`
  küçük sınıfta kutu olmadıklarını soruyor.
- **Liste AppKit'in açılır menüsü** (öğe başına `✕ ad`, akan öğede
  "(uploading)", ayraç, Cancel All): "dock'un üstünde küçük bir liste"nin
  yerli karşılığı. Akan öğenin ✕'i yalnız onu durduruyor, kuyruk sürüyor.
- **Üst çizgi çubuğa dönüyor:** `Frame::dock_ground` dört dörtlü (zemin,
  çizginin zemini, dolan kısım, ikinci ayraç); ilerlemede çizginin zemini
  ayracın rengi, dolan kısım işaretin. Kalınlık saç çizgisininki.
- Boş alan yalnız bu damlayla karşılaştırılıyor (kuyrukta bekleyen baytlar
  düşülmüyor — bilinen sınır, `upload::sheet`'in doc'u). Hedef bilinmiyorsa
  (OSC 7 yok) ev dizini ve yapıştırılan yol mutlak (`pwd`'den), `~/ad` değil.
- Üç tasarım sabiti: `upload::TICK` (200 ms haber aralığı), `SPEED_WINDOW`
  (3 s), `LINGER` (4 s sonuç satırı — durma koşulu).
- `objc2-app-kit`'e `NSDockTile` ve `NSProgressIndicator` bayrakları (yalnız
  başlık, `Cargo.lock` oynamadı); `make denetim` `Cargo.toml` farkı için
  uyarıyor, karar kaydı Kullanıcı kararı 4.
- **Bilinen sınırlar:** parola sorulamaz (BatchMode; anahtar/agent ya da açık
  ControlMaster); dock'suz pencerede (`[shell] integration = "blocks"`) durum
  satırı yok, ilerleme yalnız Dock simgesinde; dosya ↔ klasör tür çakışması
  (uzakta aynı adda klasör, yerelde dosya) tar'ın hatasıyla bitiyor.
- **Set kapısı `/code-review` bulguları (beşi de giderildi):** sekme kapanınca
  Dock simgesinin çubuğu donuyordu (`abandon_uploads` kuyruğu bırakıp simgeyi
  tazeliyor); `df` hiçbir şey basmazsa yoklamanın cevabı bir satır kayıyordu
  (satırlar kendi işaretini taşıyor, `BT-DF`); iptal edilip henüz bitmemiş
  kuyruğa onaylanan damla sessizce düşüyordu (`can_accept` o hâlde hayır);
  tek öğeyi listeden iptal "✓" diyordu (sırada başka öğe yoksa kuyruğun
  iptali); iptal toplanmış bir pid'e sinyal gönderebiliyordu
  (`wait_untracked`: `waitid(WNOWAIT)` → listeden çıkar → topla).
  `/audit`: mekanik temiz, yedi mercek temiz.
