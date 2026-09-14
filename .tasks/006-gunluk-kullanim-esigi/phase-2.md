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
    > **Aşıldı (seçim yarısı düzeltmesi):** sürüklemesiz tık artık `Some("")`
    > değil `None` verir (iki ucu eşit seçim boş). `Some("")` kolu yine
    > gerekli — boş bir satırın üstünde sürükleme onu üretiyor.
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

### Kullanıcı bildirimi — seçim bir harf fazla kopyalıyordu (2026-09-14)

**Bildirim:** "araba"nın "raba" kısmı fareyle seçilip kopyalanınca "araba"
geliyordu. **Kök neden** phase-1'deydi, kopyada değil: `set_selection` uçların
hücre **içindeki yarısını** hiç almıyor, alacritty'ye başlangıç için koşulsuz
`Side::Left`, bitiş için `Side::Right` veriyordu. Hedeflediği harfin hemen
soluna basan kullanıcı o pikseli bir önceki hücrenin sağ yarısına düşürüyor,
sabit `Left` o hücreyi aralığa katıyordu. Koordinat çevirisinde kayma yoktu
(`point_to_cell` sınamaları bunu gösteriyordu).

**Düzeltme:** `bt-core`'a `CellHalf` + `SelectionPoint { col, row, half }`
(alacritty `Side` `pub` API'ye çıkmadan); `bt-shell` yarıyı `x % cell_w`'den
okuyor. Kapıların bulduğu, aynı yolda yaşayan dört kusur daha kapatıldı:

- **Her seçimin ilk ve son hücresi hiç vurgulanmıyordu** (phase-1'den beri):
  `frame()` blok imlecin sınır istisnasına imlecin değil hücrenin kendi
  noktasını veriyordu. İmleç blok şekilliyken (varsayılan) vurgu her iki uçtan
  birer hücre kısa görünürdü; kabuğun imleç şeklini değiştirip değiştirmediği
  bilinmediği için kullanıcının ekranında olup olmadığı doğrulanmadı.
- **Sürüklemede boşa kare:** kare kapısı uçları karşılaştırıyordu; yarılı uçlarda
  aynı aralık iki uç çiftinden doğabildiği için her hücre sınırında ve her
  tıkta ekrana hiçbir şey eklemeyen kare isteniyordu. Kapı artık **ekranda
  çizilen aralığa** bakıyor (`visible_range`; `frame()` ve temizleme de aynı
  kaynaktan) — geçmişe kaymış bir seçim de çizili sayılmıyor.
- **Satır sonuna sürüklemede son harf kayboluyordu:** grid'in sağındaki
  kullanılmayan şeritteki olaylar yutuluyor, seçim son sütunun sol yarısında
  kalabiliyordu. Kenar dışı nokta artık son hücreye yapışıyor (sağda sağ yarı).
- **Geniş karakter:** yarı glyph'e uygulanıyor (baş hücre sol, spacer sağ);
  yoksa harf kopyalanırken yalnız yarısı ters videolanıyordu. Satır sonunun
  sağ yarısı ile alt satırın başının sol yarısı da aynı sınır sayılıyor —
  alacritty o ikisinin arasında üst satırın son hücresini seçiyordu.

Tek tık artık **boş** seçim (iki uç eşit → alacritty `is_empty`); phase-1'in
"tek tık kalıcı tek-hücrelik seçim" notu aşıldı ve orada damgalı.

**Reddedilen iki inceleme iddiası, kaynağa bakılarak:** "sürüklemede ters aralık
oluşuyor" — oluşmuyor, `range_simple` önce bitişi geri alıp uçlar eşitlenince
başlangıcı kaydırmıyor (sonuç ters aralık değil son hücre; yukarıdaki satır
sonu normalizasyonu onu kapattı). `clear_selection`'ın üretim çağıranı yok —
doğru ama phase-1'den kalma API, bu düzeltmenin kapsamı değil.

**Kanıt:** her davranış düzeltmesi önce kırmızı sınamayla yazıldı; beşi
(geniş karakter, spacer, satır sonu, imleç noktası, görünürlük) ve sol kenar
bekçisi (`%` → `rem_euclid`) **mutasyonla** ayrı ayrı doğrulandı. Bir sınama
mutasyonda yeşil kaldığı için düzeltildi: @2x sahnede -3 nokta iki kuralı
ayırt etmiyordu.

## Yayın Etkisi

- Cmd-C/V artık panoya dokunur; diğer Command tuşları yutulmaya devam eder.
- Yeni bağımlılık yok. Ayar şeması yok — okuma yönü (OSC 52) kapsam dışı.
- Ölçüm bekleyen iddia yok.

## Doğrulama Durumu (2026-09-13)

> **KAPANDI — tam koşu yeşil.** Engel ortamsaldı ve kullanıcı çözdü: Xcode
> lisansı kabul edildi, ardından eksik **Metal Toolchain bileşeni** indirildi
> (`xcodebuild -downloadComponent MetalToolchain`, 687,9 MB — Xcode 26'da
> `.metal` derleyicisi ayrı bileşen olmuş). Sonrasında **son hâl** üzerinde:
> `make hepsi` → exit 0 · `make test-yaris` → exit 0 · `make duman` →
> `kare=2 hucre=8 glif=6 kural=15 yuva=13/2048 kapanis=clean pipeline=ok`.
> `kare=2` beklenen aralıkta (005 phase-3 bu makinede 1 **veya** 2 ölçtü,
> `IDLE_FRAME_LIMIT=8`'in altında). Aşağıdaki tablo engelin sürdüğü andaki
> fotoğraf olarak **tarihsel kayıt** diye duruyor.

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
- [x] Doğrulama geçti — **kapı geri gelince son hâl üzerinde koşuldu**: `make hepsi` exit 0, `make test-yaris` exit 0, `make duman` → `kare=2 hucre=8 glif=6 kural=15 kapanis=clean pipeline=ok`. Engel ortamsaldı (Xcode lisansı + eksik Metal Toolchain), kullanıcı çözdü; tablo ve gerekçe → `## Doğrulama Durumu`
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- Commit: 97b9c2c
