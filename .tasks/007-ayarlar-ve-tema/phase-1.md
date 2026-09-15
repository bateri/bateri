# Phase 1 — Ayar modeli ve görünür hata

## Özet

`settings.toml`'u açılışta okuyan saf ayar modeli, kapıları ondan yalıtan
tek dal ve hatayı pencere alt başlığında gösteren yuva sistemi; ilk tüketici
`scrollback`.

_Requirements: R1, R1.1, R1.2, R1.3, R2, R10_

## Değişiklikler

- **İlk adım — göz kontrolü (kod yazmadan önce):** `NSWindow.subtitle`'ın
  bugünkü düz `Titled` pencerede (`app.rs:253-256`, araç çubuğu yok) çizildiği
  denenir. Çizilmiyorsa phase **durur**, alternatif kullanıcıya gider
  (`discussion.md` → Karar 8).
- **Kök `Cargo.toml`, `crates/bt-core/Cargo.toml`** — `toml_edit` eklenir;
  satırın üstünde karar kaydına işaret eden yorum (`007 discussion.md →
  Karar`), öteki bağımlılık satırlarının biçiminde. Feature'lar gerekenle
  kırpılır, `serde` açılmaz.
- **`crates/bt-core/src/settings.rs` (yeni)** — `Settings` (bugün yalnız
  `scrollback`; sonraki phase'ler alan ekler), `Default`, tanı tipi (anahtar
  yolu, varsa satır, Türkçe tanı metni). Tek giriş: metin → ya
  "ayrıştırılamadı" tanısı ya da `Settings` + tanı listesi. Anahtar anahtar
  okur: yanlış tip → varsayılan + tanı; bilinmeyen anahtar → sessiz.
  `scrollback` tavanı bir **kaynağa** bağlanır (alacritty `Term` kırpmıyor);
  kaynak doc'ta yazılı, sayı uydurulmaz. Dosya sistemi görmez.
- **`crates/bt-core/src/lib.rs`** — `pub use`; başlık yorumundaki `pub` tip
  listesi güncellenir.
- **`crates/bt-shell/src/settings.rs` (yeni)** — yükleyici: kök dizini
  **parametre** alır, `settings.toml`'u okur. Dosya yok → varsayılan, tanı
  yok; okuma hatası → tanı. Sonuç "dosya yok", "ayrıştırılamadı" ve
  "okundu"yu **ayrı** taşır, `Settings::default()`'a çökertmez: canlı
  yenileme ayrıştırılamayan dosyada hiçbir şey uygulamamalı (phase-4) ve
  açılışta ayrıştırılamayan dosyanın varsayılanı OSC 52'yi kapatmalı
  (phase-8). Üretim kökü `$HOME/.config/bateri/`, ev dizini
  `child.rs`'in çözümünden.
- **`crates/bt-shell/src/app.rs`** —
  - **Hermetik dal:** süreli koşuda (`run.is_some()`) yükleyici hiç
    çağrılmaz, `Settings::default()` kullanılır. Dal **tek yerde**; sonraki
    phase'lerin girişleri (izleme, görünüm, menü dolumu) aynı daldan geçer.
    Kararın kendisi saf bir fonksiyonsa sınanır.
  - `SCROLLBACK` sabiti kalkar, değer `Settings`'ten gelir (varsayılanın
    sahibi `bt-core`).
  - **Alt başlık yuvaları:** kaynak başına yuva; bu phase'de yalnız ayar
    dosyası yuvası var (kullanılmayan yuva türü ölü kod olurdu, sonraki
    phase'ler kendi yuvasını ekler). Alt başlığın **tek sahibi** bir
    fonksiyon; birden çok dolu yuvada ilki ve sayısı. Tanı stderr'e de
    `bateri:` önekiyle.
- **`docs/AYARLAR.md` (yeni)** — dosyanın yeri, hata davranışı (alt başlık,
  ayrıştırılamayan dosya, yanlış tip, bilinmeyen anahtar), `[terminal]
  scrollback`. Bu phase'de değişiklik **yeniden açılışta** uygulanır;
  phase-4 bu cümleyi değiştirir.
- **`CLAUDE.md`** — "Taban: … `toml` + `serde`" → `toml_edit` (gerekçesi
  karar kaydında); "Ayarlar" maddesinde `docs/AYARLAR.md`'ye işaret.

## Kabul

- `bt-core` sınamaları: dosya boş; geçerli `scrollback`; yanlış tip →
  varsayılan + tanı; tavanı aşan değer; bilinmeyen anahtar ve bilinmeyen
  bölüm (`[motion]`) → tanısız; ayrıştırılamayan metin → ayrı sonuç.
- `bt-shell` sınaması geçici dizinde: dosya yok, okunamayan dosya, geçerli
  dosya. Hiçbir sınama gerçek `HOME`'u okumaz.
- Hermetik dalın kararı sınanır: süreli koşu yükleyiciyi çağırmaz.
- `make duman` jetonları değişmez (`hucre=8 glif=6 kural=15`).
- Göz: bozuk bir `settings.toml` ile açılışta alt başlık hatayı söyler;
  düzeltip yeniden açınca kaybolur.

## Yayın Etkisi

- **yeni bağımlılık** — `toml_edit`; kullanıcı onayı `discussion.md` →
  Karar. `Cargo.lock` değişir.
- **ayar şeması** — `[terminal] scrollback` (varsayılan bugünkü değer);
  `docs/AYARLAR.md` doğar.
- `CLAUDE.md` bağımlılık satırı; `bt-core` `lib.rs` başlık yorumu.

## Checklist

- [ ] Alt başlık görünürlüğü göz kontrolü (ilk adım)
- [ ] `toml_edit` bağlandı, karar yorumu yazıldı
- [ ] `bt-core::settings` ayrıştırıcı ve `Settings`
- [ ] `bt-shell` yükleyici (kök parametre) ve hermetik tek dal
- [ ] `scrollback` `Settings`'ten; `SCROLLBACK` sabiti kalktı
- [ ] Alt başlık yuvası ve tek sahip fonksiyon; stderr
- [ ] Test: ayrıştırma hâlleri, geçici dizinde yükleyici, hermetik karar
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (`Cargo.lock`)
- [ ] Yayın etkisi yazıldı
