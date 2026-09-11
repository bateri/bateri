# Phase 2b — Kapanışın sınırlı beklemesi

## Özet

Ölçüm yükünün kapanışı koşuların ~%62'sinde asılıyor ve bu, setin bütün
ürününü güvenilmez kılıyor. Önce mekanizma **teşhis edilir**, sonra
`bt-core`'a sınırlı bekleme girer ve `shutdown()`'ın kodla çelişen doc'u
düzeltilir.

_Requirements: R8, R8.1, R8.2, R8.3_

---

## 0. Neden ayrı bir phase ve neden burada

Bu kalem phase-2'nin implementer'ı tarafından waive edilip phase-3'e
devredilmişti; orkestratör devri **reddetti**. Phase-3 rapor ve belge
phase'i, kök neden ise `bt-core`'un kapanış sırası — oraya `bt-core` davranış
değişikliği sığmaz.

Sırası da bilinçli: phase-3 `## Yöntem`'in dürüst sınırlarını yazacak. Asılma
burada kapanırsa o belge "asılan koşu atılır" demek zorunda kalmaz.

`phase-2b` adı `duzen.md`'nin kuralına uyuyor: doğal sıralamada
`phase-2 < phase-2b < phase-3`.

---

## 1. Teşhis — **tahminle düzeltme yasak**

`shutdown()`'ın bugünkü doc'u (`bt-core/src/session.rs:815-819`) asılmayı
**sinyali yutan bir çocukla** açıklıyor: *"`Pty::drop` `SIGHUP`'tan sonra
`child.wait()` çağırıyor; sinyali yutan bir çocuk (`trap '' HUP`) bu çağrıyı
süresiz bekletir."*

**Bizim üreticimiz HUP'ı yutmuyor.** `load_shell` düz bir `sh -c` ve içinde
`while` döngüsü var; `trap` yok. Yani kodun kendi açıklaması bu asılmayı
**kapsamıyor** ve doc ya eksik ya yanlış. Düzeltme yazılmadan önce mekanizma
bilinmeli.

Elde olan deneysel olgular:

| olgu | değer |
|---|---|
| `BT_SCROLL_TEST=1 BT_RUN_SECONDS=2` | 8 koşuda 5 asıldı (`exit 70`) |
| `BT_RUN_SECONDS=2` (smoke) | 4/4 temiz |
| `BT_FRAME_STATS` **kapalı** | asılma yine oluyor → phase-2'nin kusuru değil |
| asılan koşuda jeton satırı | **hiç çıkmıyor** (bekçi `_exit(70)`, `report_and_exit`'e varılmıyor) |

İki yükün farkı tek şey: smoke bir kez basıp uyuyor, load kapanış anında
**PTY'ye yazmakta**. Ayırt edilecek hipotezler:

- **H1 — dolu PTY tamponu.** Okuyucu thread `join` ile bitti, master'ı kimse
  okumuyor, çocuk `write()`'ta bloke. `SIGHUP`'ın varsayılan eylemi sonlandırma
  ve bloke `write()` sinyalle kesilebilir; **eğer** H1 tek başına yetiyorsa
  asılma olmamalıydı. Demek ki tek başına açıklama değil — ama katkısı ölçülmeli.
- **H2 — sinyalin hedefi.** `SIGHUP` çocuğun pid'ine mi, PTY'nin ön plan
  süreç grubuna mı gidiyor? `sh -c` bir alt süreç doğurmuyorsa ikisi aynı, ama
  `printf` builtin olmayan bir kabukta çatallanma olabilir.
- **H3 — `Pty::drop` içindeki sıra.** alacritty **0.26.0**; `Pty::drop`'un
  master fd'yi kapatma ile `child.wait()` sırası ne? Master `wait`'ten **önce**
  kapanırsa çocuğun yazısı `EPIPE`/`SIGPIPE` ile biter; **sonra** kapanırsa
  çocuk bloke kalır ve `wait` dönmez.

**Bunu ölçerek ayır**, koda bakarak varsayma:

```sh
# Asılan bir koşuyu yakala ve çocuğun gerçekten ne beklediğini gör.
BT_SCROLL_TEST=1 BT_RUN_SECONDS=2 ./target/debug/bateri & pid=$!
sleep 3; ps -o pid,stat,wchan,command -g $(ps -o pgid= -p $pid | tr -d ' ')
sample $pid 2 -f /tmp/hang.txt   # ana thread hangi çağrıda?
```

Bulduğun mekanizmayı `## Uygulama Notları`'na **tek paragraf** olarak yaz.
Yanlış teşhis üstüne kurulan düzeltme, belirtiyi başka bir yere taşır.

---

## 2. Sınırlı bekleme — `CLAUDE.md`'nin yazdığı çözüm

`crates/bt-core/src/session.rs`

`CLAUDE.md` kalıcı çözümü zaten adıyla söylüyor: **`bt-core`'da sınırlı
bekleme (`SIGHUP` → süre → `SIGKILL`)**. Bu phase onu hayata geçiriyor.

Zincirin bugünkü hâli (`session.rs:820`):

```
send(Msg::Shutdown) → reader.join() → dönen (EventLoop, State) DÜŞER
                                       └─ Pty::drop → SIGHUP → child.wait()  ← burada bloke
```

Bloke eden çağrı **alacritty'nin `Pty::drop`'u** ve onu değiştiremeyiz. Yani
çözüm "wait'e timeout eklemek" değil; düşmeyi kontrol altına almak. İki
gerçekçi yol var, **hangisi seçilirse gerekçesi yazılır**:

- **(A) Düşmeyi ayrı thread'e al, sınırlı bekle.** Tuple'ı bir thread'e taşı,
  `recv_timeout` ile bekle; süre dolarsa ana thread devam eder. Çocuk hâlâ
  yaşıyorsa süreç çıkışında zaten reap edilir. **Riski:** `Pty` `Send` mi?
  Değilse bu yol kapanır — **önce doğrula**.
- **(B) Düşmeden önce çocuğu kesinle.** Teşhis H1/H3'ü gösteriyorsa master'ı
  drain etmek ya da çocuğa `SIGKILL` göndermek `wait`'i serbest bırakır.
  alacritty 0.26'nın yüzeyinde çocuğa erişim var mı — `EventedPty`,
  `ChildEvent`, `next_child_event` — **önce doğrula**, uydurma.

Hangisi olursa olsun **sözleşme aynı**: `shutdown()` sınırlı sürede döner.
Süre bir `const` ve gerekçesi yorumda: neden o kadar, kısa olursa ne kaybolur
(çocuğun düzgün kapanma şansı), uzun olursa ne (kullanıcı donmuş sanır).

> **Bekçiyle karıştırma.** `bt-shell`'in bekçisi (`lib.rs:79-87`) yalnız
> `BT_RUN_SECONDS` yolunda kurulur ve `_exit(70)` ile **süreci** keser.
> Buradaki sınırlı bekleme `bt-core`'da ve **her** yolda çalışır — asıl
> kazancı da o: `CLAUDE.md`'ye göre etkileşimli kullanımda bugün "kesen yok",
> yani Cmd-Q'da donan bir terminal mümkün.

---

## 3. Doc düzeltmesi — kodla çelişen cümle

`shutdown()`'ın doc'u `trap '' HUP` senaryosunu tek sebep gibi anlatıyor;
üreticimiz onu çürüttü. Teşhis sonrası doc **gerçek mekanizmayı** anlatır ve
yeni sınırı söyler: artık sınırlı bekleniyor, süre dolunca ne oluyor.

`CLAUDE.md`'nin "Kapanış bloklar ve bunun bir sınırı var" maddesi de aynı
commit'te güncellenir — borç kapanıyorsa cümle kalkar, daralıyorsa daraltılır.
**Silinip sessizce geçilmez.**

---

## Uygulama Notları

- **Teşhis (ölçüldü, bu makine, debug).** Asılan bir koşuda ana thread
  `Pty::drop → std::process::Child::wait → __wait4` içinde duruyor (`sample`,
  1665/1665 örnek) ve **aynı** koşuda çocuk `?Es` durumunda: çıkışa girmiş
  ama bitmemiş bir oturum lideri. Ayırt edilenler: çocuğun **alt süreci yok**
  (`pgrep -P` boş, `date` ikamesi o anda koşmuyor) → **H2 düştü**, sinyalin
  hedefi tek süreç. Çocuğa `SIGKILL` gönderildi, durumu **değişmedi** (`?Es`
  kaldı) ve ancak *bizim* süreç `_exit(70)` ile ölüp master fd kapandığında
  gitti → bloke bir `write()` değil (onu `SIGHUP` keserdi, kesildiğini ayrı
  bir `pty` üreticisiyle de gördüm: master okunmazken `SIGHUP` alan yazıcı
  ~0,6 sn'de bitiyor, master kapatılır ya da boşaltılırsa ~0 ms), yani **saf
  H1 de düştü**. Kalan **H3**: `Pty::drop` `SIGHUP` → `child.wait()` sırasını
  koşuyor, master fd ise `Pty`'nin bir **alanı**, yani `Drop` gövdesinden
  *sonra* kapanıyor; okuyucu thread de `join` ile bittiği için kuyruğu artık
  kimse boşaltmıyor. Çocuk çıkışını bitirmek için kuyruğun boşalmasını
  bekliyor, kuyruk bizim elimizde, biz de çocuğu bekliyoruz — kilitlenme.
  H1'in "dolu tampon"u tetik, H3'ün sırası kilit. **Ne ölçmedim:** çekirdeğin
  çocuğu tam olarak hangi noktada beklettiği (`?Es`'in içi); adını
  varsaymadım, dışarıdan gözlenen bağımlılık zincirini yazdım. Aralıklılık da
  **gözlem**: bugün 2/10 ve 4/8 (kuyruğun kapanış anında dolu olup olmamasına
  bağlı), orkestratörün 5/8'i örtülü pencerede. Düzeltmenin doğruluğu bu
  belirsizliğe bağlı **değil**: sınırlı bekleme `wait4`'ün *neden*
  bloklandığını sormuyor.

- **Yol (A) seçildi: `join` + düşme ayrı thread'de, ana thread sınırlı
  bekliyor.** `Send` doğrulandı, varsayılmadı: `EventLoop::spawn`'ın
  `T: Send + 'static` sınırı `Pty`'yi zaten `Send` yapıyor ve `JoinHandle<T>`
  koşulsuz `Send` — tutamak taşınabiliyor. Kılavuzun §1'inden bir sapma:
  `join` **de** taşındı, yalnız düşme değil. Sebebi ölçülmüş mekanizmanın
  ikinci yarısı: okuyucu thread panikle sarılırken `Pty::drop` o thread'de
  koşar ve `join`'in kendisi de bloklayabilir; ikisi aynı thread'e konunca
  sözleşme ("sınırlı sürede döner") tek yerden garanti ediliyor.
- **Yol (B) ölçümle kapandı**, tasarımla değil: (a) `?Es` durumundaki çocuk
  `SIGKILL` almıyor — "`SIGHUP` → süre → `SIGKILL`" zinciri bu üreticide
  **işe yaramaz**, ölçüldü; (b) çocuğu o duruma hiç sokmamak master'ı `wait`
  bloklarken boşaltmak demek, ama alacritty **0.26.0**'ın yüzeyi buna izin
  vermiyor: `EventLoop`'un alanları private ve `pty` için erişimci yok
  (`pub fn` yalnız `new`, `channel`, `spawn`), `Pty` de `child()`/`file()`
  ile yalnız `&Child`/`&File` veriyor — `EventedPty`/`ChildEvent`/
  `next_child_event` **var** ama okuyucu döngüsünün içinde, `join`'den sonra
  ulaşılmaz. (c) Master'ın bir kopyası `Session::spawn`'da
  `pty.file().try_clone()` ile alınabilirdi, ama o yol da HUP'ı yutan çocuğu
  sınırlayamaz: sınır her hâlde gerekiyordu ve sınır tek başına iki kusuru
  birden kapatıyor.
- **Sınır çocuğu iyileştirmiyor, kapanışı sınırlıyor.** Süre dolan koşuda
  çocuk çıkışın içinde kalıyor ve süreç çıkışı topluyor; kanıt koşusunda
  bu **2/8**. Kalıcı çare (master'ı boşaltmak) phase-3'ün `## Yöntem` notuna
  ve `CLAUDE.md`'ye borç olarak yazıldı — `try_clone()` yeni bağımlılık
  istemiyor, yani kapı açık.
- **`SHUTDOWN_GRACE = 500 ms` ölçümden.** Geçici bir enstrümantasyonla sekiz
  Load koşusunda kapanış süresi okundu: **0,44–0,70 ms** (dört koşu) ya da
  **hiç bitmiyor** (dört koşu). Arası yok, yani sabit "düzgün kapanışa yetsin"
  diye değil "kullanıcı ne kadar bekler" diye seçildi; gerekçenin iki yarısı
  (kısa/uzun olmanın bedeli) `const`'un doc'unda. Enstrümantasyon ölçümden
  sonra **söküldü** — kapanış yolunda kalıcı `Instant::now()` yok, yalnız
  sınır dolduğunda bir stderr satırı var.
- **Thread kurulamazsa** (OS thread sınırı) eski davranışa dönülüyor ve bu
  **söyleniyor**: tutamak o dalda yerinde düşer, yani `Pty::drop` ana
  thread'de koşabilir. Alternatifi tutamağı `mem::forget` etmekti; alınmadı,
  çünkü `Pty`'yi (master fd + çocuk) ve `Term`'ü kalıcı olarak sızdırmak
  ihtimali OOM dalında bir blokajdan daha kötü — ve o dalda panik yasağı
  gereği `expect` de yok.
- **Doc ve belge aynı commit'te.** `shutdown()`'ın doc'u iki sebebi de
  sayıyor (`trap '' HUP` **ve** çıkışta takılma) ve artık sınırı söylüyor;
  `CLAUDE.md`'nin kapanış maddesi **daraltıldı**, silinmedi — "kesen yok"
  kalktı, yerine kalan borç (arkada kalan çocuk) ve çürütülen çare
  (`SIGKILL`) yazıldı. Kılavuzun istediği `.tasks/002-vt-motoru/phase-4.md`
  atfı `shutdown()`'ın doc'undan düştü: o atıf "bilinen sınır" cümlesinin
  dayanağıydı ve sınır artık orada değil — `app.rs:144`'teki atıf (kapanış
  sırasının tasarımı) yerinde kaldı. Bekçinin iki doc'u (`lib.rs::watchdog`,
  `app.rs::shutdown`) kapsam daralmasını yazıyor: bekçi artık bu adımın değil
  kapanış yolunun geri kalanının bekçisi.
- **Kanıt (Load, 8 koşu, debug, bu makine): asılma 0/8** (öncesi 5/8), jeton
  satırı sekizde sekiz basıldı (`kare≈226–235`), sınır 2/8 koşuda doldu.
  Ayrıca phase-3'ün bekleyen kalemi düştü: **`BT_RUN_SECONDS=5` artık
  koşulabiliyor** — dört koşu, dördü temiz, `kare≈592–595` (R5.6'nın ölçüm
  koşusu phase-2'de bu süreyle imkânsızdı, devir satırı `phase-3.md`'de
  güncellendi).

- **`/simplify` dört mercek koştu, dördü döndü; dört şey değişti.**
  1. **Üst sınır iddiası tek yere indi** (reuse + simplification, ikisi de
     aynı beş satırı gösterdi): iki sınamada birebir kopya olan "ölç,
     `shutdown()` çağır, sınırı aşmadığını doğrula" bloğu
     `shutdown_within_grace()` yardımcısına çıktı — modülün kendi deseni
     (`wait_cells` → `wait_frame`) ile aynı.
  2. **Yardımcı süreyi döndürüyor** ve `shutdown_returns_within_limit`
     buna bir **alt sınır** ekliyor (`elapsed >= SHUTDOWN_GRACE`). Bu
     kılavuzda yoktu, efficiency merceğinin "bu sınamalar her koşuda tam
     500 ms yakıyor" notundan çıktı: sınırı gerçekten dolduran bir sınamada
     erken dönen bir `shutdown` **boşuna yeşil** kalırdı (çocuk erken
     ölmüştür), üst sınır onu göremez.
  3. **Ortak dal düzleşti** (simplification): `match teardown` yerine
     erken dönen bir `if let Err`. Normal kapanış artık bir girinti daha
     sığ; nadir dal (OS thread sınırı) yukarıda duruyor.
  4. **`Err` dalının yorumu düzeltildi** (altitude, ölçerek): "eski
     davranışa dönüyoruz" **yanlıştı**. Tutamak `spawn` başarısız olurken
     closure'la düşer ve `JoinHandle::drop` detach eder, yani çift ya
     okuyucu thread'in bitişinde düşer (kimse bloklanmaz, `SIGHUP` +
     `child.wait()` sınırsız koşar) ya da — okuyucu thread çoktan bitmişse
     — orada düşer ve bu thread'i bloklar. Yorum artık iki sonucu da
     söylüyor ve stderr satırı "bloklayabilir" değil "sınırsız" diyor.

- **`/code-review` iki koşucuyla koştu** (Skill fork'u phase-1 ve phase-2'de
  olduğu gibi gecikti, `proje.md`'nin iniş sırası basamak 2: `code-reviewer`
  subagent'ı; sonra fork da döndü ve ikisi de rapor verdi). **Çalışma zamanı
  hatası yok** — sınır sözleşmesi ikisinde de ayakta, `drop(tail)`'in
  `send`'den önce geldiği bağımsız doğrulandı. Yedi bulgu uygulandı:
  1. **Tanı artık yalan söyleyemiyor** (ikisi de buldu, MEDIUM):
     `recv_timeout(...).is_err()` `Timeout` ile `Disconnected`'ı katlıyordu.
     `Disconnected` = kapanış thread'i **panikledi** ve çağrı *hemen* dönüyor;
     eski satır "500 ms bekledim, arkada bıraktım" diyerek hem süreyi hem
     sebebi yanlış söylüyor, üstelik kapanış yolundaki bir paniği yutuyordu.
     Artık iki ayrı satır.
  2. **Yeni bir tehlike doğdu ve hiçbir yerde yazmıyordu** (HIGH): sınır
     dolduğunda `(EventLoop, State)` çifti `"PTY teardown"` thread'inde
     kalıyor ve o çift `Adapter` üzerinden `Arc<dyn Wake>` taşıyor — son kopya
     oraya düşerse **`Wake::drop` o thread'de koşar**. Bugünkü tek uygulayan
     `bt-gpu`'nun `Waker`'ı ve içinde ana kuyruğa senkron iş atan bir alan var
     (`MainThreadBound<Retained<CAMetalDisplayLink>>`). Bugün patlamıyor
     çünkü `bt-shell`'in `Ivars`'ı referansı süreç sonuna kadar tutuyor; o
     yük taşıyıcı gerçek `bt-shell`'de, tehlike ise artık `bt-core`'da. Kural
     `wake.rs`'in Sahiplik paragrafına yazıldı: uygulayanın `Drop`'u da
     bloklamaz.
  3. **Üç kodla çelişen cümle düzeltildi** (HIGH, hepsi diff dışıydı ama aynı
     mekanizmayı anlatıyordu): `wake.rs`'in "`EDEADLK` ve `Drop` içinde
     panik" cümlesi artık **yanlıştı** (sınır paniği yarım saniyelik bir
     durmaya çevirdi — yasak duruyor, bedeli değişti), `bt-gpu/link.rs`'in
     "`join()` o thread'de koşar"ı ve `app.rs`'in `child_exit` yorumundaki
     "kendi kendine `join`"i aynı biçimde.
  4. **Tehlikeli thread listesi uzadı**: `app.rs`'in "son referans hiçbir
     zaman Metal'in thread'inde olmaz" sayımı eksikti, kapanış thread'i
     eklendi.
  5. **Sınırın istisnası üç yerde yazılıydı gibi duruyordu ama yazılı
     değildi** (MEDIUM): `CLAUDE.md`, bekçinin doc'u ve `app.rs`'in kapanış
     sırası "artık asamaz / en çok yarım saniye" diyordu, oysa thread
     kurulamayan dalda sınır yok. Üçü de kayıtlandı.
  6. **`shutdown()`'ın doc'u iki şeyi vaat etmiyor** (LOW): süre dolduğunda
     `SIGHUP`'ın gittiği garanti değil (yavaş adım `join` ise `Pty::drop`
     hiç başlamamıştır) ve sınır her yolda yok.
  7. **Sınamanın artığı yarıya indi** (LOW): `trap ''` sinyali `SIG_IGN`
     yapıyor ve `sleep` onu **miras alıyor**, yani çocuk süreç çıkışında bile
     ölmüyor — `sleep 10` → `sleep 5` (sınırın on katı; alt sınır erken ölen
     çocuğu kırmızıya çevirdiği için pay güvenle kısaldı). İncelemeci
     `cargo test` sonrası pid'i `ps`'te gösterdi.

  **Kapsam dışı bırakılanlar (phase-3'ün checklist'ine yazıldı, gerekçe:
  `bt-core` kapanışı değil rapor/ölçüm yolları):** `IDLE_FRAME_LIMIT`'in
  dayandığı "tavan ~3 kare" ölçümünün **çürütülmesi** (fork ölçtü: 3
  saniyede `kare=352`; ben 2 saniyede 232, 5 saniyede 593 ölçtüm — yani
  kapının ölçüsü artık yanlış bir sayıdan geliyor ve meşru bir uyandırma
  kapıyı kırmızıya düşürebilir), `kapanis=` jetonu (sınır dolan koşu bugün
  yeşil bir satırla geçiyor; jeton sözleşmesi "eklenir" diyor),
  `record_gpu`'nun elenen örneği saymaması, `completion_hands_over_live_gpu_timestamps`'ın
  donanım yeteneğini sert bir kapıya çevirmesi, `verdict`'in dört konumsal
  sayacı, `Workload → yuk=` eşlemesinin çağrı yerinde durması, `Ring`in
  hizalamasının yığın dizilerini kapsamaması ve `Stats`'ın okuyucusunun
  henüz olmaması (plan gereği phase-3). Bekçi bütçesinin (`run_seconds × 3`)
  artık koruduğu şeyle ilgisiz olması da phase-3'e: sayı ölçülmeden
  değişmiyor, yorumu bugün durumu söylüyor.

- **`/audit` on merceği eledi ve koştu; yeni bulgu çıkmadı.** İlgili çıkanlar
  inline koşuldu (kural: ikiden çok ilgili yargı merceği varsa fan-out; burada
  iki tane).
  - **1 (katman/platformsuzluk): temiz.** `cargo tree -p bt-core` ve
    `-p bt-gpu` boş; `crates/bt-core/src` içinde `objc2|core_text|
    core_graphics` yok. `bt-core`'a giren tek şey `std::thread`,
    `std::sync::mpsc`, `std::time::Duration`.
  - **2 (yeni bağımlılık): temiz.** `Cargo.toml` ve `Cargo.lock` diff'te yok.
  - **3 (panik yolu): temiz.** `bt-core`'un eklenen satırlarında
    `unwrap/expect/panic!/indeksleme` yok; gönderim `let _ = done.send(())`,
    kanal hatası `match` ile karşılanıyor, iki dal da stderr'e yazıyor.
  - **6 (ölçüm sahipliği): temiz, bir nüansla.** `CLAUDE.md`'ye sayı girmedi
    (yalnız `const`'un adı ve niteliksel "yarım saniye"); ölçülen sayılar
    `SHUTDOWN_GRACE`'in doc'unda (sabiti gerekçelendiriyorlar, phase-2'nin
    deseni) ve bu phase dosyasında. `docs/OLCUMLER.md` bu sette **bilerek
    yok** (R7.3: ilk `/measure` kurar), yani sayının gidebileceği başka bir
    sahip yok. Ölçülmemiş iddia yok: "asamaz" cümlesinin arkasında 0/8 var.
  - **7 (thread ve blokaj): bulgu `/code-review` turunda kapandı.** Render
    yoluna bloklayan çağrı **girmedi** — eklenen bekleme kapanışta ve
    `DisplayLink::stop`'tan *sonra*. Yeni kilit sırası yok: kapanış thread'i
    `Term` kilidini almıyor, yalnız `Arc`'ları düşürüyor. Gerçek tehlike
    `Wake::drop`'un o thread'e düşebilmesiydi ve bu tura girmeden
    belgelendi (`wake.rs` Sahiplik, `app.rs` kapanış sırası).
  - **10 (belge ve üslup): temiz.** Eklenen tanımlayıcıların tamamı İngilizce
    (`SHUTDOWN_GRACE`, `shutdown_within_grace`, `teardown`, `tail`, thread adı
    `"PTY teardown"`); yorumlar Türkçe ve "neden" anlatıyor; Türkçe kalan
    dizgiler yalnız tanı çıktısı ve `assert!` gerekçeleri; yeni `#[allow]`
    yok.
  - **İlgisiz (bakılmadı, sebebiyle): 4** (`settings.rs`/tema el değmedi),
    **5** (`assets/shell/` el değmedi), **9** (`.metal` yok, `Cell` yapısı
    değişmedi — `bt-gpu`'da yalnız bir doc satırı). **8** de dar anlamda
    ilgisiz (diff animasyon/zamanlayıcı eklemiyor), ama merceğin koruduğu
    kapının kendisi `/code-review`'da çürük çıktı ve phase-3'e devredildi.

  **Uygulanmayanlar:** (a) efficiency'nin "iki sınama `cargo test`'e ~1 sn
  ekliyor, `make test-yaris` tek thread'de bunu bir kez daha ödüyor" notu —
  sınanan şeyin kendisi `SHUTDOWN_GRACE`, süre kısaltılamaz; kayda geçti,
  koda dokunulmadı. (b) `sleep 10`'u kısaltmak: çapa (`wait_cells`) en kötü
  hâlde 5 saniyeye kadar bekleyebilir ve alt sınır eklendiği için erken ölen
  bir çocuk artık **kırmızı** düşer — payı korumak o kırmızının yanlış
  pozitif olmasını engelliyor. (c) Thread adının `String` ayırması (`"PTY
  teardown".to_owned()`): oturum başına bir kez, tanı değeri ayırmadan büyük.

- **sadakat: makas yok.** `git show --stat e991d78` checklist'le
  karşılaştırıldı: sekiz dosyanın her biri bir maddeyle eşleşiyor
  (`wake.rs`/`link.rs` sapma (c)'nin mekanizma cümleleri, `CLAUDE.md` madde 3,
  `app.rs`/`lib.rs` bekçi ve kapanış bağlantısı), `plan.md` damga commit'inde
  (`9630c5d`). `Cargo.lock` oynamadı.

- **Orkestratör doğrulaması ve kendi hatasının kaydı.** Düzeltmeyi bağımsız
  ölçtüm: load 2 sn → `kare=230–234` (beş koşu), load 5 sn → `kare=594`,
  smoke 5 sn → `kare=1`. Yani boşta sıfır kare **kusursuz** çalışıyor ve yük
  altında link tam hızda.

  Bu, phase-2 sonrası **benim** yaptığım çıkarımı çürütüyor: `kare=3` ölçüp
  "pencere örtülü, sistem display link'i askıya alıyor, tavan 3" demiştim.
  Örtülülük gerçek (phase-1 `occlusionState`'i okudu) ama sebep o değilmiş;
  düşük sayı kapanış kilitlenmesinin kök nedeninden geliyormuş — master
  boşaltılmayınca kuyruk doluyor, okuyucu tıkanıyor, kirli satır düşmüyor.
  **Bozuk bir koşuyu ölçüp tavan sanmışım; korelasyonu nedensellik yapmışım.**
  `plan.md → R5.6`'nın gerekçesi, `phase-3`'ün checklist maddesi ve
  `phase-1`'deki notum buna göre düzeltildi. Gereksinimin kendisi ayakta:
  dayanağı bu ölçüm değil, "az örnek üstünden p95 anlamsızdır" ilkesi.

  Sonuç olarak `IDLE_FRAME_LIMIT=2`'nin **değeri** hâlâ doğru ama **payı**
  sanılandan çok büyük: sağlam smoke koşusu 5 saniyede bile `kare=1`, bozulmuş
  bir boşta yolu ise yüzlerce kare üretirdi. Phase-3'ün devraldığı
  "dayanağı çürüdü" maddesi bunu yeniden ölçüp yazacak.

## Yayın Etkisi

- **Ölçüm bekliyor: yok.** Bu phase bir kusuru kapatıyor; kapanış süresi bir
  kare/gecikme iddiası değil, sınırın kendisi `const`.
- **`make test-yaris` zorunlu.** Kapanış sırası ve okuyucu thread'in ömrü tam
  bu sınamanın konusu; `race_*` stresi ve tek-thread karşılaştırma koşusu
  ikisi de geçmeli.
- **`make duman` zorunlu** ve iki yükte birden: `Smoke` (jeton satırı bit bit
  aynı) **ve** `Load` (asılma oranı ölçülür — düzeltmenin kanıtı bu).
- **Belge:** `CLAUDE.md`'nin kapanış maddesi ve `session.rs`'in doc'u.
  `.tasks/002-vt-motoru/phase-4.md`'ye atıf yapan cümle de gözden geçirilir.
- Yeni bağımlılık **yok** beklentisi — `libc` zaten `bt-core`'da serbest
  (`CLAUDE.md`: "Unix PTY (`libc`, `rustix`) serbest"). `Cargo.lock` oynarsa
  **eskalasyon**.
- `.metal`, `build.rs`, `assets/`, ayar şeması, tema: el değmiyor.

---

## Checklist

- [x] **Teşhis:** H1/H2/H3 ayırt edildi, mekanizma `## Uygulama Notları`'na tek paragraf yazıldı (ölçümle, koda bakarak varsayarak değil)
- [x] Seçilen yol (A ya da B) ve **neden öteki değil** — gerekçe notlarda
- [x] `shutdown()` sınırlı sürede dönüyor; süre bir `const` ve gerekçesi yorumda (kısa/uzun olmanın bedeli)
- [x] `shutdown()`'ın doc'u gerçek mekanizmayı anlatıyor; `trap '' HUP`'ı tek sebep gibi sunan cümle düzeltildi
- [x] `CLAUDE.md`'nin kapanış maddesi güncellendi — borç kapandıysa kalktı, daraldıysa daraltıldı, **silinip geçilmedi**
- [x] Test: `shutdown_returns_within_limit` — HUP'ı yutan çocukla (`trap '' HUP`) `shutdown()` sınırı aşmıyor
- [x] Test: `shutdown_with_busy_writer` — kapanış anında PTY'ye yazan çocukla asılmıyor (bu setin gerçek üreticisi)
- [x] **Ölçüm:** `BT_SCROLL_TEST=1` en az 8 koşu, asılma oranı **0/8** olmalı; sayı notlara yazılır (öncesi 5/8) — **0/8**, sınır 2/8 koşuda doldu
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` **zorunlu** + `make duman` **iki yükte**) — kapıdan sonra yeniden koşuldu: üçü de çıkış 0, Smoke `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke pipeline=ok` (bit bit aynı), Load 8/8 jeton satırı
- [x] `/simplify` çalıştırıldı, bulgular uygulandı — dört mercek, dördü döndü; dört bulgu uygulandı, üçü gerekçeyle geçildi (notlarda)
- [x] `/code-review` çalıştırıldı, bulgular giderildi — Skill fork'u gecikti, `proje.md` basamak 2 (`code-reviewer` subagent) koştu, sonra fork da döndü; yedi bulgu uygulandı, ölçüm yolundaki dokuz bulgu phase-3'e devredildi (notlarda)
- [x] `/audit` çalıştırıldı, bulgular giderildi — on mercek elendi, ilgili altısı koştu (1/2/3/6/7/10 temiz), dördü ilgisiz; yeni bulgu yok (notlarda)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: `e991d78`
