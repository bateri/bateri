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
- `CLAUDE.md` bağımlılık satırı, "Ayarlar" maddesi ve bugünkü hâl; `bt-core`
  ve `bt-shell` `lib.rs` başlık yorumları.
- **app bundle / lisans** — yeni paketlerin hepsi MIT seçilebilir; atıf
  mevcut MIT bildirim borcuna eklenir (`docs/YOL-HARITASI.md`), yeni dosya
  yok.

## Checklist

- [x] Alt başlık görünürlüğü göz kontrolü (ilk adım)
- [x] `toml_edit` bağlandı, karar yorumu yazıldı
- [x] `bt-core::settings` ayrıştırıcı ve `Settings`
- [x] `bt-shell` yükleyici (kök parametre) ve hermetik tek dal
- [x] `scrollback` `Settings`'ten; `SCROLLBACK` sabiti kalktı
- [x] Alt başlık yuvası ve tek sahip fonksiyon; stderr
- [x] Test: ayrıştırma hâlleri, geçici dizinde yükleyici, hermetik karar
- [x] `docs/AYARLAR.md`, `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (`Cargo.lock`)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Alt başlık başlığın yanında, aynı satırda** çiziliyor ("bateri – …"),
  altında değil: pencerede araç çubuğu yok. Tanı metni bu yüzden tek satır
  (`Diagnostic`'in `Display`'i) ve birden çok tanıda "ilk (+N daha)".
- **`toml_edit` yalnız `parse`.** Biçim koruyan yazma `display` ister ve o
  `toml_writer`'ı `Cargo.lock`'a sokar: phase-7'ye riskli kutusu eklendi.
- **`Cargo.lock`'a 13 paket girdi, 7'si derleniyor** (`toml_edit`,
  `toml_parser`, `toml_datetime`, `winnow`, `indexmap`, `hashbrown`,
  `equivalent`). `serde_core`, `serde_derive`, `syn`, `quote`, `proc-macro2`,
  `unicode-ident` `toml_datetime`'ın zayıf `serde_core?/std` özelliği yüzünden
  kilitte duruyor, `cargo tree -p bt-core`'da yok. Yedisi de MIT
  seçilebilir; atıf, `CLAUDE.md`'deki mevcut MIT bildirim borcuna düşer.
- **`scrollback` tavanının kaynağı alacritty uygulaması**
  (`alacritty/src/config/scrolling.rs`, `MAX_SCROLLBACK_LINES = 100_000`),
  `alacritty_terminal` değil — sabit `bt-core`'a kaynağıyla kopyalandı.
  Tavanı aşan değer **tavana** kırpılır ve tanı bırakır (niyet "çok geçmiş");
  negatif ya da tam sayı olmayan varsayılana döner.
- **Hermetik dal `app::Inputs`** (`Hermetic` | `User { config_root }`);
  ev dizini çözülemezse `config_root: None`, stderr'e bir satır, alt başlık
  yok. Ev dizini `child::home()`'dan (`working_directory` ile aynı çözüm).
- **Alt başlıktaki tanı İngilizce** (phase metni "Türkçe tanı metni"
  diyordu): pencerede görünen metin UI dizgisi, `CLAUDE.md` → Dil'e bu
  ayrım bir cümleyle yazıldı; stderr aynı metni basıyor.
- **Ayarlar geometriden ve oturumdan önce okunuyor:** phase-5'in fontu
  ilk grid'i belirleyecek. `Inputs` saklanmıyor, her soruşta `run`'dan
  türüyor (ikinci kopya olmasın).
- **`/code-review` kararları.** Düzeltilen: ev dizini çözülemezse tanı alt
  başlığa da gidiyor; kırık sembolik bağ "dosya yok" değil okunamadı; düz
  dosya olmayan yol (FIFO, dizin) okunmadan eleniyor; `[[terminal]]` kendi
  adıyla; ayrıştırıcı iletisinin "expected …" listesi kesiliyor; tanı biçimi
  tek yardımcıda; `SCROLLBACK_MAX` `pub(crate)`; `AYARLAR.md`'ye `i64`
  sınırı, tam ekran, yol düzeltmesi; yeni paketler lisans borcu maddesine
  adıyla. **Waive:** ana thread'de sınırsız okuma (iCloud'dan tahliye
  edilmiş hedef, takılmış ağ ev dizini) — bilinen sınır olarak
  `bt-shell/src/settings.rs` doc'unda; `i64`'ü aşan `scrollback` tavana
  kırpılamıyor (değer ayrıştırıcıda düşüyor), belgelendi. **Red:** tavanı
  `Session`'da da zorlamak — tavan kullanıcı girdisinin kuralı, oturumun
  değişmezi değil.
