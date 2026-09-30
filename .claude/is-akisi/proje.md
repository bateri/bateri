# Proje profili — bateri

Skill'ler ve `duzen.md` **projeden bağımsızdır**: "kapı komutunu koş",
"kilit dosyası değiştiyse", "pahalı karar sınıfı" derler; bu projede neyin o
olduğunu **yalnız burası** söyler. Başka bir projeye taşırken yeniden yazılan
dosyalar bu dosya, `olcum.md` ve `.claude/settings.json`'dur — skill'lere,
`duzen.md`'ye ve şablonlara dokunulmaz. Genel dosyalara proje adı, komutu ya
da yolu girerse `make audit` kırmızı düşer.

Proje sözleşmesinin tamamı `CLAUDE.md`'dedir ve **burada tekrarlanmaz**; her
ajan onu zaten yükler, iki kopya hem bağlam hem drift demektir.

## İçindekiler

- Belgeler
- Doğrulama
- Dosya sınıfları
- Riskli phase tetikleyicileri
- Set kapısı ekleri
- Teslim
- Pahalı karar sınıfı
- Jüri mercek notları
- Otonom şerit ekleri
- Denetim mercekleri

## Belgeler

| rol | dosya |
|---|---|
| proje sözleşmesi | `CLAUDE.md` |
| sıra belgesi (açılmamış işler, sırası ve gerekçesi) | `docs/YOL-HARITASI.md` |
| ölçüm defteri (sayının tek sahibi) | `docs/OLCUMLER.md`; ölçüm türleri ve kancaları `olcum.md` |
| referans ürün envanteri | `docs/ARASTIRMA.md` (Metalterm; tarihli kayıt, bilerek eskir) |
| ayar belgesi | `docs/AYARLAR.md` |

## Doğrulama

| durum | komut |
|---|---|
| **kapı komutu** — her phase | `make check` — sürüm + `fmt` + `denetim` + `clippy -D warnings` + `test` |
| hızlı iç döngü | `cargo test -p {crate}` |
| mekanik denetim (kapının içinde) | `make audit` — katman yönü, `bt-core`'da gerekçesiz panik, rc dosyasına yazma, `bt-gpu`'nun platformsuzluğu (doğrudan bağımlılıkta ve kaynakta `objc2`/`dispatch2`/`block2`/`metal` yok), `bt-shell-common`'ın platform sınırı (doğrudan bağımlılıkta AppKit ailesi, `block2` ve `bt-shell-{macos,linux}` yok; kaynakta `objc2`/`dispatch2`/`block2` yalnız `watch`'ın macOS gövdesinde), bağımlılık uyarısı, genel iş akışı dosyalarının projeden bağımsızlığı |
| `.wgsl` değiştiyse | `make shader` — pipeline'ları kuran sınama (`wgsl_pipelines_build`: naga doğrulaması + Vulkan'ın immediate tabanıyla istenmiş device'ta pipeline kurulumu) |
| `assets/terminfo/*` değiştiyse | `make terminfo` — *henüz girdisi yok*: koşunca "not yet" deyip kırmızı düşer; tetiklenirse doğrulama "yeşil" değil "koşamadı"dır, `[~]` işaretlenir |
| `assets/bundle/*`, `assets/shell/*`, `crates/bateri` ya da `kur` hedefi değiştiyse | `make bundle` — **ürünü** denetler, düşerse çıkış 2; neyi denetlediği `Makefile`'ın `kur` yorumunda. Developer ID varsa imza zaman damgalı, yani ağ ister; notarization yalnız `make package`'te. `assets/shell/*` aynı satırda, çünkü betik de pakete kopyalanıp `cmp` ile denetleniyor ve `make check` yalnız **girdiyi** görüyor |
| PTY okuyucu, render thread ya da paylaşılan duruma dokunulduysa | `make test-race` — iki zamanlama profili, ikisi de geçmeli. TSan nightly ister ve araç zinciri pin'li değil: "TSan koşmadı" waive değil, bilinen sınırdır |
| Linux'ta derlenen bir crate (bugün `bt-core`, `bt-atlas`, `bt-gpu`, `bt-shell-common`) değiştiyse | `make linux` — Docker'da `clippy -D warnings` + `test`, `--locked` (`bt-gpu` Vulkan'da: `wgsl_pipelines_build` ve offscreen piksel sınamaları lavapipe'ta; `bt-shell-common`'da `jobs`'un gerçek PTY'si ve `child`'ın gerçek zsh'i; Mesa'nın `XDG_RUNTIME_DIR` satırları stderr gürültüsü, düşüş değil); ne sınadığı `Makefile`'ın `linux` yorumunda. Docker yoksa ya da daemon cevap vermiyorsa "SKIPPED" → `[~]`, **yalnız** o kolda; yerel rustc ile imaj etiketinin uyuşmazlığı "koşamadı" değil kırmızıdır (çaresi `tools/linux/Dockerfile`'ın `FROM` satırı) |
| pencereyi açan davranış değiştiyse (giriş, çizim, sekme) | `make smoke` — geçme ölçütü `CLAUDE.md` → Komutlar'da, jetonların anlamı `Makefile`'ın `smoke` yorumunda ve `Report::token_line`'da. Sayaçlar CPU'nundur: GPU'nun boyadığını `make check`'deki offscreen sınamalar, pencerenin görünürlüğünü hiçbiri kanıtlamaz. Başsız ortamda "SKIPPED" → `[~]`. Süreli koşuda pencere kayan seviyede açılıyor, yani öndeki uygulama kapıyı düşürmüyor; ekran uykudaysa pencere yine örtülü sayılır ve `frames=0` ortamdır, kod değil. `IDLE_FRAME_LIMIT` ölçülmüş bir sözleşmedir: değişikliği kod phase'lerinden **ayrı** commit'le iner |

**Araç zinciri pin'li değil** (Homebrew rustc; `rustup` ve
`rust-toolchain.toml` bilinçli olarak yok): `brew upgrade` sonrası yeni bir
clippy lint'i dokunulmamış kodu kırmızıya çevirebilir. `make check` sürümü
başta basar; kırmızıda önce sürüme bak.

## Dosya sınıfları

- **Derlenmeyen dosyalar** — `.tasks/`, `docs/`, `.claude/`, `CLAUDE.md`.
  Son yeşil kapıdan beri yalnız bunlar değiştiyse kapı yeniden koşulmaz.
- **Kilit dosyası** — `Cargo.lock` (bağımlılık bildirimi `Cargo.toml`'lar).
  Değiştiyse ya kayıtlı bir bağımlılık kararıdır ya da kusurdur; `make
  denetim` uyarır. Depoya **girer**, kirlilik değildir.
- **Türetilmiş dosya yoktur** (shader'lar WGSL kaynağı olarak gömülü).
- **Depoya girmeyenler** — `target/`, `*.metallib`, `*.dSYM`, `*.dmg`,
  `*.app`, `*.icns`, `*.iconset/`, `*.trace`, `.DS_Store`; kişisel/geçici
  örnekleri `~/.config/bateri` kopyası ve ekran kaydı. İkonun kaynağı
  (`assets/bundle/bateri.png`) **girer**.

## Riskli phase tetikleyicileri

Doğrulama tablosundan türer, ayrı tutulmaz: phase `make test-race`
(paylaşılan durum) ya da `make shader` (`#[repr(C)]` ↔ `.wgsl` düzeni:
`var<immediate>` blokları ve instance'ın vertex düzeni) gerektirdiyse, ya da
kilit dosyası değiştiyse.

## Set kapısı ekleri

- **`/audit` var**: set kapısında `/code-review`'dan sonra koşar, mercekleri
  aşağıda (Denetim mercekleri).
- **Gözle kontrolün yüzeyleri.** Hücre çizen üç yüzey var — ızgara, dock ve
  doldurma bandı — ve davranış değiştiyse devir mesajı üçünde de ne
  görüldüğünü söyler. Farklı görünüyorlarsa o fark bir **gerekçe** ister,
  "yapısal olarak böyle" değil. Ölçüt: kullanıcı bu değişikliği nerede
  *yaparak* görecek, yalnız nerede *okuyarak*? (023'te dört sahnenin dördü de
  ızgaraydı; emojinin **yazıldığı** dock listede yoktu ve kusuru kullanıcı
  buldu.)

## Teslim

- Tek branch: `main`; push komutu `git push origin main`. Dev branch,
  migration, container yok.
- Commit iletisi **Türkçe, emir kipinde, tek satırlık özet** ("Glyph
  atlasını tek dokuya topla ve tahliyeyi ölç").
- Geri alma her sette `git revert`.

## Pahalı karar sınıfı

`/plan-review` paneli yalnız birden çok yaklaşım varken **ve** seçim şunlardan
birine dokunuyorsa açılır (`/rfc` adım 6). Sınıf **değişecek dosyaya** göre
okunur, konunun adına göre değil (025 "shell entegrasyonu" diye panel açtı ama
betiğe hiç dokunmadı):

- yeni crate bağımlılığı (`Cargo.toml`)
- katman yönü — `bt-core`'a platform kütüphanesi, `bt-gpu`'ya terminal
  semantiği
- `Cell`'e alan (`bt-core/src/lib.rs`'in boyut assert'i)
- `TERM` / terminfo (`assets/terminfo/`)
- shell betiği (`assets/shell/`)
- her karede CPU hesabı (kare yolu: `Session::frame`, `bt-gpu`'nun encode'u)

## Jüri mercek notları

`/plan-review`'un üç jürisine ek olarak verilen, bu projeye özgü itiraz
konuları:

- Yeni bir crate bağımlılığı öneren plan → otomatik itiraz konusu; taban
  liste `CLAUDE.md`'dedir ve dışına çıkmak mimari karardır. Özellikle
  "kendi VT ayrıştırıcımızı yazalım" → `alacritty_terminal` neden yetmiyor?
- Renderer'a (`bt-gpu`) terminal semantiği koyan ya da `bt-core`'a platform
  kütüphanesi sokan plan → itiraz: katman yönü ve platformsuzluk `CLAUDE.md`'de.
- Her karede CPU tarafında hesap yapan bir efekt önerisi → itiraz: durma
  koşulu nerede, shader parametresi olarak GPU'ya taşınamaz mı, boşta sıfır
  kare korunuyor mu?
- Hücre yapısına alan ekleyen plan → itiraz: yan tablo neden olmuyor?
- `TERM` adını değiştiren ya da terminfo dağıtan plan → itiraz: özel
  terminfo SSH'daki uzak makinede yok (Metalterm #23); geri düşüş
  tasarlanmış mı, yoksa `xterm-256color` ile uyum mu korunuyor?
- Shell betiğine dokunan plan → üç kabuğu (zsh, bash, fish) birden
  kapsamalı; kullanıcı rc dosyasına yazan her yol KIRMIZI.
- Ölçüm iddiası taşıyan plan ("120 fps tutar", "gecikme düşer") → itiraz:
  ölçüm tahmin edilmez, `/measure` ile gösterilir; plan iddiayı hiç yazmaz.

## Otonom şerit ekleri

- **Uygulamanın süreç adı** `bateri`: `pkill`/`killall bateri` yasak,
  kullanıcı aynı anda kendi örneğini açık tutuyor olabilir.
- **Uzun komutlar** (ön planda, `timeout`'la): `make check`, `make bundle`,
  `make test-race`, `make linux`.
- **Yayın etkili sürprizler** (eskalasyon): beklenmeyen `Cargo.lock`
  değişimi, yeni bağımlılık ihtiyacı, ayar şeması / `TERM` / shell
  entegrasyonu etkisi, beklenmeyen ölçüm gerilemesi ya da boşta kare üreten
  bir yol.

## Denetim mercekleri

`/audit`'in mercekleri. Kuralların gerekçesi `CLAUDE.md`'dedir; aşağıdakiler
onların **kontrol edilebilir hâlleridir**. Mekanik yarı `make audit`'de.

1. **Bağımlılık kararı.** `make audit` `Cargo.toml`/`Cargo.lock` uyarısı
   verdiyse: kararın kaydı (`discussion.md` → `## Karar` ya da phase notu)
   var mı? Yoksa bulgudur ve kullanıcıya sorulur.
2. **Ayar ve tema şeması.** `settings.rs` ya da tema modeli değiştiyse: yeni
   anahtarın varsayılanı, eski anahtarın akıbeti (silinmez), `docs/AYARLAR.md`,
   yeniden yazma yolunun **bilinmeyen anahtarı koruduğu** round-trip sınaması,
   İngilizce `snake_case` adlar. Shell dosyası değiştiyse üç kabuk da (zsh,
   bash, fish) diff'te mi; değilse gerekçesi Uygulama Notları'nda mı.
3. **Ölçüm sahipliği.** Diff'te ölçüm sayısı taşıyan belge satırı ya da
   **ölçülmemiş iddia** var mı? Tek sahip `docs/OLCUMLER.md`. İstisnalar:
   `docs/ARASTIRMA.md` ve bir `const`'un doc'undaki türetme. `Measured`'ın
   doc'undaki sayılar istisna değil **emanettir** (`olcum.md`).
4. **Thread ve blokaj.** Render yolunda (kare üreten kod, display link
   callback'i) bloklayan çağrı var mı — PTY `read`, kilit bekleme, `sleep`,
   dosya G/Ç? AppKit çağrıları `MainThreadMarker` taşıyor mu? PTY okuyucu ile
   renderer arasındaki paylaşılan durumda iki kilit sırası kilitlenme
   üretebilir mi?
5. **Boşta sıfır kare ve animasyon durma.** Yeni animasyon ya da
   zamanlayıcının **durma koşulu** nerede? Kirli satır olmadan kare talebi var
   mı? Belirti sessizdir: uygulama çalışır, pil gider.
6. **Hücre boyutu ve shader/Rust düzen uyumu.** `Cell` değiştiyse `const`
   assert güncel ve gerekçeli mi, alan yan tabloya mı gitmeliydi? `.wgsl`
   yapısı değiştiyse Rust `#[repr(C)]` karşılığı alan sırası, tip ve hizalama
   ile aynı mı, WGSL'in kurallarıyla:
   `vec3`/`vec4` 16, `vec2` 8 hizalı ve yapının boyu en büyük hizaya
   yuvarlanıyor, yani Rust ikizinde görünmez dolgu **açık alan** olarak
   yazılı mı; `var<immediate>` bloğu bütçenin (`IMMEDIATE_BUDGET`, Vulkan'ın
   128 baytı) altında mı ve `set_immediates` ofset/uzunluğu 4'ün katı mı;
   uniform'a giden yapıda dizi adımı 16 mı; `VertexBufferLayout`'un ofsetleri
   ve `@location`'lar `#[repr(C)]`'nin `offset_of` assert'leriyle aynı mı?
7. **Belge ve dil.** Yeni crate'in `lib.rs` başlık yorumu var mı? Yorumlar
   "neden"i mi anlatıyor? Yorumlar Türkçe, **kod tanımlayıcılarının tamamı
   İngilizce** mi (`build.rs` dahil)? Türkçe kalan üç öbek yerinde mi (tanı
   metni, `Makefile` hedefleri, jeton satırının anahtarları) ve jeton
   **değerleri** İngilizce mi (`CLAUDE.md` → Dil)? İstisna: Linux/wgpu
   refactor'ının yazdığı ya da taşıdığı kodda yorumlar ve tanı metinleri
   **İngilizce** olmalı (`CLAUDE.md` → Dil, 040 `plan.md`). `#[allow]`
   gerekçeli mi?
