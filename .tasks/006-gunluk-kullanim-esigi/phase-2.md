# Phase 2 — Pano: Cmd-C/V, bracketed paste

## Özet

Kopyala ve yapıştır çalışır; yapıştırma uygulamaya yapıştırma olduğunu söyler.

_Requirements: R2, R2.1, R2.2, R2.3, R6.1, R6.2_

---

## 1. `keyDown:`'da iki tuşluk dal (Karar 2 (a))

`crates/bt-shell/src/view.rs` — `view.rs:61-63` bekçisi bugün Command'lı her
tuşu yutuyor. Cmd-C ve Cmd-V o bekçiden önce ele alınır; diğer Command tuşları
yutulmaya devam eder. Menü günü (00X) bu dal **silinir** — kalıcı çözüm menü
seçicileridir, bu dal geçici köprüdür.

## 2. `NSPasteboard` köprüsü

`bt-shell`'de: Cmd-C → `core.selection_text()` (phase-1'in tek metin yolu) →
panoya yaz. Cmd-V → panodan oku → `paste()`'e ver. Panoya dokunan yalnız
`bt-shell` (AppKit); `bt-core` bayt görür, pano görmez.

## 3. `paste()` + 2004 sorgusu

`crates/bt-core/src/session.rs` — yapıştırma `session.write` yolundan
(`session.rs:811`) ama ham değil, yeni bir `paste()` ile sarılarak. DECSET
2004 **tutulmaz**; kiplik alacritty `Term`'in içinde, kilit altında sorgulanır.
Set ise `\e[200~…\e[201~` sarılır; değilse ham yazılır.

Güvenlik notu: ham yapıştırma vim/REPL'de satırları çalıştırır. Sarmalayan
`paste()`'tir; `session.write`'a doğrudan yapıştırma baytı verilmez.

---

## Uygulama Notları

- **Devralınan iş** (önceki deneme API hatasıyla düştü): `session.rs`'te
  `paste()`, `bracketed_paste()`, `TermMode` importu ve üç sınama zaten
  vardı; ikisi **kırmızıydı**. `bt-shell` tarafı (keyDown dalı, köprü) hiç
  yoktu.
- **Ölüm döşeği notu çürüdü.** "PTY kanonik modda satır tamponluyor, `\n`
  ekliyorum" teşhisi **yanlıştı**; deneyle ölçüldü: `od` 16 baytlık bloklar
  hâlinde okuyor ve blok dolmadan **döküm basmıyor** — 3 baytlık yük ("AB\n")
  sessizlik veriyor, 16+ bayt döküm veriyor. `\n` eklense de sorun sürüyordu.
  Çözüm: yük 16 baytı aşsın (`PASTE_PAYLOAD`, 20 bayt). Sarma dalında ayrıca
  **ikinci bir `paste`** gerekiyor: sarılı yük 32 bayt ve kuyruğu hem `od`'nin
  blok tamponunda hem PTY'nin satır tamponunda kalıyor; ikinci yapıştırma
  ikisini de akıtıyor (yorumu sınama gövdesinde).
- **Karar (phase-1'in devri): Cmd-C seçimi temizlemez.** Vurgu, kopyalanan
  aralığın ekrandaki kaydıdır; yeni bir tıklama zaten `mouseDown:` →
  `set_selection` ile üzerine yazıyor. Ek kod yok, davranış bu satırla kayda
  geçti.
- **`/code-review` bulguları** (ikisi gerçek kusurdu, ikisi sahte yeşil):
  - `copy()` yalnız `None`'a bakıyordu, oysa `selection_text()` boş
    hücrelerde `Some("")` döner (sürüklemesiz tık + alacritty'nin boş
    satırda `line_length()`'i sıfır) → Cmd-C **kullanıcının genel panosunu
    silerdi**. Kapı artık `Some("")`'ı da eliyor; yalnız-boşluk (`"   "`)
    elenmiyor, o meşru bir kopya.
  - Sarma **kendi iğnesini** korumuyordu: yükteki `ESC`/`ETX` süzülmüyordu,
    yani panoya `\x1b[201~` koyan bir süreç bölgeyi erken kapatıp gerisini
    uygulamaya **yazılmış girdi** olarak geçirebilirdi (Cmd-V'ye basar basmaz
    komut çalışır, Enter yok). Sarma dalı artık bu iki baytı süzüyor; emsal
    alacritty (`ActionContext::paste`). Sınama: `paste_strips_escape_and_etx…`.
  - `paste_empty_writes_nothing` okuyucu thread'le **yarışıyordu**: kip
    görünür olduğunda `dirty` bayrağı henüz dikilmemiş oluyor
    (`Event::Wakeup` `term.process()` sonrasında), gecikmiş kare "yapıştırma
    kare doğurdu" diye okunuyordu. Ölçüt tek kare değil **durulma** oldu.
  - Pano sınamasının başsız dalı **taze bir panoya** iddia kuruyordu (taze
    pano zaten boş — iddia düşemezdi, sahte yeşil). İki sınama da aynı yoldan
    sessizce atlıyor; asıl ölçüm canlı pano dalında.
  - `SAFETY` yorumu `NSPasteboard`'un ana thread istediğini söylüyordu;
    `objc2` onu `AnyThread` işaretliyor ve sınamalar işçi thread'den
    çağırıyor. `unsafe` gerekçesi statik erişimine çevrildi (gerçek sebep o).
  - `command_key` doc'u harf karşılaştırmasını Shift ile gerekçelendiriyordu
    ama Shift kapıda eleniyor; gerçek sebep **CapsLock** (`CapsLock` kapıda
    değil). Yorum koda uyduruldu, sınaması da yazıldı.
  - `generalPasteboard()` tuş bilinmeden alınıyordu: **her** Command vuruşu
    (yutulanlar dâhil) pano sunucusuna dokunuyordu. Artık yalnız eşleşen
    kısayolda alınıyor.
  - Sarma dalı yükü iki kez kopyalıyordu; `write_owned(Vec<u8>)` ile tek
    kopya kaldı. `paste()` **sahiplenen** bayt alıyor (`Vec<u8>`): ham dal
    artık sıfır ek kopya, çağıran `clipboard::read` zaten `String` sahibi.
- **`/simplify` bulguları**: tek gönderim noktası (`write_owned`),
  `wait_bracketed_mode` + `glyph_text` yardımcıları (kopyala-yapıştır
  deadline döngüsü ve üçüncü `filter_map` kopyası gitti), bayrak kapısı tek
  `intersects`, `command_shortcut` saf karar olarak ayrıldı, `spawn_od_child`
  satır içine alındı, pano canlılık çapası köprünün kendi gövdesinden.
- **`/audit` bulguları**: kilit sözleşmesi cümlesi yanına düşmüştü (modül
  doc'u "kilit yalnız `frame`/`resize`'da" diyordu, oysa seçim ve `paste`
  yolları da alıyor) — çağrı yerleri sayıldı, sıra sözü (`term` → `size`)
  korundu; `write_owned` doc'unun çelişen cümlesi düzeltildi (`Msg::Input`'u
  kuran üçüncü yer `Adapter::reply`, kapının dışında); `clipboard` modül
  sözleşmesindeki çelişkili iki cümle yazıldı; `bt-shell` crate doc'una
  `clipboard` eklendi; "kapi" yazım hatası. Mercek 1/2/3/6 temiz; 4/5/8/9
  ilgisiz (ayar, shell, animasyon, hücre/shader el değmedi).
- **WAIVE (bulgu, uygulanmadı):** *sarma olmayan dalda satır sonu
  normalizasyonu.* `alacritty` bracketed istenip kip kapalıyken `\n` → `\r`
  çeviriyor; bizim ham dal dümdüz yazıyor. **Uygulanmadı** çünkü
  `discussion.md` → Karar 3 bunu açıkça kullanıcının sorumluluğuna bırakıyor
  ("uygulamanın istemediğini terminalin bilemeyeceği bir şeyi terminal
  çözemez") ve kılavuz §3 dalı "değilse ham yazılır" diye tanımlıyor.
- **WAIVE (bulgu, uygulanmadı):** *kip sorgusu ile yazma arasında dar TOCTOU*
  (`/audit` mercek 7). Çocuk tam o pencerede 2004'ü kapatırsa sarma yanlış
  dala düşer. Pencere mikrosaniye mertebesinde; tek kilit tutuşuna almak
  `paste`'in büyük yük kopyasını `Term` kilidi altına sokardı — kazancından
  pahalı. Kayıt burada, karar bilinçli.

## Yayın Etkisi

- Cmd-C/V artık panoya dokunur; diğer Command tuşları yutulmaya devam eder.
- Yeni bağımlılık yok. Ayar şeması yok — okuma yönü (OSC 52) kapsam dışı.
- Ölçüm bekleyen iddia yok.

## Doğrulama Durumu (2026-09-13)

**Son hâl üzerinde tam koşu tamamlanamadı — engel ortamsal, kod değil.**
Bu makinede Xcode lisansı kabul edilmemiş: `/usr/bin/{make,cc,clang,git}` ve
`xcrun` "You have not agreed to the Xcode license agreements" deyip **exit
69** veriyor. Araç zincirinin kendisi yerinde — kapı yalnız shim'lerde:
`.../XcodeDefault.xctoolchain/usr/bin/clang` SDKROOT verilince sorunsuz
bağlıyor (bugünkü `bt-core` koşusu böyle alındı). Lisans kabulü `sudo`
ister ve **kullanıcının kararı**; ajan kabul etmedi, `xcrun`'u gölgeleyen bir
shim kurmayı da denemedi (reddedildi, doğru olan da bu).

| adım | sonuç |
|---|---|
| `cargo fmt --all -- --check` | ✅ exit 0 — **son hâl** |
| `cargo clippy -p bt-core --all-targets -- -D warnings` | ✅ exit 0, uyarı yok — **son hâl** |
| `cargo test -p bt-core` | ✅ **35 geçti** — son hâl (bağlayıcı ortam değişkeniyle) |
| `cargo clippy --workspace` | ❌ koşturulamadı — `bt-gpu/build.rs` `xcrun -sdk macosx -f metal` istiyor |
| `cargo test -p bt-shell` · `--workspace` | ❌ koşturulamadı — `bt-shell` → `bt-gpu` → `build.rs` aynı kapıda |
| `make hepsi` (exit 0) · `make duman` (`kare=1 hucre=8 glif=6 kural=15 … kapanis=clean pipeline=ok`) | ✅ lisans bozulmadan **önce** yeşildi; o koşu `/audit` düzeltmelerinden önceki hâle aitti |
| `cargo test -p bt-shell` (22 geçti) | ✅ `/code-review` düzeltmelerinden **sonra**, `/audit` düzeltmelerinden önce |

`/audit` sonrası delta: yorumlar + `paste(&[u8])` → `paste(Vec<u8>)` + çağrı
yerleri. Davranış aynı baytları yazıyor ve `bt-core` bu hâlde yeşil; ama
`bt-shell` tarafı son hâlde koşturulamadı, o yüzden kapı **yeşil sayılmadı**.

**Kapanış için gereken:** `sudo xcodebuild -license accept` (kullanıcı), sonra
`make hepsi` + `make duman` yeniden; bu tablo o zaman güncellenir. Not: ajan
bağlayıcıyı doğrudan clang'e çevirdiği için cargo'nun artımlı durumu birkaç
build script'ini yeniden bağlamak istiyor — lisans kabul edilince kendiliğinden
toparlanır, `cargo clean` gerekmez.

---

## Checklist

- [x] `keyDown:`'da Cmd-C/V dalı; diğer Command tuşları yutuluyor
- [x] `NSPasteboard` köprüsü `bt-shell`'de; `selection_text()` tek kaynak
- [x] `paste()` + 2004 sorgulu sarma; `session.write`'a ham yapıştırma yok
- [x] Test: 2004 setken sarma, değilken ham yazma (`paste_wraps…`, `paste_writes_raw…`; üstüne `paste_empty…` ve `paste_strips_escape_and_etx…`)
- [x] Test: Cmd-C seçili metni panoya yazıyor (canlı panoda round-trip; başsızda atlanır — `clipboard::tests`)
- [ ] `[elle]` göz kontrolü: kopyala-yapıştır turu (terminal içi + dış uygulama) — **kullanıcının işi**
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**) — **son hâl üzerinde koşturulamadı**: makinede Xcode lisans kapısı bağlamayı kilitledi (`cc`/`clang`/`make` exit 69). Ayrıntı ve ölçülen/kalan tablosu → `## Doğrulama Durumu`
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- Commit: {hash}
