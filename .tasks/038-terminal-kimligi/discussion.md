# Terminal kimliği ve sekme URL'i — Tartışma

Biçim: karar-listesi. Ürün yüzü (adlar, link biçimi, `TERM_PROGRAM` ailesi)
kullanıcının onayıyla geldi; açık kalanlar yerleşim ve kenar davranışı.

## Karar 1: Kimliğin ve şemanın sahibi — `bt-core` mu, `bt-shell` mi?

- **A — `bt-core` sahibi.** Tipli bir sekme kimliği (`TabId`: UUID metnini
  doğrulayan kurucu, URL'yi yazan ve `bateri://tab/<id>`'yi çözen tek yer)
  ve `SessionOptions`'ta tipli bir alan; dört değişken `Session::spawn`'da
  `TERM`/`COLORTERM`'ün yanına, aynı "ezilemez" katmana giriyor. `bt-shell`
  yalnız UUID'yi üretir ve URL'den gelen kimliği pencerelerde arar.
- **B — `bt-shell` sahibi.** `child::identity_env(id)` dört çifti üretir ve
  `locale_env()` + entegrasyon zincirinin **sonuna** eklenir; ezilmezlik zincir
  sırasıyla. URL çözümü de `bt-shell`'de.

A'nın artıları: `bateri://` şeması tek crate'te (`block_id` zaten
`session.rs`'te), çözüm saf ve Linux kapısıyla sınanıyor, ezilmezlik
`TERM`'ün kuralının ta kendisi (ikinci bir öncelik kuralı yok). Eksisi: 15
`SessionOptions` kurucusuna bir alan. B'nin artısı `bt-core`'a dokunmaması;
eksisi şemayı iki crate'e bölmesi ve ezilmezliği bir zincirin sırasına
emanet etmesi.

→ ✅ A (aşağıda Karar).

## Karar 2: UUID nereden?

- `uuid` crate — **yeni bağımlılık**, taban listenin dışı; reddedildi.
- `libc::arc4random_buf` `bt-core`'da — macOS'ta var, Linux kapısında
  `getrandom(2)` ile `cfg` dalı ister; platformsuz çekirdeğe rastgelelik
  kaynağı sokmanın gereği yok.
- **`NSUUID`, `bt-shell`'de** — `objc2-foundation`'ın `NSUUID` **bayrağı**;
  yeni crate değil, `Cargo.lock` bayrak kaydetmiyor (emsal:
  `crates/bt-shell/Cargo.toml`'daki `NSLocale`/`NSURL` yorumları, 006 Karar 6
  eki). Üretim `TerminalWindow::new`'da, ivar olarak; süreç içi `u64` kimlik
  kalıyor (tüketicileri var, anlamı ayrı).

→ ✅ `NSUUID`.

## Karar 3: `TERM_PROGRAM_VERSION` hangi sürüm?

Bütün crate'ler `version.workspace = true`; `make kur` paket sürümünü
`cargo pkgid -p bateri`'den, yani aynı alandan okuyor. Değer `bt-core`'da
`env!("CARGO_PKG_VERSION")` ve `pub const` olarak dışarı açık; bir crate
günün birinde kendi sürümünü alırsa ayrışmayı `bt-shell`'deki bir sınama
yakalar (`bt_core`'un sabiti == `bt-shell`'in `CARGO_PKG_VERSION`'ı; ikisi de
`bateri` paketinin sürümüyle aynı alandan).

→ ✅ `bt-core`'un `CARGO_PKG_VERSION`'ı, bekçili.

## Karar 4: URL teslimi ve odak

- `application:openURLs:` (`NSApplicationDelegate`, 10.13+; `objc2-app-kit`
  0.3.2'de var) mi, `NSAppleEventManager` + `kAEGetURL` mi? İkincisi ayrı bir
  bayrak, el yazması Apple Event çözümü ve eski deyim; birincisi URL'leri
  `NSArray<NSURL>` olarak veriyor.
- Odak: pencere küçültülmüşse `deminiaturize`, sonra `makeKeyAndOrderFront`
  — `selectTab:`'ın emsali (`window.rs`), sekme grubundaki pencereyi seçili
  sekme yapıyor, `setSelectedWindow` gerekmiyor — sonra `NSApp.activate()`
  (macOS 14 API'si; taban 14).

→ ✅ `application:openURLs:` + bu sıra.

## Karar 5: Tanınmayan ve ölü URL

- Biçim `bateri://tab/<UUID>` değilse (`bateri://block/3`, bozuk UUID, fazla
  yol) → **hiçbir şey**: panik yok, pencere açılmaz, `activate` çağrılmaz.
- Biçim doğru ama o kimlikte sekme yok (kapandı, uygulama yeniden başladı) →
  **yalnız** `NSApp.activate()`. Yeni pencere **açılmaz**; bu kullanıcının
  onayladığı davranış ("sekme yoksa yalnız uygulama öne gelir"), boşluk
  değil — "kullanıcı tarafı" diye pencere açan bir yorum kararı çiğner.
- Soğuk başlatma (`open bateri://tab/X` uygulama kapalıyken): URL
  `applicationDidFinishLaunching:`'ten önce gelebilir; kimlik taze UUID'lerin
  hiçbirine uymaz, ilk pencere olağan yolundan **bir kez** açılır.
- Aynı çağrıda birden çok URL: sırayla işlenir, sonuncusu öne gelir.

→ ✅ yukarıdaki gibi.

## Karar 6: Güvenlik değişmezi — URL yalnız odak

Şema kaydedilince her uygulama (tarayıcı dahil) `bateri://…` açabilir. URL
yolu **hiçbir zaman** kabuğa girdi göndermez, komut koşturmaz, pencere
açmaz; tek etkisi var olan bir sekmeyi öne getirmek. Bu bir sözleşme
cümlesi olarak `CLAUDE.md`'ye girer — gelecekte "URL'den komut çalıştır"
isteği ayrı bir ürün kararıdır.

→ ✅

## Karar 7: OSC 8 ile çakışma

`bateri://block/N` prompt'un iç çıpası; bateri bugün hiçbir OSC 8
bağlantısını açmıyor (tıklama yolu yok), yani kayıtlı şema onunla bugün
**çakışmıyor**. Gelecekteki "tıklanabilir bağlantılar" seti bir
`bateri://` bağlantısını `NSWorkspace`'e vermeden yutmalı (kendimize
yollamanın anlamı yok); yutmasa bile Karar 5'in çözümü `block/` yolunu
sessizce eliyor — yanlışın yönü güvenli. Kural cümlesi `CLAUDE.md`'ye.

→ ✅

## Karar 8: Kapsam kenarları

- **Süreli koşu (`BT_RUN_SECONDS`)** — dahil, tek kol: dizin ve yerelin
  "istisnasız" emsali; değişkenler dosya okumuyor, jetonları oynatmıyor.
- **`LC_TERMINAL`** — kapsam dışı. iTerm2'nin icadı; tüketicilerin tanıdığı
  tek değer `iTerm2`, yani `bateri` değeri hiçbir özelliği açmıyor. Tek
  etkisi macOS'un varsayılan `SendEnv LANG LC_*`'ıyla ssh'ta öbür uca
  taşınmak; uzak tarafta onu okuyan bir tüketicimiz doğunca gelir.
- **Miras kalan yabancı kimlikler** (`ITERM_SESSION_ID`, `LC_TERMINAL`,
  `KITTY_WINDOW_ID`…) — kapsam dışı. Yalnız başka bir terminalden
  `cargo run`'da var (launchd'nin ortamında yok) ve alacritty'nin
  `tty::Options.env`'i yalnız **ekliyor**, silme bilmiyor.
- **ssh** — değişkenler yalnız yerel kabukta; ssh `TERM_PROGRAM`'ı
  taşımıyor. Terminal.app ile aynı hâl.

→ ✅

## Karar (2026-09-26, otonom akış)

Panel **koşmadı**: değişecek dosyalar pahalı karar sınıfına dokunmuyor
(`proje.md` → Pahalı karar sınıfı) — `Cargo.toml`'a yalnız var olan
bağımlılığın bayrağı (`NSUUID`, yeni crate değil, `Cargo.lock` oynamıyor),
`bt-core`'a platform kütüphanesi girmiyor, `TERM`/terminfo, shell betiği,
`Cell` ve kare yolu değişmiyor. Karar 1'in iki yolu yalnız kod yerleşiminde
ayrışıyor, kullanıcı ikisini ayırt etmez — teknik karar.

- **Seçilen:** Karar 1 A — `bt-core` kimliğin biçiminin ve ortamın sahibi
  (`TabId`, `bateri://tab/` yazımı ve çözümü, dört değişken `spawn`'da
  `TERM`'ün katmanında); `bt-shell` UUID'yi `NSUUID` ile üretir (Karar 2),
  URL'yi `application:openURLs:` ile alır ve odaklar (Karar 4–5). Sürüm
  `bt-core`'un `CARGO_PKG_VERSION`'ı, bekçili (Karar 3). URL yalnız odak
  (Karar 6); OSC 8 çakışması bugün yok (Karar 7); `LC_TERMINAL` ve yabancı
  kimliklerin silinmesi kapsam dışı (Karar 8).
- **Reddedilen:** B (`bt-shell`'de zincir sırası) — şemayı iki crate'e
  bölerdi ve ezilmezliği ikinci bir öncelik kuralına emanet ederdi; `uuid`
  crate — yeni bağımlılık, `NSUUID` aynı işi bayrakla yapıyor;
  `arc4random` — platformsuz çekirdekte `cfg` dalı; `kAEGetURL` — ek bayrak
  ve el yazması Apple Event çözümü; ölü sekmede yeni pencere — kullanıcının
  onayladığı davranışın dışı.
