# Phase 4 — Bağlantı kopunca teklif

## Özet

ssh 255 ile bitince dock'un boş giriş satırı `⇄ prod  Connection lost · ⏎
reconnect` diyor; ⏎ aynı komutu yeniden koşturuyor, başka herhangi bir tuş
teklifi kaldırıyor.

_Requirements: R7.1, R7.2, R7.3, R7.4_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `ShellLog`'a teklif yuvası (host,
  çözülmüş işaret, satır). Kuruluşu: uzak oturum etkinken, hedefin türü ssh,
  bizim kimlikli `D`'miz `exit == Some(255)` — 036'nın "uzak oturumu yalnız
  bizim işaretimiz bitirir" kapısının içinde, uzak durumun silindiği aynı
  kolda. Silinmesi: bir sonraki `Running`'e geçiş ve `set_remote` (yeni
  hedef). `A` ve `B` silmiyor. mosh ve 255 dışındaki kodlar teklif doğurmuyor.
- **`crates/bt-core/src/session.rs`** — `send_input` teklifi siliyor (nesil
  artışıyla aynı yerde; yapıştırma ve damla da oradan geçiyor).
  `Session::dock_key`'e düz ⏎ kolu, bugünkü `dock_edit_line()` kapısından
  **önce**: teklif yoksa ilk ifadede `false` — bugünkü Enter yolu bayt bayt
  aynı kalmalı; teklif varken kapı teklif + dock caret'in sahibi
  (`caret_in_dock`) + ayna taze + `BUFFER` boş, düzenleme widget'ına
  (`8133;w`) bağlı değil (Karar 8); geçerse satır + `\r`'yi `send_input`'tan
  gönderip `true`. İşaret değişimi (`set_host_marks`) teklifin rengini de tazeliyor.
- **`crates/bt-shell/src/keys.rs`** — `dock_key` düz `\r`'yi (değiştiricisiz,
  Shift'siz) yeni `DockKey` koluna çeviriyor; ⇧⏎ bugünkü `NewLine`.
  Sözlük sınaması güncelleniyor.
- **`crates/bt-core/src/dock.rs`** — giriş satırı boşken ve teklif varken
  caret'ten sonra yer tutucu: `⇄ {host}` işaretin renginde, iki boşluk,
  `Connection lost · ⏎ reconnect` `dim`'de (UI dizgisi, İngilizce). Öneri
  (`POSTDISPLAY`) ile aynı katman ve aynı sütun aritmetiği (`dock_layout`);
  sığmayan kuyruk kırpılıyor, satır sarılmıyor (yer tutucu satır sayısına
  girmiyor — `needed_rows` değişmiyor). Teklifin bir kopyası kare yolunda
  bağlamla aynı kilit turunda alınıyor (`clone_from`, 036 emsali).
- **`⇄` büyük boy sınıfında** — 036 yalnız küçük sınıfı (bağlam satırı)
  sınadı (`the_remote_mark_is_a_glyph_in_the_small_class`); yer tutucu giriş
  satırında, yani büyük sınıfta ve başka bir yüz merdiveninde. Aynı atlas
  sınamasının büyük sınıf ikizi (Menlo adıyla); kutuysa 036 Karar 7'nin `↔`
  yedeği, ikisi de kutuysa dur ve raporla.

## Kabul

- `ShellLog`: uzak ssh + bizim `D;255` → teklif; `D;0`, `D;1`, mosh + `D;255`,
  kimliksiz `D;255` → teklif yok; `A` teklifi silmiyor, sonraki `C` siliyor.
- `Session`: `send_input` teklifi siliyor; teklifle boş satırda ⏎ satır +
  `\r` yazıyor ve teklifi siliyor; teklif yokken `dock_key(Enter)` `false`
  ve PTY'ye hiçbir şey gitmiyor; `BUFFER` doluyken ⏎ bugünkü yolundan.
- `dock`: yer tutucunun hücreleri ve renkleri (host işaretin renginde, metin
  `dim`); dar pencerede kırpılıyor, `needed_rows` aynı.
- `⇄` büyük sınıfta kutu değil (atlas sınaması).
- Teklif varken düzenleme widget'ı bildirilmemiş (`8133;w` yok) oturumda da
  ⏎ satırı gönderiyor.
- `make duman` yeşil.

## Checklist

- [x] Teklif yuvası, kuruluş ve silinme kenarları
- [x] `send_input` silmesi; `dock_key` ⏎ kolu (teklifsiz erken `false`)
- [x] `keys::dock_key` düz ⏎
- [x] Yer tutucu çizimi; `⇄`'in büyük sınıf sınaması
- [x] Test: yukarıdaki Kabul maddeleri
- [x] (phase-3'ten devralınan, `/code-review` bulgusu; orkestratör kararı
  2026-09-26) **⌘T'den hemen sonra yazılan tuşlar ssh satırının önüne
  yapışıyor** (`ls` + ssh satırı → `lsssh …`). Phase-3'ün önerdiği iki çare
  de reddedildi. `^U` kullanıcının yazdığını sessizce yutar. Tuşları satırın
  arkasına eklemek ise `ssh prod ls` üretip komutu uzakta etkileşimsiz
  koşturur. **Karar:** ⌘T ile doğan uzak sekmede kullanıcı girdisi ilk
  satırımız gidene kadar **tutulur**. Satır, yani ssh komutu artı ⏎, gittikten
  sonra tutulan baytlar **arkasından** aynı sırayla gönderilir. Böylece ssh'ın
  girdisine, yani uzağa düşerler: kullanıcı o sekmeyi uzak için açtı. Hiçbir
  tuş kaybolmaz ve komut bozulmaz. Tutma kimlikli `A` hiç gelmezse de sonsuza
  kadar sürmemeli (Karar 6'nın bilinen sınırı). Çözülmenin tetiği ölçülmemiş
  bir zaman aşımı olmasın: var olan bir kenara bağla (ör. kabuğun çıkışı ya da
  kabuğun ilk çıktısından sonraki ilk kullanıcı tuşu) ve seçimi Uygulama
  Notları'na yaz. Sınama: tutulan girdi satırdan sonra ve sırasıyla gidiyor.
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Gözle kontrol (devir mesajının cümlesi): `ssh <host>`'ta `~.` — ssh
  "Connection to host closed." basıyor, **dock**'un boş giriş satırında
  `⇄ host  Connection lost · ⏎ reconnect` (host işaret renginde), ⏎ aynı
  komutu yeniden koşturup bağlanıyor; ikinci denemede bir harf yazınca yer
  tutucu kalkıyor ve geri gelmiyor. Uzakta `exit` (kod 0) teklif göstermiyor.
  Yer tutucu bir tuş vuruşu boyunca bile ızgaraya sıçramıyor.

## Uygulama Notları

- **Teklif `DockContext`'te** (`reconnect: Option<Reconnect>`; host, çözülmüş
  işaret, satır), `ShellLog`'un kendi alanı değil: kare yolu bağlamı zaten
  aynı kilit turunda `clone_from` ile alıyor. `Reconnect`'in `Clone`'u elle
  (kapasite). `set_host_rules` teklifin işaretini de yeniden çözüyor ve
  değişimini `true` diye bildiriyor (sekme noktası tazelemesi zararsız no-op).
- `send_input` teklifi yaprak kilitte siliyor ve varsa kare istiyor (yer
  tutucu alacritty'nin hasarında yok).
- **Numpad Enter da `DockKey::Enter`** (Control'süz U+0003; `encode_key` onu
  `\r`'ye çeviriyor): aynı tuşun ikinci yüzü, teklif yokken fark yok.
- Yer tutucu yalnız `BUFFER`, `PREBUFFER` **ve** `POSTDISPLAY` boşken (öneri
  aynı katman). `·` ve `⏎` de Menlo'nun büyük sınıfında sınanıyor (Kabul
  yalnız `⇄`'i istiyordu; üçü de kutu değil); dizge ile atlas sınaması
  `the_reconnect_hint_is_the_one_the_atlas_checks` ile bağlı.
- **Devralınan: ⌘T'den önce yazılan tuşlar tutuluyor.** Tutma yuvası
  (`HeldInput`, `Arc<Mutex<Option<Vec<u8>>>>`) yalnız ilk girdi prompt'u
  beklerken kuruluyor; `send_input` gönderimi `send_or_hold`'dan geçiyor,
  okuyucu thread'i satırı + tutulanları **tek gönderimde ve yuvanın kilidi
  altında** yolluyor — iki yazar aynı kanala, sıra kilidin sırası. Nesil
  tutulan her tuşta da artıyor (tazelik kapısı yalnız "değişti mi" soruyor).
  **Sonsuz tutmayı önleyen kenar: kullanıcının `\r`, `\n` ya da `^C`'si**
  tutulanı o anda gönderiyor; tutma **satırın teslimine kadar sürüyor**
  (`/code-review`: ilk ⏎'de bitseydi `ls⏎pwd` → `pwdssh …`). Orkestratörün
  iki örneği tartıldı: "kabuğun çıkışı" tek başına yetmiyor (kabuk yaşıyor
  ama `A` basmıyor: `exec fish`); "ilk çıktıdan sonraki ilk tuş" p10k
  instant prompt'ta ve motd basan rc'de `A`'dan çok önce ateşleniyor, yani
  `lsssh`'ı o kullanıcılara geri getirirdi. **Bilinen sınırlar:** tutma
  boyunca yankı yok (yazılan ⏎'ye kadar görünmüyor); `A` hiç gelmezse
  (`exec fish`) o sekme satır satır kalıyor — tab tamamlama ve oklar ⏎'yi
  bekliyor; rc'de tek tuş okuyan bir soru (`read -k 1`) ⏎ istiyor ve fazla
  `\r` boş bir komut satırı olarak düşüyor.
- **`/code-review` bulguları (dördü de giderildi):** ⏎'nin kapısına ekleme
  keymap'i ve `holding_end` (`can_be_typed`'ın kemeri — `vicmd`'de `ssh
  prod⏎` vi komutu olurdu); tutmanın yukarıdaki süreklilik kuralı; tek tuşluk
  soru notu (yukarıda); çizim ile ⏎'nin kapısı hizalandı — ⏎ öneriyi de
  soruyor, yer tutucu `owned` ve ekleme keymap'i istiyor (tazelik çizimde
  sorulamıyor ama teklif her girdide silindiği için teklif varken ayna
  zaten son girdinin cevabı).
