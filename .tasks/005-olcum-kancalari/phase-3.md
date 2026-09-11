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
- [ ] Test: `p95_returns_none_on_empty_ring`
- [ ] Test: `token_line_preserves_old_tokens` — beş eski jeton ve `pipeline=ok` yerinde
- [ ] Test: `smoke_counts_unchanged` — `kare=1 hucre=8 glif=6 kural=15` bit bit aynı
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
