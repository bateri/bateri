# Phase 8 — OSC 52 yazma yönü

## Özet

Uzak uygulamanın OSC 52 ile panoya yazmasını ayar anahtarıyla aç: `Adapter`'da
düşen `ClipboardStore` olayını kilitsiz tek bir yuvadan genel panoya taşı.

_Requirements: R9, R5, R10_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[clipboard] osc52 = "off" |
  "copy"`, varsayılan `"copy"`. **Kapalıya düşer:** tanınmayan değer
  (`"paste"`, `false`, sayı) → `"off"` + tanı. Açılışta ayrıştırılamayan
  dosyada yükleyicinin döndüğü varsayılanlar `osc52`'yi `"off"` verir;
  canlı yenilemede ayrıştırılamayan dosya hiçbir şey uygulamaz (phase-4),
  kullanıcının son seçimi kalır. Fark fonksiyonu kapsar.
- **`crates/bt-core/src/session.rs`** —
  - Kendi OSC 52 tipi (`pub`, alacritty tipi sızmaz) `SessionOptions`'a ve
    terminal seçeneklerine girer; phase-4'ün `Config`'i tamamından kuran
    fonksiyonu onu `Osc52::{Disabled, OnlyCopy}`'ye çevirir.
  - `Adapter`: `ClipboardStore(Clipboard, metin)` → `Wake`'in yeni çağrısı;
    `ClipboardStore(Selection, …)` (`p`/`s`) yoksayılır — macOS'ta birincil
    seçim yok, genel panoya yazmak kullanıcının panosunu sessizce ezerdi.
    `ClipboardLoad` düşmeye devam eder (okuma yönü yok, `OnlyCopy` onu
    zaten üretmiyor).
  - `TestWake` (`:1739`) yeni çağrıyı uygular ve metni kaydeder.
- **`crates/bt-core/src/wake.rs`** — yeni çağrı: pano metni. Varsayılan gövde
  **yok** (uygulayan unutamasın). Doc: okuyucu thread'de ve `Term` kilidi
  tutulurken gelir; uygulayan bloklamaz, kilit almaz, `Session`'a girmez;
  kapanış sırasında gelen yazmanın kaybolması zararsız.
- **`crates/bt-shell/src/app.rs`** — `ShellWake`:
  - **Kilitsiz tek yuva, son yazma kazanır:** metin atomik takasla yuvaya
    konur; yuva boştu ise ana kuyruğa **tek** iş atılır (`child_exit`'in
    `exec_async` kalıbı), iş yuvayı boşaltıp `clipboard::copy`'ye verir.
    Durmadan OSC 52 basan bir uygulama ana kuyruğa sınırsız iş yığamaz.
  - Yuva mantığı AppKit'ten ayrık sınanabilir biçimde (pano parametre;
    `clipboard.rs`'in emsali).
  - Canlı `osc52` değişimi uygulayıcıdan terminal seçeneklerine gider.
- **`docs/AYARLAR.md`** — `[clipboard] osc52`, ne işe yaradığı (ssh'taki
  vim'in kopyası), okuma yönünün neden olmadığı, bozuk değerde kapalıya
  düşme.

## Kabul

- Ayrıştırma: `"copy"`, `"off"`, tanınmayan değer → `off` + tanı; açılışta
  ayrıştırılamayan dosya → `off`.
- `Config` kurucusu: `osc52` değişince `scrollback` korunur, tersi de
  (phase-4'ün sınaması iki alana genişler).
- `Adapter`: OSC 52 `c` dizisi `TestWake`'e metni ulaştırır; `p` dizisi
  ulaştırmaz; `osc52 = "off"` iken `c` dizisi de ulaştırmaz.
- Yuva: art arda çok sayıda metin → tek iş, son metin yazılır; dışarıdan
  verilen panoya (genel pano değil).
- `make test-yaris` iki profilde yeşil (okuyucu thread yolu).
- `make duman` jetonları değişmez.
- Göz: bateri içinde `printf` ile OSC 52 dizisi basmak panoya yazar; ssh
  üstünden vim'in (`clipboard` sağlayıcısı OSC 52) kopyası yerel panoya
  gelir; `osc52 = "off"` kaydedince aynı dizi panoya dokunmaz.

## Yayın Etkisi

- **ayar şeması** — `[clipboard] osc52` (`"copy"` | `"off"`, varsayılan
  `"copy"`); şablona `[clipboard]` bölümü girdi, `docs/AYARLAR.md`'nin
  kopyası sınamayla bağlı. Eski anahtar yok. Kapalıya düşme belgede: kabul
  edilmeyen değer (kayıt anında da), bölüm olmayan `clipboard`, açılışta
  okunamayan ya da geçersiz dosya, çözülemeyen ev dizini.
- Güvenlik davranışı: varsayılan açık (`copy`); arka plandaki uzak uygulama
  panoya yazabilir, `"off"` kapatır. Okuma yönü yok. `p`/`s` hedefleri de
  genel panoya yazar (**plandan sapma**, Uygulama Notları); boş metin
  yoksayılır. Boyut sınırı yok: dev bir kopya pencereyi yazma süresince
  durdurur (belgede).
- **`bt-core` pub API** — `Osc52`, `TerminalOptions::osc52`,
  `Settings::osc52`, `Settings::for_unusable_file`,
  `Wake::copy_to_clipboard` (varsayılan gövdesiz: dışarıda uygulayan varsa
  derleme kırılır; bugün tek uygulayan `ShellWake`).
- `CLAUDE.md` bugünkü hâl (`osc52` anahtarı, pano yolu), katman tablosunun
  `bt-core` / `bt-shell` satırları ve "Ayarlar" maddesi (`osc52` istisnası)
  güncellendi.
- `make duman` jetonları değişmedi: `kare=1 hucre=8 glif=6 kural=15`.
- **Bekleyen göz kontrolü** `[elle]`: bateri içinde
  `printf '\033]52;c;aGVsbG8=\a'` → `pbpaste` "hello"; ssh'taki vim/tmux'un
  kopyası yerel panoya gelir; `osc52 = "off"` kaydedince aynı dizi panoya
  dokunmaz. Uçtan uca zincir (`ShellWake` → ana kuyruk → genel pano) gerçek
  pencere ve kullanıcının panosu ister; parçaları sınamada (`Adapter` iletimi
  `TestWake`'le, yuva ve teslim benzersiz panoyla).

## Checklist

- [x] `[clipboard] osc52`, kapalıya düşme, fark
- [x] Kendi OSC 52 tipi; `Config` kurucusunda çeviri
- [x] `Wake` yeni çağrısı (varsayılan gövdesiz), doc; `TestWake`
- [x] `Adapter` kolu: `Clipboard` iletilir; `Selection` de iletilir (plan: yoksayılır — sapma, notlarda)
- [x] `ShellWake` atomik tek yuva → ana kuyruk → `clipboard::copy`
- [x] Test: ayrıştırma, `Config` koruması, `Adapter` iletimi, yuva birleştirmesi
- [x] `docs/AYARLAR.md`, `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`) — bulgu düzeltmelerinden sonra yeniden: `make hepsi` 0, `make test-yaris` iki profil 0, `make duman` üç koşu `kare=1 hucre=8 glif=6 kural=15`
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum) — 11 bulgunun 7'si düzeldi, 4'ü waive (notlarda)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **`p`/`s` hedefleri de genel panoya yazıyor** (plan ve Karar 5:
  yoksayılır; `/code-review` bulgusu). Neovim'in OSC 52 sağlayıcısı `*`
  kaydını `p` diye yolluyor (`runtime/lua/vim/ui/clipboard/osc52.lua`:
  `reg == '+' and 'c' or 'p'`) ve macOS'ta `*` ile `+` aynı pano:
  `clipboard=unnamed`'lı kullanıcının ssh'taki kopyası sessizce kaybolurdu.
  Kararın gerekçesi ("genel panoyu ezerdi") `c` için de geçerli, yani ayrım
  bir koruma sağlamıyordu. alacritty macOS'ta seçimi düşürüyor; bilerek
  ayrılındı. Ürün davranışı değiştiği için onay kapısında söylendi.
- **Açılışta okunamayan dosya ve çözülemeyen ev dizini de `off`** (plan:
  yalnız ayrıştırılamayan; ev dizini `/code-review` bulgusu): gerekçe aynı,
  dosyadaki `"off"` okunamıyor. Değer `bt-core`'da
  `Settings::for_unusable_file()` (varsayılanların sahibi orası), karar
  `bt-shell`'in `Loaded::at_launch`'ında ve `load_settings`'in ev dizini
  dalında; dosya **yoksa** düz varsayılan.
- **Kabul edilmeyen `osc52` kayıt anında da `off`**, `parse_keeping`'in
  "geçerli değeri tut" kuralının tek istisnası; bölüm olmayan `clipboard`
  da `off`. Tutma kuralının sebebi geri alınamayan uygulamaydı, kapatmak geri
  alınabilir; `"of"` yazımı panoyu açık tutmamalı.
- **Boş OSC 52 metni `Adapter`'da düşüyor** (planda yok): xterm'de panoyu
  temizler; son-yazma-kazanır yuvasında önceki gerçek metni boş metin ezmesin.
- **Yuva `clipboard.rs`'te ayrı tip** (`PendingCopy`; plan: `ShellWake`'in
  içinde): `AtomicPtr<String>` + `Box`, `put` iş gerekip gerekmediğini
  döner, `deliver(board)` alıp yazar. `ShellWake` onu `Arc`'la tutuyor —
  ana kuyruğun işi `'static` ister, `Wake` yalnız `&self` veriyor.
- **Yarış kapsamı** (planda yok): `race_set_terminal_options_and_frame`'in
  betiği OSC 52 basıyor ve seçici kipi ayrı ritimle açıp kapatıyor (pano
  kolunun koştuğu iddia ediliyor); yuvanın iki değişmezi (kuyrukta en çok bir
  bekleyen iş, son metin kaybolmaz) `race_pending_copy_put_and_take`'te —
  ilk hâlinin `runs <= TEXTS` iddiası hiç düşemiyordu (`/code-review`).
  `TestWake`'in durumu demetten yapıya geçti (üç alan).
- **Şablona `[clipboard]` girdi** (planda açık değil): varsayılanı olan her
  anahtar şablonda yazılı sözleşmesi (`template_is_the_defaults`).
- **Olumsuz sınamalar mutasyonla denendi** (hedef ayrımı kalkmadan önce):
  `Selection`'ı iletmek ve boş metni elememek Adapter sınamasını düşürüyordu
  (`["sel", "sel", "hello"]`, `["", "hello"]`). Bugünkü sınama boş metnin
  düştüğünü ve üç hedefin de geldiğini aynı kayıtta sırayla iddia ediyor.
- **`/code-review` waive'leri:**
  - *Metin boyu sınırsız, dev kopya ana thread'i durdurur* — tavan seçilmiş
    bir sayı ister; vte'nin tavansız tamponu ve kilit altındaki çözme bu
    değişiklikten önce de vardı (alacritty'nin varsayılanı `OnlyCopy`), sel
    gibi çıktı basan program pencereyi zaten meşgul edebiliyor. Kolun
    yorumunda ve `docs/AYARLAR.md`'de bilinen sınır.
  - *Yanlış yazılmış anahtar/bölüm (`osc_52`, `[clipbaord]`) sessizce açık
    kalır* — bilinmeyen anahtarın sessizliği Karar 1'in ileriye uyum kuralı;
    yazım hatası sınıfı her anahtarda aynı.
  - *Açılışta bozuk dosyanın uyarısı OSC 52'nin kapandığını söylemiyor* —
    alt başlık tek satır ve uzayan metin satır numarasını kesiyor; dosya
    bozukluğu zaten görünür, düzeltmek kopyayı geri getirir, belgede yazılı.
  - *Yalnız `osc52` değişen kayıt `set_options` + bir kare doğurur* — kayıt
    başına bir kare, boşta değil; `Changes`'i bölmek ikinci bir kapı olurdu.
- **`Wake::copy_to_clipboard` doc'u "ana thread ister" demiyor**
  (`/code-review`): `NSPasteboard` `objc2`'de `AnyThread`; gerçek kısıt
  `Term` kilidi altında yavaş iş yapmamak. Üretimde yazma ana kuyrukta kaldı
  (Cmd-C'nin yolu).
- **Duman:** ilk koşu `kare=4 glif=14 yuva=16` (006 phase-4c'de kayıtlı
  derleme sonrası gürültü); ardından üç koşu `kare=1 hucre=8 glif=6
  kural=15 yuva=13/2048`.
