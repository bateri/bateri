# Phase 3 — Rapor: jetonlar ve belge uyumu

## Özet

p95 ile en kötü değer süreç içinde hesaplanır, jeton satırı genişler
(`ornek=`, `profil=`, CPU/GPU/açılış), ve belgelerdeki kanca adları kodla
uyumlanır. Bu phase'ten sonra `/measure` gerçek sayı okuyabilir.

_Requirements: R5, R5.1, R5.2, R5.3, R5.4, R5.5, R7, R7.1, R7.2, R7.3_

---

## 1. İstatistik süreç içinde — dosya **yok**

`crates/bt-gpu/src/` (halkanın yanında)

Dosya kararı üç koldan çürüdü ve gerekçesi `discussion.md` Karar 4'te; en
somutu şu: `/measure`'ın `allowed-tools`'unda `awk`, `python`, `cut` yok —
yalnız `sort` ve `wc`. Bir TSV'den p95 hesaplamak o araç setiyle eziyet, tek
jeton satırı ise bir `Read`.

```rust
/// p95 ve en kötü. Ortalama **yok**: bir takılma ortalamayı oynatmaz ama
/// kullanıcı onu görür (`/measure` → "dağılım, ortalama değil").
///
/// Örnek sayısı azsa p95 anlamsızdır ama **sessiz** de olmamalı: sayı
/// basılır, yanında `ornek=` gider, yorumu `/measure` yapar.
fn p95_and_worst(halka: &[Duration]) -> Option<(Duration, Duration)>
```

---

## 2. Jeton satırı genişler

`crates/bt-shell/src/app.rs` → `report_and_exit`

Sözleşme aynen: **silinmez, eklenir.** Mevcut beş jeton (`kare`, `hucre`,
`glif`, `kural`, `yuva`) ve `pipeline=ok` yerinde kalır.

```
kare=1 hucre=8 glif=6 kural=15 yuva=21/256 ornek=0 profil=debug pipeline=ok
kare=412 hucre=118 glif=3944 kural=0 yuva=97/256 ornek=412 profil=release \
  cpu_kare_p95=1.8ms cpu_kare_max=4.1ms cpu_encode_p95=0.3ms \
  gpu_p95=2.2ms gpu_max=5.0ms acilis=284ms pipeline=ok
```

Üç jeton yük taşıyor:

- **`ornek=`** — örnekleme **sessizce durabilir**: pencere örtülünce kapı
  kapanıyor (`link.rs:262`, `app.rs:271-281`) ve üç örnek üstünden hesaplanan
  p95 *iyi* görünür. Bundle'sız `cargo run` penceresi tam da onu başlatan
  terminalin arkasına düşüyor, yani bu teorik bir risk değil. `/measure` sayı
  yorumlamadan **önce** buna bakar.
- **`profil=`** — `make duman` **debug** koşuyor, `/measure` **release** şart
  koşuyor. Belgeye güvenmek yerine sayının kendisi profilini söylüyor; debug
  sayısını taban sanmak imkânsız oluyor.
- **`acilis=`** — tek sayı, dağılım değil: açılış koşu başına bir kez olur.

> **`Drop`'a güvenilmez (R5.5).** `report_and_exit` `process::exit` ile
> çıkıyor ve o `Drop` koşturmuyor; bekçinin `_exit(70)`'i atexit zincirini
> bile atlıyor (`lib.rs:79-87`). Jetonlar `println!` ile **açıkça** basılır,
> hiçbir tampon `Drop`'ta boşalmaya bırakılmaz.
>
> **Sıra korunuyor:** `report_and_exit` `shutdown()`'dan **sonra** çağrılıyor
> ve bu bilinçli (`app.rs:411-415`) — asılan bir kapanışta `kare=` satırı hiç
> çıkmasın diye. Yan faydası bize: `shutdown()` içinde `link.stop()` koştuğu
> için rapor anında halka **durağan**. Uçuştaki son bir iki tamamlanma bloğu
> kaçabilir; `ornek=` bunu görünür kılar.

---

## 3. Belgeler — borç cümleleri **silinmez, yeniden yazılır**

`CLAUDE.md`, `.claude/skills/measure/SKILL.md`,
`.claude/is-akisi/sablonlar/context.md`, `.claude/is-akisi/proje.md`

**R7.2 — kanca adları.** Belgelerde yazan `BT_FRAME_LOG` adı bu tasarımda
**yalan** olurdu: log yok, dosya yok. `BT_FRAME_STATS` olur. `BT_STARTUP_TRACE`
ayrı bayrak değil — açılış aynı bayrağın altında tek sayı. Dört yer güncellenir
(adların bugün geçtiği yerler `context.md`'de listeli).

**R7.1 — bench sözü.** `CLAUDE.md` bugün *"kancalar gelince bu cümle kalkar ve
`cargo bench` satırı yukarıdaki bloğa geri gelir"* diyor. Kancalar geldi ama
bench gelmedi (`discussion.md` Karar 7). Cümle **silinmez**: bu set bir borç
cümlesini başka bir borçla takas ediyor ve bunu gizlemek, tam da bu setin
düzelttiği hatanın kendisi olurdu.

Yeni hâli şunu söylemeli: ölçüm kancaları **var**, `docs/OLCUMLER.md` ilk
`/measure` ile doğacak, `cargo bench` satırı **bench setini** bekliyor ve
bunun bedeli 003 #1 ile #2'nin bench yarısının açık kalması.

**R7.3 — `docs/OLCUMLER.md` bu sette yazılmaz.** `/measure` skill'i zaten
"dosya yoksa ilk ölçüm onu kurar" diyor. Ama phase-2'nin **dürüst sınırı**
(açılış damgası `main()`'in başında, süreç başlangıcında değil) kaybolmamalı:
kodun doc yorumunda durur ve ilk `/measure` onu `## Yöntem`'e taşır.

`.tasks/README.md`: 002, 003, 004 satırları artık "ölçüm aracı yok" demiyor —
"`/measure` koşulabilir; bench'e bağlı iddialar bench setini bekliyor".

---

## Uygulama Notları

## Yayın Etkisi

- **Duman sözleşmesi:** jetonlar **eklendi, silinmedi**. `Smoke` yükünde
  `ornek=0` ve ölçüm jetonları yok (kapı kapalı); `kare=1 hucre=8 glif=6
  kural=15` bit bit aynı kalmalı.
- **Belge:** `CLAUDE.md` (kanca adları + bench borcunun yeniden yazımı +
  `make duman` satırı), `/measure` skill'i, `context.md` şablonu,
  `proje.md`, `.tasks/README.md`.
- **`docs/OLCUMLER.md` yazılmıyor** — ilk `/measure` kuracak.
- **Ölçüm bekliyor: yok.** Bu set araç üretiyor; sayı `/measure`'ın işi ve
  bu phase bittiğinde o komut **koşabilir** hâle geliyor.
- Yeni bağımlılık yok, `.metal`/`build.rs`/`assets` el değmiyor.
- **Kapanan iddialar:** kare süresi iddialarının tamamı, açılış/ölçek
  iddiaları ve atlas doluluğu (phase-1). **Açık kalan:** 003 #1 ve 003 #2'nin
  bench yarısı — kayıtlı ve gerekçeli.

---

## Checklist

- [ ] **phase-1'den devir:** başarı satırı `9788d95` ile iki jeton kazandı — `yuva=U/T` **ve** `yuk=smoke|load`. `plan.md`'nin Akış şeması `yuk=`'ü göstermiyor (phase-1'de, `/code-review` bulgusu üzerine eklendi): oradaki satırı olduğu gibi kopyalayan bir `println!` jetonu **sessizce düşürür** ve sözleşme "silinmez, eklenir" der. Rapor genişlerken ikisi de korunacak; korunduğunu `make duman` çıktısında gözle doğrula
- [ ] `p95_and_worst` — ortalama **yok**; boş/az örnekte davranışı tanımlı
- [ ] Jetonlar `report_and_exit`'te `println!` ile **açıkça** basılıyor; `Drop`'a güvenen yol yok
- [ ] `ornek=` jetonu — düşen örnekler dâhil
- [ ] `profil=` jetonu (`cfg!(debug_assertions)`)
- [ ] `acilis=` tek sayı olarak
- [ ] `CLAUDE.md`: kanca adları, `make duman` satırı, **bench borcunun yeniden yazımı** (silme değil)
- [ ] `/measure` skill'i, `context.md` şablonu, `proje.md` kanca adlarıyla uyumlandı
- [ ] `.tasks/README.md`: 002/003/004 satırları "ölçüm aracı yok" demiyor
- [ ] Phase-2'nin dürüst sınırı (açılış damgası `main()`'den, süreç başından değil) kodun doc'unda yazılı
- [ ] **R5.6 örnek tabanı** — taban altında p95 **basılmaz**, `ornek=` yerine
      gerekçe çıkar. Orkestratör ölçtü: örtülü pencerede `BT_SCROLL_TEST=1
      BT_RUN_SECONDS=5` → `kare=3` (içerik akıyor, `glif=1836`). Sistem örtülü
      pencerede display link'i askıya alıyor; `.app` paketi (006) gelene kadar
      anlamlı ölçüm **öne getirilmiş pencere** istiyor ve bu `/measure`'ın
      `[elle]` adımıdır
- [ ] **phase-2'den devir — okuma API'si.** Halka `bt-gpu`'da ve `bt-shell`'in elinde bir `Arc<Stats>` var (`AppDelegate` ivar'ı, `report_and_exit` oradan okur). Yüzey: `Stats::startup() -> Option<Duration>` ve sütun başına `cpu_frame()` / `cpu_encode()` / `gpu() -> Samples { nanos: Vec<u64>, dropped: u64 }` — değerler **nanosaniye**, eskiden yeniye. p95 bu üç sütunun her birinden ayrı hesaplanır; `Duration` isteyen bir `p95_and_worst` imzası `from_nanos` ile besleniyor
- [ ] **phase-2'den devir — GPU sütunu CPU'dan kısa olabilir.** `GPUStartTime`/`GPUEndTime` Metal'de sıfır dönebiliyor ("başlamadı" / "bildirim gelmedi") ve o kare **hiç örnek yazmıyor** (0 ns p95'i aşağı çeker). `ornek=` jetonu bu yüzden tek sayı olamaz: ya sütun başına verilir ya da en küçüğü basılıp hangi sütun olduğu söylenir — **karar phase-3'ün**. Düşen örnek (`dropped`) de aynı jetonun içinde görünmeli
- [ ] **phase-2'den devir — `BT_FRAME_STATS` süre ister.** `main.rs` `BT_SCROLL_TEST`'in eşi bir kolla süresiz (ve sıfır saniyelik) ölçümü eliyor: çıkış 1 + *"sıfırdan büyük bir BT_RUN_SECONDS ister"*. Sebebi rapor yolu: `report_and_exit` yalnız deadline ile koşuyor, yani süresiz ölçüm hiç basılmazdı. Belge güncellemesi (R7.2) kanca adını yazarken bu şartı da yazmalı
- [ ] **phase-2'den devir — halka kapasitesi.** `Stats::new(since, run_seconds)` içinde: `run_seconds × MAX_REFRESH_HZ` (120, bir **ayırma tavanı**; ölçülmüş tazeleme hızı değil), mutlak tavan `MAX_CAPACITY` (~10 dk). Kapı kapalıyken **hiç ayrılmıyor**. Sabitler `bt-gpu/src/stats.rs`'te ve rapor bunlara değil `dropped`'a bakmalı
- [ ] **phase-2'den devir — `## Yöntem`'e geçecek dürüst sınır.** Açılış damgası `main()`'in **ilk satırında**, `has_aqua_session()`'ın alt sürecinden de önce; ama yine de **süreç başlangıcı değil** (dyld + Rust runtime kurulumu önce bitiyor) ve bittiği yer **ilk tamamlanan kare** (`addCompletedHandler`), sunulan kare değil. `/measure`'ın "process başlangıcından" tarifinden bu kadar sapıyor
- [ ] **phase-2'den devir — R5.6'nın ölçüm koşusu 5 saniyeyle koşulamıyor.** `BT_SCROLL_TEST=1 BT_RUN_SECONDS=5` kapanışta asılıp bekçiye düşüyor (`_exit(70)`, jeton satırı hiç basılmıyor) ve bu **phase-2'den önce de böyle** (`adb8f50` üstünde `git stash` ile doğrulandı). 3 saniye sağlam. **Kök neden bulundu** (phase-2'nin `/code-review`'u, teşhis kodla doğrulandı): `Session::shutdown` (`crates/bt-core/src/session.rs:820`) önce `Msg::Shutdown` yollayıp okuyucu thread'i **join ediyor**; dönen değerin içindeki `Pty` ana thread'de düşüyor ve `Pty::drop`'un `SIGHUP`'ı master fd'yi **kimse okumazken** gidiyor. PTY'yi doldurmakta olan çocuk çıkışın içinde takılıyor (`ps` durumu `?Es`), `child.wait()` dönmüyor, bekçi 70 ile kesiyor. `smoke_shell` bunu göremez (~100 bayt yazıp uyur); `load_shell` sürekli akıtır ve betiğin `+ 1`'i çocuğun deadline anında hâlâ akıtıyor olmasını garanti eder. Ölçülen sıklık: 3 saniyede on koşunun **dördü** (12,1 sn sonra çıkış 70, stdout boş). **Phase-3 için neden kritik:** rapor `report_and_exit`'ten basılıyor ve asılan koşuda jeton satırı **hiç çıkmıyor** — yani ölçüm koşularının %40'ı sessizce kayboluyor. Seçenekler: (a) kalıcı çözüm — `bt-core`'da sınırlı bekleme (`SIGHUP` → süre → `SIGKILL`) ya da `child.wait()` öncesi master'ı boşaltmak (`CLAUDE.md`'nin adıyla andığı borç), (b) ölçümü bu borç kapanana kadar yalnız `Smoke` yüküyle koşmak
- [ ] **phase-2'den devir (phase-1 devir 3) — boşta sıfır karenin daha derin ölçütü.** Bekçi bugün **GPU karesini** sayıyor (`kare`), yani örtülme ve display link'in askıya alınması onu köreltiyor (tavan 3). Daha derin ölçüt **kare talebi**: `Waker::wake` / `request_frame` sayısı — bizim tarafımızda, örtülmeden etkilenmez ve bilerek bozulmuş boşta-sıfır-kareyi anında yakalar. Yeni sayaç **ve yeni jeton** demek; jeton basmak bu phase'in işi
- [ ] **phase-2'den devir — izlenmeyen belge borcu.** `/audit` (mercek 10) yakaladı: `CLAUDE.md`'nin **katman tablosundaki** `bt-gpu` satırı crate'in yeni sorumluluğunu (ölçüm defteri) anmıyor. `crates/bt-gpu/src/lib.rs` başlık yorumu phase-2'de güncellendi; tablo satırı R7.2'nin kanca adları listesinde **yok**, yani bu satır yazılmasa kimse görmezdi
- [ ] Test: `few_samples_suppress_p95` — taban altında sayı basılmıyor
- [ ] Test: `p95_returns_none_on_empty_ring`
- [ ] Test: `token_line_preserves_old_tokens` — beş eski jeton ve `pipeline=ok` yerinde
- [ ] Test: `smoke_counts_unchanged` — `kare=1 hucre=8 glif=6 kural=15` bit bit aynı
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
