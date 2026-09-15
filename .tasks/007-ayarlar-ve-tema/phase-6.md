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
  paneli erişilemiyor" borcu kapanır; paketten göz kontrolüyle doğrulandı).
- **ayar şeması** — şablon dosyası.
- Klavye davranışı: Cmd-Q çalışır ve açık programı sormadan kapatır, Cmd-H
  gizler (teslim notu; kapatma onayı kapsam dışı).
- **Bekleyen göz kontrolü** `[elle]`: Cmd-C/V/Q/T tuşları, seçimle kopyalama
  ve başka uygulamayla pano alışverişi (Uygulama Notları).
- `CLAUDE.md` "Bugünkü hâl" ana menüyü, `bt-shell` başlık yorumu `menu`'yü
  söylüyor.

## Checklist

- [x] `objc2-app-kit` feature'ları; `Cargo.lock` kontrolü
- [x] `menu.rs`: bateri ve Düzen menüleri
- [x] `BateriView` `copy:`/`paste:`; köprü silindi, yutma dalı kaldı
- [x] Ayarlar… eylemi (şablon, editör, izlemeyi yeniden kurma)
- [x] Şablon `bt-core`'da, varsayılanlarla sınamayla bağlı
- [x] Yorumlar: `view.rs:309-314`, `:491-493`, `app.rs:332`
- [x] Test: şablon, yutma
- [x] `docs/AYARLAR.md`
- [x] Doğrulama geçti (`make hepsi` + `make duman` + `make kur`)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Bayraklar kök `Cargo.toml`'da değil `bt-shell/Cargo.toml`'da** (phase-3
  emsali); `objc2-foundation`'a `NSURL` da eklendi. `Cargo.lock` değişmedi,
  phase riskli değil.
- **Uygulama menüsüne Hide bateri (Cmd-H), Hide Others, Show All eklendi**
  (planda yok): macOS'un standart öğeleri, yoksa Cmd-H yutuluyordu. AppKit
  menülere kendi öğelerini de ekliyor (Quit and Keep Windows, Edit'te
  AutoFill/dikte/emoji).
- **Şablonda anahtarlar değeriyle yazılı, yorumda değil** (yalnız varsayılanı
  olmayan `family` yorumlu örnek); bölüm başlıkları açık — yorumu kaldırılan
  anahtar başlıksız kalıp sessizce yoksayılmasın. Sınama anahtarların
  varlığını da bağlıyor (boş şablon eşitliği geçerdi). Şablon
  `docs/AYARLAR.md`'de blok olarak duruyor ve
  `documented_template_is_the_template` ile bağlı.
- **Editör:** önce `NSWorkspace::openURL` (türün uygulaması), `false`
  dönerse `open -t` (varsayılan metin editörü) — `.toml`'u sahiplenen
  uygulama her makinede yok. Yedek yol **sınanmadı**: bu makinede `.toml`'un
  sahibi var. İkisi de açamazsa ayar yuvasına dosyanın yolu.
- **Hata ayar yuvasına, okumanın tanılarının arkasına** ekleniyor: Settings…
  yarattıktan sonra `reload_settings` yuvayı dosyanın hâline göre yeniden
  yazdığı için önce yazılan hata hemen silinirdi.
- **Göz kontrolü** (paketten, geçici `HOME`, menü öğeleri System Events'le
  süreç hedeflenerek): About paneli atıfla açıldı; Settings… açılışta olmayan
  dizini ve şablonu yarattı (içerik belge bloğuyla aynı), `.toml` editörünü
  öne getirdi; aynı dosyada `theme` yerinde değiştirilince tema anında döndü
  (izleme yeni dizini görüyor); Edit ▸ Paste metni yapıştırdı; bateri ▸ Quit
  süreci temiz kapattı, stderr boş. **Gözle sınanmayan:** Cmd-C/V/Q/T
  tuşları, seçimle Copy ve başka uygulamayla pano alışverişi — fare
  sürüklemesi pencerenin boyutlandırma kenarına düştü ve kullanıcı makineyi
  kullanıyordu, tuş/fare enjeksiyonu başka uygulamaya düşebileceği için
  bırakıldı. Etkin olmayan uygulamanın menüsünden tıklanan Paste hiçbir şey
  yapmadı: key pencere yok, eylem view'a varmıyor (AppKit davranışı; kullanıcı
  etkin olmayan uygulamanın menüsüne tıklayamaz).
