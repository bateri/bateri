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

- [ ] Karar 7'nin cevabı `discussion.md`'ye işlendi
- [ ] (B) `upload.rs` saf yarı ve süreç yarısı
- [ ] (B) Onay sayfası, tek yükleme bayrağı, nesil kapılı yapıştırma, hata sayfası
- [ ] (A) Uzak kolda ret ve doc
- [ ] `CLAUDE.md` ve yol haritası
- [ ] Test: yukarıdaki Kabul maddeleri
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Gözle kontrol (devir mesajının cümlesi): (B) anahtarla girilen bir
  host'ta `ssh <host>`, uzak kabuk OSC 7 basıyorsa bir dizine `cd`, Finder'dan
  bir dosya bırak — sayfa hedefi söylüyor, Upload sonrası uzak kabuğun
  satırında (**ızgara**, ssh'ın içi) `/uzak/yol/dosya` yazılı ve `ls` onu
  gösteriyor; parolalı bir host'ta sayfa açık bir hata söylüyor, asılı
  kalmıyor. (A) ssh sekmesine sürüklenen dosyada imleç "+" göstermiyor,
  bırakınca geri dönüyor. İkisinde de yerel sekmede damla **dock**'a yolu
  yazıyor.
