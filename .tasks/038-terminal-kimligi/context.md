# Terminal kimliği ve sekme URL'i — Bağlam

## Mevcut Durum

- **Kabuğa giden kimlik yalnız `TERM` ve `COLORTERM`.** İkisi `bt-core`'un
  sabiti ve `Session::spawn`'da ek ortamın haritasına **sonra** giriyor, yani
  ezilemiyor (`crates/bt-core/src/session.rs`, `SessionOptions::env`'in
  öncelik doc'u ve `spawn`'un gövdesi; bekçisi
  `extra_env_reaches_child_without_overriding_term`). Ek ortamı `bt-shell`
  kuruyor: `TerminalWindow`'un oturum doğumunda
  `child::locale_env()` + shell entegrasyonunun ortamı
  (`crates/bt-shell/src/window.rs`, `SessionOptions { env: … }`). Kural
  `CLAUDE.md` → Bilinmesi gerekenler → "`tty::setup_env()` çağrılmaz".
- **`TERM_PROGRAM` hiç yazılmıyor.** Dock'tan açılışta launchd'nin ortamında
  yok, yani kabuk "hangi terminal" sorusuna boş cevap alıyor; Terminal.app ya
  da iTerm2'den `cargo run` ile açılışta ise o terminalin değerleri
  (`TERM_PROGRAM=Apple_Terminal`, `TERM_SESSION_ID`, `ITERM_SESSION_ID`,
  `LC_TERMINAL`…) **miras** kalıyor — kabuk kendini başka bir terminalde
  sanıyor.
- **Sekmenin dışarıdan adı yok.** Her sekme bir `NSWindow` ve kendi
  `Session`'ı (`CLAUDE.md` → 026 paragrafı); pencerenin kimliği süreç içi bir
  sayaç (`AppDelegate::next_window_id`, `TerminalWindow::id` → `u64`),
  tüketicileri iç haberciler (alternatif ekran habercisi vb.). Yeniden
  başlatmada sıfırdan sayıyor, yani dışarı verilecek bir ad olamaz.
- **`bateri://` şeması bugün yalnız içeride.** Prompt'un OSC 8 çıpası
  `bateri://block/N` (`assets/shell/zsh/bateri.zsh` → `__bateri_anchor`;
  okuyan `session.rs` → `block_id`). bateri OSC 8 bağlantısını **açmıyor**:
  hücrenin bağlantısı yalnız blok kimliği olarak okunuyor, tıklama yolu yok
  (`docs/YOL-HARITASI.md` → "tıklanabilir bağlantılar" satırı). Şema
  `Info.plist`'te kayıtlı değil (`assets/bundle/Info.plist.in`'de
  `CFBundleURLTypes` yok) ve `AppDelegate` URL teslimi almıyor
  (`application:openURLs:` yok).
- **Paket denetimi** `Makefile` → `kur`'un sonundaki içerik denetimi:
  `plutil -lint`, yer tutucu kalmaması, `CFBundleExecutable`,
  `LSMinimumSystemVersion` ↔ `minos`, ikon, lisans ve shell betiği. URL
  şeması denetlenmiyor.
- **Sürüm tek yerde:** bütün crate'ler `version.workspace = true`
  (`Cargo.toml` → `[workspace.package] version`); `make kur`
  `CFBundleShortVersionString`'i `cargo pkgid -p bateri`'den, yani aynı
  alandan dolduruyor.

## Motivasyon

Terminale bir **kimlik**, sekmelere **dışarıdan bilinirlik**:

- Kabukta koşan programlar terminali `TERM_PROGRAM`/`TERM_PROGRAM_VERSION`
  ile tanıyor (özellik algılama, hata raporu, kurulum sihirbazları).
  Bugün cevap ya boş ya da yanlış terminalin adı.
- Bir bildirim, betik ya da başka bir uygulama "beni doğuran sekmeye dön"
  diyebilmeli: `open $BATERI_TAB_URL` o sekmeyi öne getirir.
  `TERM_SESSION_ID` aynı kimliğin Terminal.app'in adlandırdığı yuvası;
  oturum boyunca sabit, sekme başına tek.

Kullanıcı onaylı (yeniden sorulmaz): link biçimi `bateri://tab/<id>`, adlar
`TERM_SESSION_ID` ve `BATERI_TAB_URL`, `TERM_PROGRAM` ailesi eklenecek.

### Kanıt — miras kalan `TERM_PROGRAM` sarmalayıcının dizinine yazdırıyor

`/etc/zshrc:74` `[ -r "/etc/zshrc_$TERM_PROGRAM" ] && . "/etc/zshrc_$TERM_PROGRAM"`
diyor (`/etc/bashrc:10` aynısı) ve `/etc/zshrc_Apple_Terminal` oturum
geçmişini `${ZDOTDIR:-$HOME}/.zsh_sessions`'a kuruyor. `/etc/zshrc` koşarken
`ZDOTDIR` hâlâ **bizim** sarmalayıcı dizinimizi gösteriyor (`.zshrc` geri
koyuyor, `/etc/zshrc`'den sonra). Ölçüldü (2026-09-26, macOS 26.4.1, boş
geçici `ZDOTDIR`, `zsh -ic`):

| ortam | `ZDOTDIR`'da doğan |
|---|---|
| `TERM_PROGRAM=Apple_Terminal TERM_SESSION_ID=X` | `.zsh_history`, `.zsh_sessions` |
| `TERM_PROGRAM=bateri TERM_SESSION_ID=X` | hiçbir şey |

Yani Terminal.app'ten `cargo run` ile açılan debug oturumu bugün Apple'ın
betiğini depodaki `assets/shell/zsh/`'e yazdırır (009 phase-3'ün
`.zsh_history` olayının ikinci bir kapısı; `bundle_assets` envanteri
kızarır), pakette de imzalı bundle'ın içine yazmaya çalışır. `TERM_PROGRAM`'ı
ezilemez yazmak bunu yan ürün olarak kapatıyor.

`TERM_PROGRAM=bateri` ile `/etc/zshrc_bateri` aranıyor ve yok — sistemin
oturum-geçmişi mekanizması **tetiklenmiyor**; `TERM_SESSION_ID`'yi okuyan tek
sistem betiği Apple'ınki ve o yalnız `TERM_PROGRAM=Apple_Terminal`'da
yükleniyor (tabloda ikinci satır).
