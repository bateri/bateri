# Phase 3 — ⌘T aynı host'a ve New Local Tab

## Özet

ssh sekmesinde ⌘T (ve sekme çubuğunun `+`'sı) yeni sekmede aynı ssh/mosh
komutunu kendiliğinden koşturuyor; Shell ▸ New Local Tab (⌥⌘T) her zaman
yerel bir sekme açıyor.

_Requirements: R6.1, R6.2, R6.3_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `SessionOptions`'a ilk girdi
  (`Option<String>`, yazılacak satır). Teslimin kuralı Karar 6: sarmalayıcı
  kurulan oturumda (`dock` değil — `blocks` kademesi de işaret basıyor;
  bilgiyi `bt-shell`'in `shell_integration`'ı veriyor) bizim ilk kimlikli
  `A`'mızda, sarmalayıcısızda doğumdan hemen sonra; kimlikli `A` hiç
  gelmezse satır gitmiyor (Karar 6'nın bilinen sınırı, zaman aşımı yok);
  satır + `\r`, `send_input`'un yolundan (nesil
  ilerliyor, tazelik kapısı onu kullanıcı girdisi gibi görüyor). Tek atımlık:
  teslimden sonra yuva boş, ikinci prompt'ta yeniden gitmiyor.
- **`crates/bt-core/src/reader.rs` / `shell.rs`** — `A` kenarının haberi
  okuyucu döngüden teslime; yazım döngünün kendi yazma kuyruğuna (okuyucu
  döngü bizim, 035) — `Term` kilidi altında PTY'ye yazılmıyor, kilit sırası
  bugünkü.
- **`crates/bt-shell/src/app.rs`** — `open_window` bir "başlatma" argümanı
  alıyor: yerel mi, yoksa `from`'un uzak hedefinin satırı mı. ⌘T ve
  `newWindowForTab:` uzak sekmede satırı veriyor; ⌘N ve Dock ikonu her zaman
  yerel; yeni `newLocalTab:` eylemi her zaman yerel sekme. Dizin mirası (026
  Karar 4) üçünde de aynen — uzak sekmede `working_directory()` yerel dizini
  veriyor (036 Karar 4), doğrula.
- **`crates/bt-shell/src/window.rs`** — `start` ilk girdiyi `SessionOptions`'a
  geçiriyor. Süreli koşu yolu (`BT_RUN_SECONDS`) dokunulmuyor.
- **`crates/bt-shell/src/menu.rs`** — Shell ▸ New Local Tab (⌥⌘T), New Tab'ın
  altında.

## Kabul

- `bt-core` sınaması (sahte kabuk akışıyla): ilk girdi sarmalayıcılı
  oturumda ilk kimlikli `A`'dan önce yazılmıyor (kimliksiz `A` saymıyor),
  `A`'da bir kez yazılıyor, ikinci `A`'da yazılmıyor; sarmalayıcısız oturumda
  doğumda.
- Gerçek PTY sınaması (mevcut `/bin/sh` kalıbı, dock'suz): ilk girdi olarak
  verilen `echo …` satırının çıktısı ızgarada.
- `open_window`'un başlatma kararı saf bir fonksiyonda: (uzak `from`, ⌘T) →
  satır; (uzak `from`, ⌥⌘T / ⌘N) → yerel; (yerel `from`, ⌘T) → yerel.
- `make duman` yeşil (jetonlar bugünkü).

## Checklist

- [x] `SessionOptions` ilk girdi ve teslim kuralı
- [x] Okuyucu döngüden `A`'da yazım (`reader.rs` değişmedi, notlarda)
- [x] `open_window` başlatma argümanı; ⌘T/`+` uzak, ⌘N yerel
- [x] New Local Tab (⌥⌘T)
- [x] Test: yukarıdaki Kabul maddeleri
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Riskli phase: `/code-review` koştu; tek bulgu waive (notlarda, kullanıcıya soru)
- [ ] Gözle kontrol (devir mesajının cümlesi): `ssh -p <port> <host>` (ya da
  `-J`'li) sekmede ⌘T — yeni sekmenin **dock**'unda aynı komut belirip
  koşuyor, **ızgarada** komut bloğu işaretiyle, bağlanınca dock tek satırlık
  `⇄ host`'a iniyor ve başlık `⇄`; `exit` yeni sekmeyi yerel kabukta,
  ⌘T'nin açıldığı yerel dizinde bırakıyor. ⌥⌘T ve ⌘N aynı sekmeden yerel
  açıyor. Sekme çubuğunun `+`'sı ⌘T gibi.

## Uygulama Notları

- **`reader.rs`'e dokunulmadı.** Teslim `TappedPty::read`'de: `ScanOutcome`
  yeni `prompt` bayrağını (yalnız kimlikli `A`) taşıyor ve satır + `\r`
  `Adapter::reply`'ın kanalından döngünün yazma kuyruğuna gidiyor — kanal
  kilitsiz, `read` `Term` kilidi altında da koşabildiği için PTY'ye doğrudan
  yazılmıyor. `TappedPty` bunun için `Adapter`'ın bir kopyasını taşıyor
  (`Adapter::new` `TappedPty`'nin önüne alındı).
- **"`send_input`'un yolundan"ın iki hâli:** doğumda (sarmalayıcısız)
  gerçekten `write_owned` → `send_input`; `A`'da okuyucu thread'i `Session`'ı
  görmediği için yalnız neslin artışı + kanal. Seçim temizliği ve dibe
  dönüş orada yok — taze oturumda ikisi de no-op; kullanıcı ilk prompt'tan
  önce geçmişe kaydırmışsa pencere dibe dönmüyor (bilinen, kozmetik).
- `SessionOptions`'a iki alan: `initial_input` ve `shell_marks`
  (sarmalayıcı kuruldu mu; `bt-shell`'de `!integration.is_empty()`, `dock`'tan
  türetilmiyor — `blocks` kademesi). Boş satır `None` sayılıyor.
- `open_window`'un `as_tab`'ı üç kollu `Opening`'e döndü (Window/Tab/
  LocalTab); karar saf `initial_line`'da. `TerminalWindow::start`'ın dizin ve
  ilk girdisi `window::Launch`'ta (clippy'nin argüman sınırı).
- Yeni API: `Session::remote_line` (yaprak kilit).
- **Waive (`/code-review`, orta): açılış sırasında yazılan tuşlar satırın
  önüne yapışıyor.** Sarmalayıcılı oturumda satır ilk kimlikli `A`'da
  gidiyor; kullanıcı ⌘T'den hemen sonra rc yüklenirken `ls` yazarsa ZLE
  `ls` + `ssh -p 2222 prod⏎`'i tek satır okuyor ve `lsssh …` koşuyor. Doğum
  kolunda (sarmalayıcısız) yok. Giderilmedi, çünkü iki çare kullanıcının
  gördüğünde ayrışıyor ve bu bir ürün kararı: (a) satırın önüne `^U`
  (`kill-whole-line`) — önceden yazılan tuşlar sessizce **düşer**; (b) ilk
  girdi gidene kadar kullanıcı girdisini tutup satırın **arkasına** eklemek
  — tuşlar uzak kabuğa gider ama kimlikli `A` hiç gelmezse (Karar 6'nın
  bilinen sınırı) klavye ölür, yani bir zaman aşımı (ölçülmemiş sayı)
  ister. Bugünkü hâl: yanlış komut koşar, sekme bağlanmaz; kullanıcı
  görür ve yeniden yazar.

