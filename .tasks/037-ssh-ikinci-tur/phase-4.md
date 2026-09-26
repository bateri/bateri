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

- [ ] Teklif yuvası, kuruluş ve silinme kenarları
- [ ] `send_input` silmesi; `dock_key` ⏎ kolu (teklifsiz erken `false`)
- [ ] `keys::dock_key` düz ⏎
- [ ] Yer tutucu çizimi; `⇄`'in büyük sınıf sınaması
- [ ] Test: yukarıdaki Kabul maddeleri
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Gözle kontrol (devir mesajının cümlesi): `ssh <host>`'ta `~.` — ssh
  "Connection to host closed." basıyor, **dock**'un boş giriş satırında
  `⇄ host  Connection lost · ⏎ reconnect` (host işaret renginde), ⏎ aynı
  komutu yeniden koşturup bağlanıyor; ikinci denemede bir harf yazınca yer
  tutucu kalkıyor ve geri gelmiyor. Uzakta `exit` (kod 0) teklif göstermiyor.
  Yer tutucu bir tuş vuruşu boyunca bile ızgaraya sıçramıyor.
