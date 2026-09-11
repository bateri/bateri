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

- [ ] **Teşhis:** H1/H2/H3 ayırt edildi, mekanizma `## Uygulama Notları`'na tek paragraf yazıldı (ölçümle, koda bakarak varsayarak değil)
- [ ] Seçilen yol (A ya da B) ve **neden öteki değil** — gerekçe notlarda
- [ ] `shutdown()` sınırlı sürede dönüyor; süre bir `const` ve gerekçesi yorumda (kısa/uzun olmanın bedeli)
- [ ] `shutdown()`'ın doc'u gerçek mekanizmayı anlatıyor; `trap '' HUP`'ı tek sebep gibi sunan cümle düzeltildi
- [ ] `CLAUDE.md`'nin kapanış maddesi güncellendi — borç kapandıysa kalktı, daraldıysa daraltıldı, **silinip geçilmedi**
- [ ] Test: `shutdown_returns_within_limit` — HUP'ı yutan çocukla (`trap '' HUP`) `shutdown()` sınırı aşmıyor
- [ ] Test: `shutdown_with_busy_writer` — kapanış anında PTY'ye yazan çocukla asılmıyor (bu setin gerçek üreticisi)
- [ ] **Ölçüm:** `BT_SCROLL_TEST=1` en az 8 koşu, asılma oranı **0/8** olmalı; sayı notlara yazılır (öncesi 5/8)
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` **zorunlu** + `make duman` **iki yükte**)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
