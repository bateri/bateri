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
  uzak kimliği (`bt_remote=<P>.<S>.<n>`) `identified` hesabından **önce** ayırır:
  `ours`, `command_open`, `outcome.started`, `clear_remote` ve
  `outcome.prompt` (⌘T'nin `initial_input`'u) uzak işaretten etkilenmez.
  Uzak iz `context.remote`'a bağlanmaz; `P` değişince temizlenir, ssh
  bitince silinmez (şeritler geçmişte kalır).
- **`crates/bt-core/src/session.rs`** — blok çıpası `bateri://rblock/<P>.<n>`
  ayrı ad alanı; çıpayı okuyan iki çizim döngüsü ve `block_row_continues` iki
  ad alanını tanır; yerel çıpanın öbür kullanıcıları (⌘K, bastırma) uzak
  çıpayı görmez (bugünkü önek yüzünden zaten; bekçi sınamayla).
- **`assets/shell/remote/`** — zsh/bash/fish betikleri `A`/`B`/`C`/`D`'yi
  `bt_remote=<P>.<S>.<n>` ile basar; `P` önyüklemeye yerel ssh komutunun blok
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

- **Uzak işaret ayrı bir olay, `apply`'in içinde bir dal değil**: tarayıcı
  `bt_remote=` taşıyan payload'u `ScanEvent::RemoteMark`'a çeviriyor ve
  `apply_scan_answering` onu `apply_remote`'a veriyor — yerel `apply` uzak
  işareti tipiyle alamıyor. Planın "`identified`'dan önce ayır" maddesinin
  güçlü hâli: `identified`'dan önce de iki yerel yan etki var
  (`end_since.take()` → `end_line()`, `command_open = false`) ve ayrım
  ikisinden de önce. Bekçisi `remote_marks_touch_nothing_local` (`ours`,
  `command_open`, tutulan `line-finish`, yerel saat ve safha, uzak durum ve
  üç `ScanOutcome` biti değişmiyor).
- **Alan dört harfin hepsinde** (`bt_block=` yalnız `A`/`D`'de): yoklama
  uzak durumu kurmadan önceki pencerede kimliksiz bir uzak `B`/`C` yerel
  safhayı sürerdi. Kimliğin yanında bir `bt_block=` varsa uzak alan kazanıyor
  (yanlışın yönü yerele dokunmayan iz); bozuk alan (`7`, `7.`, `x.1`)
  bizim değil, harf yerel kimliksiz işaret olarak kalıyor.
- **Kimlik `<P>.<S>.<n>`, planın `<P>.<n>`'i değil** (`/code-review`
  bulgusu): tek komut satırı iki bağlantı açabiliyor (`ssh a; ssh b`, `for`
  döngüsü) ve ikisi de aynı `P`'yi alıp birden sayıyor — ikinci oturumun
  `rblock/P.1`'i birincinin bloğunu yeniden açıp satırlarını boyardı. `S`
  uzak kabuğun pid'i (zsh/bash `$$`, fish 3 `fish_pid`; fish 2'de blok yok);
  iz `RemoteShell { parent, pid }` değişince temizleniyor, uzak saatin kapısı
  yine yalnız `P`. Alanın ve çıpa yolunun tek okuması `shell::remote_key`.
- **`BlockTrack` = `state` + `running_since` + `BlockLog`**; yerel `ShellLog`
  `local`'ı, uzak `remote`'u ve `remote_parent`'ı taşıyor. Kurallar
  (`prompt`/`command`/`end`, "ilk `C` kazanır", "saati yalnız kimlikli `D`
  tüketir") tek yerde, yerel `apply` de onları çağırıyor; gerekçe yorumları
  tipe taşındı.
- **Uzak blok yalnız `P` açık yerel komutken koşuyor**
  (`ShellLog::running_blocks`: `running_command()` + yerel defterin son açık
  kaydı `P`): kopan bağlantı uzak `D` bırakmaz ve uzak iz sonsuza dek
  "koşuyor" der, yani accent şerit ve boşta tikleyen sayaç. Bizim yerel
  `D`'miz ikinci bir yol olmadan bitiriyor; blok `Pending` kalıyor, çizilmiyor
  (yerel `exit` komutunun kaderi). Bekçisi
  `our_local_d_ends_the_remote_clock_and_keeps_the_stripes`.
- **Çıpa basılıyor, `PS1`'e konmuyor** (yerel sarmalayıcıdan sapma): uzakta
  prompt kullanıcının ve dokunulmuyor; zsh `PROMPT_SP`'nin işaretini
  `precmd`'den **önce** basıyor (ölçüldü, PTY), yani son kanca olan
  `precmd`'imizin açtığı bağlantı yalnız prompt'u ve yazılan komutu
  kapsıyor; `precmd`'siz yeniden çizim (Ctrl-L, SIGWINCH) açık bağlantının
  içinde. Temanın kendi OSC 8'i bizimkini kapatır (yerelinki gibi bilinen
  sınır).
- **`B` yok**: uzak izde tüketicisi yok (bastırma ve dock caret'i yerelin);
  basmak için zsh'te `PS1`'e ya da `line-init`'e dokunmak gerekirdi.
- **bash: kapanış `PS0`'dan, yani bash 4.4+**; öncesinde blok işareti hiç
  basılmıyor — `DEBUG` trap'i kullanıcının ve kapanmayan çıpa bütün çıktı
  satırlarına yayılırdı. "Komut koştu" bayrağı `PS0`'ın aritmetik alt
  simgesiyle (`${__bateri_none[__bateri_ran=1]-}`, `set -u`'da da boş); boş
  satır `PS0` basmıyor, yani blok kapatmıyor (zsh'in `preexec` kuralı).
  Bilinen sınır: kullanıcının `PROMPT_COMMAND`'ı bizimkinden sonra koşuyor
  (bizimki `$?`'ı okumak için ilk) ve çıktısı çıpayı taşıyor. macOS'un bash
  3.2'sinde sınama `SKIPPED`, `make linux` imajında (bash 5) koştu.
- **fish**: `A` + çıpa `fish_prompt`, kapanış + `C` `fish_preexec`, `D`
  `fish_postexec`'in `$status`'uyla ve yalnız başlamasını gördüğümüz komutta.
- **`P`'nin teli**: `bateri ssh-argv [--tty] [--block N] -- …`; uzak komut
  `exec sh -c '<tek satır>' bateri-boot <P>` (`sh`'nin `$1`'i; csh/fish'te de
  düz bir kelime), önyükleme yalnız rakamsa `BATERI_RBLOCK` olarak ihraç
  ediyor, betikler kabuk değişkenine alıp ortamdan siliyor. `unwrap` sonu
  `bateri-boot` ya da `bateri-boot <rakamlar>` olan biçimi kabul ediyor
  (gidiş-dönüş `None`/`Some(7)`/`Some(u32::MAX)` ile); bozuk ya da boş
  `--block` yalnız blokları kaybettiriyor, bağlantı yine sarılıyor
  (`/code-review` bulgusu: önce düz `ssh`'a düşüyordu ve OSC 7'yi de
  götürüyordu). `--block`'suz çağrıda uzak blok yok.
- **Uçtan uca**: PTY simülasyonu (`ssh_wrap::remote_shells`) sshd'nin
  `"$SHELL" -c '<komut>'`'unu `P = 41` ile koşuyor ve şeridi + sayacı
  `Blocks`'tan okuyor (`draws_remote_blocks`: `false` → hata, öncekiler →
  başarı, `sleep 1.5` → `1.5s`–`1.9s`); zsh macOS'ta, zsh/bash 5/fish
  `make linux`'ta yeşil. Gerçek sshd koşulmadı: yeni olan yalnız uzak
  komutun sonundaki bir kelime ve sshd'nin ilettiği dizgi simülasyonunkiyle
  aynı (phase-2 aynı biçimi gerçek sshd'den geçirdi).
- **`bt_block=` taklidi yeniden değerlendirildi** (discussion → Karar):
  oturum anahtarı yine **eklenmedi**. Uzak betiklerimiz `bt_block=` basmıyor;
  bu phase'den sonra bizim uzak işaretimizin kendi alanı ve izi var, yani
  meşru bir uzak yolun `bt_block=`'e ihtiyacı kalmadı. Taklit edebilecek tek
  kaynak bilerek basan bir sunucu ve en kötü etkisi uzak göstergeyi erken
  silmek ya da bir şeridi yanlış boyamak (komut koşturmaz, bayt göndermez); anahtar
  yerel işaretin biçimini ve yerel betiği değiştirirdi.
- **⌘K uzak çıpayı okuyor, bastırma okumuyor** (plan "⌘K ve bastırma uzak
  çıpayı görmez" diyordu; `/code-review` bulgusu, "kodu açmak" kuralı): ssh
  içinde sarılan girişin üst satırları yerel kimlik taşımadığı için ⌘K'de
  gidiyordu ve ⌘K kabuğa bayt göndermediği için uzak kabuk onları yeniden
  çizmezdi. `row_block` iki ad alanını okuyor (bekçisi
  `clear_to_start_keeps_every_row_of_a_wrapped_remote_input`); bastırma
  yalnız yerel giriş satırını bastırıyor (`anchor_row_at_or_above` →
  `block_id`, karşılaştırma `BlockKey::Local(input.block)`).
- **Sayacın saati iki canlı bloktan yakın olanı** (`shell::sooner`;
  `/code-review` bulgusu): `ssh` satırının sayacı ile altındaki uzak blok
  aynı anda canlı ve saniye fazları farklı; atama sonrakini kazandırıp
  ötekini bir saniyeye kadar geç adımlatırdı.

## Checklist

- [x] `BlockTrack` tipi ve iki örnek
- [x] `apply`'de erken uzak ayrım
- [x] `rblock` ad alanı ve okuyucular
- [x] Uzak betiklerde 133
- [x] `P`'nin taşınması
- [x] `CLAUDE.md` cümlesi
- [x] Test: yukarıdaki `bt-core` senaryoları, PTY simülasyonu
- [x] Doğrulama geçti (`make check` + `make test-race` + `make bundle` + `make linux`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (9 bulgu:
  aynı `P`'li iki oturum → pid'li kimlik; iki canlı sayacın saati →
  `sooner`; ⌘K'nin uzak girişi → iki ad alanı; boş `--block` → tırnak +
  ebeveynsiz sarma; `running_of` → `RunningBlocks::is`; `parse_mark`'ın
  çift kuruluşu; `running_blocks`'un `then().flatten()`'ı;
  `BlockLog::clear`; `CLAUDE.md` paragrafı kısaltıldı)
