# Phase 1 — Kimlik ortamı

## Özet

Her sekmenin kabuğuna `TERM_PROGRAM`, `TERM_PROGRAM_VERSION`,
`TERM_SESSION_ID` ve `BATERI_TAB_URL`'i ezilemez olarak ver; kimliğin biçimi
`bt-core`'da, üretimi `bt-shell`'de.

_Requirements: R1, R1.1, R1.2, R1.3, R2, R2.1_

## Değişiklikler

- **`crates/bt-core/src/identity.rs`** (yeni modül; `session.rs` zaten
  on sekiz bin satır) —
  - `TabId`: kanonik UUID metnini (8-4-4-4-12 onaltılık) kabul eden kurucu
    (`parse(&str) -> Option<TabId>`), içeride büyük harfe normalize; `Eq`,
    `Clone`, `Debug`; `as_str`.
  - URL'yi yazan tek yer (`url() -> String`, `bateri://tab/{id}`) ve çözen
    tek yer (`from_url(&str) -> Option<TabId>`): şema ve host büyük-küçük
    harf duyarsız, UUID `parse`'tan; sorgu, parça, fazladan yol bileşeni,
    sondaki `/` ve `bateri://block/…` → `None`. Panik yok (`make denetim`).
  - Sabitler: `TERM_PROGRAM` (`"bateri"`) ve `TERM_PROGRAM_VERSION`
    (`env!("CARGO_PKG_VERSION")`), `pub const`. Modül başlığı neden burada
    olduklarını söyler: `TERM`'ün ailesi, şemanın öbür yolu (`block_id`)
    `session.rs`'te — başlık ona işaret eder.
- **`crates/bt-core/src/lib.rs`** — `mod identity;` ve `TabId` ile iki
  sabitin `pub use`'u.
- **`crates/bt-core/src/session.rs`** —
  - `SessionOptions`'a `tab_id: Option<TabId>` (doc: `None` yalnız sınama ve
    gömülü kullanım; uygulama her pencerede verir).
  - `SessionOptions::env`'in öncelik doc'u: "`TERM` ve `COLORTERM`" →
    kimlik ailesinin dördü de o katmanda.
  - `spawn`: `TERM`/`COLORTERM`'ün hemen ardından `TERM_PROGRAM` ve
    `TERM_PROGRAM_VERSION` koşulsuz; `tab_id` varsa `TERM_SESSION_ID`
    (`as_str`) ve `BATERI_TAB_URL` (`url()`). Yorum: miras kalan
    `TERM_PROGRAM=Apple_Terminal`'ın `/etc/zshrc` üzerinden `ZDOTDIR`'a
    yazdırması (`context.md` → Kanıt) — tek cümle + işaretçi.
  - `block_id`'nin doc'una tek satır: şemanın öbür yolu `identity`'de.
  - Bütün `SessionOptions { … }` sınama kurucularına `tab_id: None`.
- **`crates/bt-shell/Cargo.toml`** — `objc2-foundation` bayraklarına
  `"NSUUID"`; yorum emsal biçiminde (yeni crate değil, `Cargo.lock`
  oynamıyor, karar kaydı `.tasks/038-terminal-kimligi/discussion.md` →
  Karar 2).
- **`crates/bt-shell/src/window.rs`** —
  - Ivar: `tab_id: TabId`, `TerminalWindow::new`'da
    `NSUUID::new().UUIDString()`'ten `TabId::parse` ile, `expect` +
    gerekçe: `UUIDString` kanonik 8-4-4-4-12 biçimini veriyor, `parse` onu
    reddederse kusur `bt-core`'un sözleşmesinde (`bt-shell` panik kapısının
    dışında).
  - Erişici `tab_id(&self) -> &TabId`.
  - Oturum doğumu (`SessionOptions { … }`): `tab_id: Some(…clone())`.
    Süreli koşu aynı kol (Karar 8).
- **`crates/bt-shell/src/{child,jobs}.rs`** sınamalarındaki
  `SessionOptions { … }` kurucuları — `tab_id: None`.

## Kabul

- `bt-core` sınaması (`extra_env_reaches_child_without_overriding_term`'ün
  deseni, yanına): ek ortam dört anahtarı da başka değerle verse bile çocuk
  `TERM_PROGRAM=bateri`, `TERM_PROGRAM_VERSION=<sürüm>`, `TERM_SESSION_ID=<id>`
  ve `BATERI_TAB_URL=bateri://tab/<id>` görüyor. `tab_id: None`'da `bt-core`
  son ikisini yazmaz: ek ortamdaki değer çocuğa aynen geçer (ezilecek sabit
  yok) — sınama bunu da doğrular.
- `identity` birim sınamaları: `TabId::from_url(&id.url()) == Some(id)`;
  küçük harfli UUID ve `BATERI://TAB/…` kabul; `bateri://block/3`,
  `bateri://tab/`, `bateri://tab/<id>/`, `bateri://tab/<id>?x`,
  `bateri://tab/zz…`, `https://tab/<id>` → `None`.
- `bt-shell` sınaması: `bt_core::TERM_PROGRAM_VERSION ==
  env!("CARGO_PKG_VERSION")` (Karar 3'ün bekçisi).
- `bt-shell` sınaması: iki `NSUUID` kimliği `TabId::parse`'tan geçiyor ve
  farklı.
- `make duman` yeşil (oturum doğum yolu değişti; jetonlar oynamamalı).

## Checklist

- [ ] `identity` modülü: `TabId`, `url`/`from_url`, iki sabit, başlık yorumu
- [ ] `SessionOptions::tab_id`, `spawn`'da dört değişken, öncelik doc'u
- [ ] Bütün `SessionOptions` kurucuları güncel (bt-core, bt-shell)
- [ ] `NSUUID` bayrağı + yorum; pencere kimliği ivar ve erişici
- [ ] Test: ezilmezlik (dört anahtar) ve `tab_id: None` kolu
- [ ] Test: `from_url` ↔ `url` gidiş-dönüş ve ret listesi
- [ ] Test: sürüm eşitliği, iki `NSUUID` kimliği
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
