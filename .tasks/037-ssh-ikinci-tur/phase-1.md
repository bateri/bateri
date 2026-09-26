# Phase 1 — Hedefin modeli, host işareti ve renk

## Özet

Yoklama ssh/mosh'un argv'sini de taşıyor, `bt-core` uzak hedefi bütün olarak
tutuyor; `[remote] hosts` desen listesi okunuyor ve işaretin rengi (yeni
`warning` rolü dahil) dock'un `⇄ host`'una ve üst çizgisine gidiyor.

_Requirements: R1.1, R1.2, R2.1, R2.2, R2.3, R2.4, R3.1, R3.2_

## Değişiklikler

- **`crates/bt-shell/src/jobs.rs`** — `Probe::Remote` host yerine bir hedef
  taşıyor: host (bugünkü ayrıştırıcıların çıktısı, değişmeden), tür (ssh /
  mosh) ve yeniden koşturulacak argv. argv kuralı saf bir fonksiyonda
  (Karar 6): ssh'ta `-L`/`-R`/`-D` değerleriyle, `-M` ve `-f` düşüyor,
  kalanı sırasıyla; birleşik kısa seçenekler (`-fN`, `-p2222`) 036'nın
  `SshSession` yürüyüşüyle aynı kuralla bölünüyor — ikinci bir ayrıştırıcı
  yazılmıyor. mosh'ta `mosh` + betiğin argümanları; yalnız `mosh-client`
  görüldüyse onun `-#` satırı (036 phase-3 → Uygulama Notları: bütün komut
  satırı).
- **`crates/bt-shell/src/quote.rs`** — yeniden koşturma satırı için ayrı giriş
  noktası: `shell_quote`'un kuralı + `@ : , +` her yerde, `=` sözcük başı
  dışında (Karar 1); damlanın kuralı değişmiyor.
- **`crates/bt-shell/src/window.rs`** — `probe_remote` hedefin satırını o
  giriş noktasından (argüman başına) üretip `set_remote`'a veriyor.
  Ayar yüklenince (açılış ve `watch`) desen listesi her oturuma gidiyor
  (`app.rs`'teki ayar uygulama yolu, `set_terminal_options`'ın yanında).
- **`crates/bt-core/src/shell.rs`** — `DockContext`'in uzak yuvası hedefi
  (host, tür, argv, satır) ve çözülmüş **işareti** taşıyor; `clone_from`'un
  kapasite koruması bugünkü gibi (036 phase-1 → Uygulama Notları). `C`/`D`/`A`
  silme kuralı değişmiyor. Kontrol karakteri reddi (036 `/code-review`)
  host'ta aynen; argv ve satır başlığa/hücreye girmediği için orada yok.
- **`crates/bt-core/src/session.rs`** — `set_remote(nesil, Option<hedef>)`;
  nesil kapısı aynen. Yeni setter (`set_host_marks`, `set_theme` emsali:
  yaprak kilit, değişimde kare ister, aynı listede no-op) listeyi tutuyor ve
  etkin uzak hedefin işaretini yeniden çözüyor. `set_remote` de çözüyor. Kare
  yolu yalnız çözülmüş işareti okuyor.
- **`crates/bt-core/src/settings.rs`** — `[remote] hosts`: tip (`HostRule {
  pattern, mark }`, `HostMark` = Production/Staging/Development/None/Rgb),
  ayrıştırma (`parse_keeping` kuralı: bozuk girdi anahtarın tamamını
  reddeder), `Settings::changes`'e alan, eşleştirici (saf; `*`/`?`, harf
  duyarsız, `@`'siz desende girdinin son `@`'ten sonrası, ilk eşleşen,
  `none` durdurur — Karar 2). Yazım yolu (`SettingsEdit` kolu) phase-2'de.
- **`crates/bt-core/src/color.rs` + `theme.rs`** — `warning` rolü: `Theme`
  alanı, `#d6b16a` / `#8f6a00` (Karar 3), `warning_linear`, tema dosyasında
  opsiyonel anahtar; `the_info_role_reads_on_the_ground` iki rolü de kapsıyor.
  İşaretten renge tek fonksiyon (`Theme` üstünde; `Rgb` kolu lineerleşerek).
- **`crates/bt-core/src/dock.rs`** — `render_remote_context`'in `⇄ host`'u
  ve `Dock`'un üst çizgisi `info_linear()` yerine işaretin rengini alıyor;
  işaretsizde değer bugünküyle bit bit aynı.
- **`docs/AYARLAR.md`** — `[remote] hosts` bölümü, `warning` tema anahtarı,
  şablona yorumlu örnek (şablonun İngilizce dili).

## Kabul

- `jobs` saf sınamaları: `ssh -p 2222 -J jump -L 8080:x:80 prod` → argv
  `ssh -p 2222 -J jump prod`; `ssh -fN …` etkileşimsiz (036'nın cevabı
  değişmedi); `ssh -t prod tmux attach` argv'si aynen; mosh betiği ve yalnız
  `mosh-client` iki kolu. 036'nın bütün `jobs` sınamaları host'ta aynı.
- Satır: `ssh -o User=x deploy@prod` kaçırmasız; `=x` sözcük başında ve
  boşluklu argüman kaçırılıyor; damlanın `shell_quote` sınamaları değişmedi.
- Eşleştirici: `prod-*` ~ `prod-web-1`, `PROD-WEB-1`; `prod-?` ≁
  `prod-10`; `deploy@prod` girdisinde `prod` eşleşir, `root@*` yalnız
  `root@…`'da; sıra ve `none`'un durdurması.
- Ayar: geçerli dizi, bozuk girdide önceki değer + tanı, round-trip bilinmeyen
  anahtarı ve yorumu koruyor; `changes` alanı.
- `Session`: `set_remote` ve `set_host_marks` sonrası çözülmüş işaret;
  eşleşmede dock hücresinin rengi `error`/`warning`/`success`/hex; işaretsizde
  ve eski sınamalarda `info` (036'nın `a_remote_session_shows_the_host_…`
  sınaması değişmeden yeşil).
- `warning` zeminde 3:1, iki gömülü temada.

## Checklist

- [x] `jobs`: hedef tipi, argv ayıklama, mosh `-#`
- [x] Yeniden koşturma satırının kaçırması (`quote.rs`)
- [x] `probe_remote` satır üretimi; ayarın oturumlara canlı gidişi
- [x] `DockContext` hedef + işaret; `set_remote` imzası; `set_host_marks`
- [x] `[remote] hosts` ayrıştırma, eşleştirici, `changes`
- [x] `warning` rolü ve bekçisi; işaret → renk
- [x] Dock'un iki rengi
- [x] `docs/AYARLAR.md`
- [x] Test: yukarıdaki Kabul maddeleri
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Gözle kontrol (devir mesajının cümlesi): `settings.toml`'a
  `[remote] hosts = [{ host = "<gerçek host>", mark = "production" }]` yaz ve
  kaydet, `ssh <host>` — **dock**'ta `⇄ host` ve üst çizgi kırmızı (`error`),
  `staging` sarı, `development` yeşil; deseni silince camgöbeğine dönüyor
  (ssh sürerken, kayıt anında). Açık temada da okunuyor.

## Uygulama Notları

- **Uzak yuva iki alan:** `DockContext::remote: Option<RemoteTarget>` ve
  yanında `remote_mark: HostMark`; işaret `clear_remote`'ta sıfırlanıyor.
  `RemoteTarget`'ın `Clone`'u elle (`clone_from` argv'nin ve iki dizginin
  kapasitesini koruyor — kare yolu bağlamı her karede kopyalıyor). Host
  okuyan yerler `DockContext::remote_host()`'tan.
- **Desen listesi `ShellLog`'da** (`host_rules`), `Session`'da değil: çözüm
  `set_remote` ile aynı yaprak kilitte. `Session::set_host_marks`'ın dönüşü
  **görünen işaretin** değişimi (phase-2'nin sekme noktası onu tüketecek),
  listenin değil; `set_remote` de host aynı kalıp işaret değişirse (liste
  arada değişmişse) kare istiyor.
- **`[remote] hosts` iki yazılışı da okuyor**: satır içi dizi ve
  `[[remote.hosts]]` bölüm dizisi. Tablo olmayan öğe, `host`'suz ya da boş
  `host`'lu girdi, bilinmeyen/eksik `mark` anahtarın tamamını reddediyor;
  tanı ilk bozuk girdinin satırında. `theme::hex_color` `pub(crate)` oldu
  (doğrudan renk aynı ayrıştırıcıdan).
- **Şablon `r##"…"##`**: `"#c678dd"` örneği `r#"` sınırlayıcısını kapatıyordu.
  Şablona `[remote] hosts = []` ve yorumlu örnek girdi; `docs/AYARLAR.md`'ye
  `[remote]` bölümü, `warning` satırı ve iki gömülü tema bloğuna `warning`.
- **mosh argv'si:** betik görüldüyse `mosh` + betiğin argümanları; yalnız
  `mosh-client` görüldüyse `-#` satırı **boşlukla bölünüyor** — betik satırı
  tırnaksız birleştirdiği için boşluklu bir değer (`--ssh="ssh -i k"`) geri
  kurulamıyor (036'nın host sınırıyla aynı kök). mosh.pl sonunda
  `mosh-client`'a `exec` ettiği için bu kol mosh'un olağan hâli.
- `command_line` boş argümanı `''` yazıyor (yoksa satırdan düşerdi);
  kontrol karakterli argümanı `$'…'`'e alıyor (`/code-review` bulgusu: `\` +
  satır sonu satır devamı olup argümanı birleştirirdi).
- `jobs` sınamalarında `remote_of` artık yalnız host'a bakıyor (036'nın
  sınamaları değişmeden); 037'ninkiler `target_of`/`probe_of`'la bütün hedefe.
- `CLAUDE.md` bu phase'te güncellenmedi (R9 phase-5'in): "sekizi tüketiliyor"
  ve ayar anahtarları listesi (`[remote] hosts` yok) şimdilik eski.
- `make hepsi`'nin ilk koşusu `bt-shell` sınamalarında SIGSEGV ile düştü
  (bilinen, 021/030/035'te kayıtlı ara sıra çökme); `bt-shell` üç kez ayrı ve
  `make hepsi` bir kez daha yeşil.

