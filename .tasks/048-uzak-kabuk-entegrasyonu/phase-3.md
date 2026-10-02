# Phase 3 — Uzakta komut blokları

## Özet

Uzak betikler OSC 133'ü kendi alanıyla basar; `bt-core` onları yerel
safhaya ve uzak oturuma dokunmadan ayrı bir izde tutar ve şerit, chevron ve
süre sayacı uzakta da çizilir.

_Requirements: R5_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — faz + `running_since` + `BlockLog`
  üçlüsü bir tipe (`BlockTrack`) çıkar; yerelde ve uzakta iki örnek.
  `stripe`/`duration`/`running` kaynağa göre yönlenir. `ShellLog::apply`
  uzak kimliği (`bt_remote=<P>.<n>`) `identified` hesabından **önce** ayırır:
  `ours`, `command_open`, `outcome.started`, `clear_remote` ve
  `outcome.prompt` (⌘T'nin `initial_input`'u) uzak işaretten etkilenmez.
  Uzak iz `context.remote`'a bağlanmaz; `P` değişince temizlenir, ssh
  bitince silinmez (şeritler geçmişte kalır).
- **`crates/bt-core/src/session.rs`** — blok çıpası `bateri://rblock/<P>.<n>`
  ayrı ad alanı; çıpayı okuyan iki çizim döngüsü ve `block_row_continues` iki
  ad alanını tanır; yerel çıpanın öbür kullanıcıları (⌘K, bastırma) uzak
  çıpayı görmez (bugünkü önek yüzünden zaten; bekçi sınamayla).
- **`assets/shell/remote/`** — zsh/bash/fish betikleri `A`/`B`/`C`/`D`'yi
  `bt_remote=<P>.<n>` ile basar; `P` önyüklemeye yerel ssh komutunun blok
  kimliği (`$__bateri_block`) olarak `ssh-argv` üzerinden geçer.
- **`crates/bt-shell-common/src/ssh_wrap.rs`** — `P`'nin argv'ye taşınması
  (`wrap`/`unwrap` gidiş-dönüşü korunur).
- **`docs/`** gerekmez; `CLAUDE.md`'de uzak OSC 133 cümlesi güncellenir.

## Kabul

- `bt-core` sınamaları: uzak `A`/`D` uzak oturumu bitirmiyor, yerel safhayı,
  saati ve ⌘T'nin ilk girdisini oynatmıyor; iki uzak oturumun `rblock/1`'i
  birbirinin rengini ezmiyor (`P` farklı); ssh bitince uzak satırların
  şeridi kalıyor; sarılan uzak komutun devam satırı işaret almıyor.
- PTY simülasyonu: uzak betikten beklenen 133 baytları.
- `make check`, `make test-race`, `make bundle`, `make linux` yeşil.
- Gözle kontrol: ssh'ta `ls` ve `false` → şeritler yeşil/kırmızı, `sleep 3`
  sayacı; ssh'tan çıkınca şeritler duruyor. Izgarada ve doldurma bandında.

## Uygulama Notları

<!-- `bt_block=` taklidi (discussion → Karar, teknik) burada yeniden
     değerlendirilir. -->

## Checklist

- [ ] `BlockTrack` tipi ve iki örnek
- [ ] `apply`'de erken uzak ayrım
- [ ] `rblock` ad alanı ve okuyucular
- [ ] Uzak betiklerde 133
- [ ] `P`'nin taşınması
- [ ] `CLAUDE.md` cümlesi
- [ ] Test: yukarıdaki `bt-core` senaryoları, PTY simülasyonu
- [ ] Doğrulama geçti (`make check` + `make test-race` + `make bundle` + `make linux`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
