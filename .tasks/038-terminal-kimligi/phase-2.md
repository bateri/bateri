# Phase 2 — `bateri://` şeması ve sekmeye odak

## Özet

`bateri` şemasını paket kaydına koy, `bateri://tab/<id>`'yi
`application:openURLs:` ile al ve o sekmeyi öne getir; kuralları belgeye
yaz.

_Requirements: R2.2, R3, R3.1, R3.2, R3.3, R4_

## Değişiklikler

- **`assets/bundle/Info.plist.in`** — `CFBundleURLTypes`: tek sözlük,
  `CFBundleURLName` = paket kimliği (`io.github.bateri.bateri`),
  `CFBundleURLSchemes` = `[bateri]`, `CFBundleTypeRole` = `Viewer`.
- **`Makefile`** (`kur`'un içerik denetimi; `key` yardımcısı dizin
  indeksli yolu kabul ediyor, geçici plist'te denendi) — `plutil -extract
  CFBundleURLTypes.0.CFBundleURLSchemes.0 raw` `bateri` değilse `fail "URL
  şeması (bateri) Info.plist'te yok"`; `kur`'un yorumundaki denetim listesine
  bir kalem.
- **`crates/bt-shell/src/app.rs`** —
  - Kimlikle pencere bulan yardımcı (`windows` listesinden,
    `TerminalWindow::tab_id` ile; mevcut `u64` kimlik yardımcısının ikizi).
  - `#[unsafe(method(application:openURLs:))]`: her `NSURL` için
    `absoluteString` → `bt_core::TabId::from_url`. Kollar Karar 5'te:
    `None` → hiçbir şey; pencere yok → `NSApp.activate()`; pencere var →
    `TerminalWindow`'da odak yöntemi. Sırayla, sonuncusu kazanır. Doc'ta
    değişmez: bu yol kabuğa bayt göndermez, pencere açmaz (Karar 6).
  - Soğuk başlatma: URL `applicationDidFinishLaunching:`'ten önce gelirse
    liste boş → yalnız `activate`; ilk pencere olağan yolundan **bir kez**
    açılmalı — kodlarken AppKit'in bu iki çağrının sırasını gerçekten nasıl
    verdiği `open bateri://tab/…` ile gözlenir ve Uygulama Notları'na yazılır.
- **`crates/bt-shell/src/window.rs`** — odak yöntemi (ör. `bring_to_front`):
  `isMiniaturized` ise `deminiaturize`, sonra `makeKeyAndOrderFront`
  (`selectTab:`'ın emsali: sekme grubundaki pencereyi seçili sekme yapıyor),
  sonra `NSApp.activate()`. `MainThreadMarker` yolu mevcut deyimle.
- **`CLAUDE.md`** — iki yere kural + tek cümle gerekçe + işaretçi:
  - "`tty::setup_env()` çağrılmaz" maddesi: çocuğa giden sabitler
    `TERM`/`COLORTERM`'e ek olarak `TERM_PROGRAM=bateri`,
    `TERM_PROGRAM_VERSION` (workspace sürümü) ve sekme başına
    `TERM_SESSION_ID` + `BATERI_TAB_URL=bateri://tab/<UUID>`; hepsi ek ortamı
    ezer; kimliği pencere doğarken `NSUUID` üretir, biçimi `bt-core`'un
    (`TabId`). Gerekçe cümlesi: miras kalan `TERM_PROGRAM=Apple_Terminal`
    `/etc/zshrc` üzerinden sarmalayıcının dizinine yazdırıyordu
    (`.tasks/038-terminal-kimligi/context.md` → Kanıt).
  - Bugünkü hâl paragrafına (`bt-shell`'in işleri ya da 026 sekme
    paragrafının yanı): **`bateri://` şemasının iki yolu** — `block/N`
    prompt'un iç OSC 8 çıpası, dışarıya hiç verilmez; `tab/<id>` sekmenin dış
    adı, `open` ile o sekme öne gelir, ölü kimlikte yalnız uygulama, bozuk
    biçimde hiçbir şey. **URL yalnız odaklar**: kabuğa bayt göndermez,
    komut koşturmaz, pencere açmaz — bu bir güvenlik değişmezi (şemayı her
    uygulama açabilir). İşaretçi `discussion.md` → Karar 5–7.
  - Katman tablosunda `bt-shell` satırının `objc2-foundation` listesine
    `NSUUID` (sekme kimliği).
- **`docs/YOL-HARITASI.md`** — "tıklanabilir bağlantılar" satırına tek cümle:
  `bateri://` bağlantıları `NSWorkspace`'e verilmeden yutulur (bugün
  çözüm onları zaten eliyor; Karar 7).

## Kabul

- `make kur` yeşil ve içerik denetimi şemayı buluyor; şablondan şema
  silinince `kur` "URL şeması" tanısıyla düşüyor (bir kez elle denenir, geri
  alınır).
- `make hepsi` ve `make duman` yeşil.
- Kurulu pakette (`make yukle` ya da `bateri-dev` sabit paketi) elle:
  - A sekmesinde `echo $BATERI_TAB_URL`; B sekmesine geç, `open <A'nın URL'i>`
    → A seçili sekme ve key.
  - Pencere küçültülmüşken ve uygulama ⌘H ile gizliyken aynı komut (başka
    uygulamanın terminalinden) → pencere geri gelir, A öne.
  - A kapatıldıktan sonra URL → uygulama öne, yeni pencere yok.
  - `open bateri://block/3` ve `open 'bateri://tab/xyz'` → hiçbir şey.
  - Uygulama kapalıyken `open bateri://tab/<eski id>` → uygulama açılır,
    **tek** pencere.

## Uygulama Notları

Bilinen sınırlar (kodlanırken değişmezse bu hâliyle kalır):

- `cargo run` sürecine URL gitmez: LaunchServices şemayı kayıtlı pakete
  yollar; debug sekmesindeki `open $BATERI_TAB_URL` kurulu uygulamayı açar
  ya da öne getirir ("sekme yok" kolu).
- Aynı şemayı ilan eden iki paket (kurulu `bateri.app` ve `bateri-dev`)
  varsa LaunchServices birini seçer; öteki kimliği tanımaz, yalnız öne gelir.
- tmux sunucusu ilk sekmenin `BATERI_TAB_URL`'ini ve `TERM_SESSION_ID`'sini
  taşır; sonradan başka sekmeden bağlanan istemci eski sekmeyi görür
  (Terminal.app'in `TERM_SESSION_ID`'siyle aynı sınır; çare kullanıcının
  `update-environment`'ı).

## Checklist

- [ ] `Info.plist.in` `CFBundleURLTypes`
- [ ] `make kur` içerik denetimi + yorum
- [ ] `TerminalWindow::tab_id` erişicisi (phase-1'den devredildi: ilk tüketicisi burada)
- [ ] Kimlikle pencere yardımcısı, `application:openURLs:`, odak yöntemi
- [ ] Soğuk başlatma sırası gözlendi, notu yazıldı
- [ ] `CLAUDE.md` üç yer, `docs/YOL-HARITASI.md` tek cümle
- [ ] Doğrulama geçti (`make hepsi` + `make kur` + `make duman`)
- [ ] Elle: Kabul'deki beş sahne kurulu pakette
