# Phase 6 — Ana menü

## Özet

Uygulamaya ana menüyü getir: About, Ayarlar…, Çıkış ve Düzen'de
Kopyala/Yapıştır — Cmd-C/V'nin geçici `keyDown:` köprüsü menü seçicilerine
taşınır.

_Requirements: R7, R10_

## Değişiklikler

- **Kök `Cargo.toml`** — `objc2-app-kit` feature listesine `NSMenu`,
  `NSMenuItem`, `NSWorkspace`. Feature `Cargo.lock`'a yazılmıyor olmalı;
  değişirse phase riskli sayılır ve kutusu açılır.
- **`crates/bt-shell/src/menu.rs` (yeni)** — ana menünün kurulumu (ana
  thread, `MainThreadMarker`):
  - **bateri:** "About bateri" (`orderFrontStandardAboutPanel:` — paket
    içindeki `Credits.html`'i gösterir), ayırıcı, "Settings…" (Cmd ,),
    ayırıcı, "Quit bateri" (Cmd Q → `terminate:`).
  - **Edit:** "Copy" (Cmd C → `copy:`), "Paste" (Cmd V → `paste:`).
    Hedefsiz; first responder `BateriView`.
  - Menü dizgileri İngilizce (CLAUDE.md → Dil: UI dizgileri İngilizce);
    belgelerde Türkçe karşılıkları ("Ayarlar…") yalnız anlatım içindir.
- **`crates/bt-shell/src/view.rs`** —
  - `copy:` ve `paste:` seçicileri: bugünkü `run_shortcut`'ın iki kolunun
    yaptığını yapar (`clipboard::copy` / `clipboard::read` → oturum).
  - `command_shortcut`, `run_shortcut`, `Shortcut` ve onlara bağlı sınamalar
    **silinir** (köprünün kendi doc'u: "menü günü silinir", `:491-493`).
  - **Command'lı tuşu yutan `return` kalır** (`:319`): menüde karşılığı
    olmayan Cmd-T kabuğa "t" yazmamalı. Yorum (`:309-314`) yeni gerekçeyle:
    menü `performKeyEquivalent:` ile önce yakalar, yakalamadığı yutulur.
  - Yutmayı bağlayan bir sınama `keyDown:`'ın kalan dalına yazılır; saf bir
    karar fonksiyonuna iniyorsa oradan.
- **`crates/bt-shell/src/app.rs`** —
  - Menü `did_finish_launching`'de kurulur.
  - **Ayarlar…** eylemi: dizin ve `settings.toml` yoksa oluşturur (içerik
    şablon), dosyayı varsayılan editörde açar (`NSWorkspace`), izleme
    kaynaklarını yeniden kurar (phase-4'ün dış tetiği). Oluşturma hatası
    ayar yuvasına.
  - `:332` yorumu: Cmd-Q artık menüden `terminate:`'e varır.
  - Hermetik dalda "Ayarlar…" dosya oluşturmaz (süreli koşu menüyle
    etkileşmez; dal yine tek).
- **`crates/bt-core/src/settings.rs`** — **şablon** metni: her anahtar
  yorumlu, varsayılan değeriyle ve kısa açıklamayla. Şablonun sahibi
  varsayılanların sahibiyle aynı yer: şablon ayrıştırılınca `Settings::default()`
  çıkması sınamayla bağlanır.
- **`docs/AYARLAR.md`** — "Ayarlar…" menüsü, şablon; "dizini elle oluşturan
  kullanıcı" cümlesi "Ayarlar…"ı gösterir.

## Kabul

- Şablon ayrıştırılınca tanısız `Settings::default()` verir.
- Yutma sınaması: menüde olmayan Command'lı harf PTY'ye yazılmaz.
- `make duman` jetonları değişmez; kapanış yolu aynı (`kapanis=clean`).
- `make kur` yeşil (About paneli paketin `Credits.html`'ini okur; paket
  içeriği değişmedi).
- Göz (paketten açılışta):
  - Seçip Cmd-C, başka uygulamada yapıştır; başka uygulamadan kopyalayıp
    Cmd-V.
  - Cmd-Q uygulamayı kapatır; Cmd-T kabuğa hiçbir şey yazmaz.
  - bateri ▸ About paneli atıfla açılır.
  - Ayarlar… dosya yokken şablonu oluşturup editörde açar; kaydedilen
    değişiklik anında uygulanır.

## Yayın Etkisi

- **app bundle** — About paneli artık erişilebilir (006'dan kalan "About
  paneli erişilemiyor" borcu kapanır; borç `context.md`'ye taşındı, göz
  kontrolüyle kapanışı `teslim.md`'ye düşer).
- **ayar şeması** — şablon dosyası.
- Klavye davranışı: Cmd-Q çalışır ve açık programı sormadan kapatır (teslim
  notu; kapatma onayı kapsam dışı).

## Checklist

- [ ] `objc2-app-kit` feature'ları; `Cargo.lock` kontrolü
- [ ] `menu.rs`: bateri ve Düzen menüleri
- [ ] `BateriView` `copy:`/`paste:`; köprü silindi, yutma dalı kaldı
- [ ] Ayarlar… eylemi (şablon, editör, izlemeyi yeniden kurma)
- [ ] Şablon `bt-core`'da, varsayılanlarla sınamayla bağlı
- [ ] Yorumlar: `view.rs:309-314`, `:491-493`, `app.rs:332`
- [ ] Test: şablon, yutma
- [ ] `docs/AYARLAR.md`
- [ ] Doğrulama geçti (`make hepsi` + `make duman` + `make kur`)
- [ ] Yayın etkisi yazıldı
