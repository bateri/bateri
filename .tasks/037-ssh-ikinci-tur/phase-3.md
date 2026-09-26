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

- [ ] `SessionOptions` ilk girdi ve teslim kuralı
- [ ] Okuyucu döngüden `A`'da yazım
- [ ] `open_window` başlatma argümanı; ⌘T/`+` uzak, ⌘N yerel
- [ ] New Local Tab (⌥⌘T)
- [ ] Test: yukarıdaki Kabul maddeleri
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Gözle kontrol (devir mesajının cümlesi): `ssh -p <port> <host>` (ya da
  `-J`'li) sekmede ⌘T — yeni sekmenin **dock**'unda aynı komut belirip
  koşuyor, **ızgarada** komut bloğu işaretiyle, bağlanınca dock tek satırlık
  `⇄ host`'a iniyor ve başlık `⇄`; `exit` yeni sekmeyi yerel kabukta,
  ⌘T'nin açıldığı yerel dizinde bırakıyor. ⌥⌘T ve ⌘N aynı sekmeden yerel
  açıyor. Sekme çubuğunun `+`'sı ⌘T gibi.
