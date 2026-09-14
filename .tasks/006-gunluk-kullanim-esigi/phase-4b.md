# Phase 4b — Dock açılışı: ev dizini ve yerel

## Özet

Kabuk her açılışta kullanıcının ev dizininde başlar; süreç ortamında yerel
(`LC_ALL`/`LC_CTYPE`/`LANG`) yoksa çocuğa macOS'un dil/bölge ayarından bir
UTF-8 yereli verilir.

_Requirements: R4.4_

---

## Neden bu phase var

phase-4'ün `/code-review`'u buldu, `phase-4.md` → `## Uygulama Notları` →
"Dock açılışında kabuğun başladığı yer": LaunchServices süreci `cwd=/` ile
başlatıyor (probe'la doğrulandı) ve Dock launchd'nin ortamını veriyor —
orada `LANG` olup olmadığı doğrulanmadı; yoksa UTF-8 girişi bozulur.
`cargo run` ikisini de göstermez (çağıranın dizinini ve ortamını miras alır).

Kullanıcı 2026-09-14'te üç karar verdi (`discussion.md` → Karar 6 eki):
**006'ya girer** (phase-3b'den sonra, phase-5 ölçümünden önce);
**her zaman ev dizini** (istisnasız, `cargo run` dahil);
**yerel yoksa macOS dil/bölge ayarından**.

Referans: alacritty macOS'ta `main`'de koşulsuz `env::set_current_dir(home)`
yapıyor ve `macos/locale.rs` → `set_locale_environment()` ortam yereli
geçersizse `NSLocale`'in `languageCode` + `countryCode`'undan
`{dil}_{ülke}.UTF-8` kuruyor, geçersizse `LC_CTYPE=UTF-8`'e düşüyor
(orkestratör `master`'dan okudu). **İkisini de kendi sürecinde `set_var` /
`set_current_dir` ile yapıyor — biz yapmıyoruz** (`CLAUDE.md` →
"`tty::setup_env()` çağrılmaz"): ikisi de yalnız çocuğa verilir.

---

## 1. `bt-core`: çocuğun dizini ve ek ortamı

`crates/bt-core/src/session.rs` → `SessionOptions` ve `Session::spawn`.

- `SessionOptions`'a çocuğun çalışma dizini (`tty::Options.working_directory`)
  ve **ek ortam** girer. Ad ve tip senin kararın (`Option<PathBuf>`,
  `Vec<(String, String)>` gibi); pub API'de alacritty tipi görünmez.
- `None` → bugünkü davranış (miras). Sınamalar ve duman bugünkü gibi
  `None` verir; sonuçları dizine bağlı olmamalı.
- Ek ortam `TERM`/`COLORTERM`'ü **ezemez**; öncelik sırasını doc'a yaz ve
  sınamaya bağla.
- **Politika `bt-core`'da değil:** "ev dizini" ve "yerel" kararları
  uygulamanın (`bt-shell`) kararıdır; `bt-core` yalnız verileni çocuğa
  geçirir. `bt-core` `/usr/share/locale` gibi macOS yolu da bilmez.

## 2. `bt-shell`: ev dizini

`crates/bt-shell/src/app.rs` → kullanıcı oturumunun `SessionOptions`'ı.

- Çalışma dizini `$HOME`. `HOME` yoksa ya da boşsa `None` (miras) — ve bunu
  notlara yaz. `bt-core`'da `libc` doğrudan bağımlılık değil ve
  `getpwuid` için bağımlılık **eklenmez**; launchd GUI süreçlerine `HOME`
  veriyor, bu yol yalnız kenar durum.
- Duman/süreli koşunun sabit komutu da aynı kuralı alabilir ya da `None`
  kalabilir; seçimini gerekçesiyle yaz (duman sonucunun dizine bağlı
  olmadığını göster).

## 3. `bt-shell`: yerel

- **Karar saf bir fonksiyonda** ve sınanabilir: girdi olarak ortam okuyucu
  (`LC_ALL`, `LC_CTYPE`, `LANG`), sistemin dil/ülke kodu ve "bu yerel var mı"
  sorusu; çıktı çocuğa eklenecek tek bir ortam çifti ya da hiçbir şey.
  - Üçünden biri **boş olmayan** bir değerle tanımlıysa → hiçbir şey eklenmez
    (kullanıcının ortamına dokunulmaz; `cargo run` bu yoldan geçer).
  - Değilse ve `{dil}_{ülke}.UTF-8` sistemde varsa → `LANG={dil}_{ülke}.UTF-8`.
    `LC_ALL` değil `LANG`: en zayıf değişken, Terminal.app'in yaptığı; kabuğun
    rc dosyası kendi `LC_*`'ını üstüne yazabilsin (alacritty `LC_ALL` yazıyor —
    farkı doc'a yaz).
  - Yoksa (ör. İngilizce dil + Türkiye bölgesi → `en_TR.UTF-8` **yok**, bu
    makinede `ls /usr/share/locale` ile doğrulandı) → `LC_CTYPE=UTF-8`
    (alacritty'nin düşüşü; `/usr/share/locale/UTF-8` var). Mesajlar İngilizce
    kalır ama UTF-8 girişi çalışır.
- **"Var mı" sorusu:** `/usr/share/locale/{ad}` dizininin varlığı. `setlocale`
  ile sınama **yapılmaz** — kendi sürecimizin global yerelini değiştirir.
- **Sistem kodu:** `NSLocale::currentLocale()` → `languageCode` ve
  `countryCode`. `objc2-foundation`'a `NSLocale` feature'ı eklenecek: **yeni
  crate değil**, var olan bağımlılığın bayrağı. `Cargo.lock` **oynamamalı** —
  oynarsa dur, eskalasyon. `Cargo.toml` değişikliğini notlara ve commit
  gövdesine yaz (karar kaydı: `discussion.md` → Karar 6 eki).

## 4. Belge

`CLAUDE.md` → "`tty::setup_env()` çağrılmaz" maddesi çocuğun ortamını
sayıyor (`TERM`, `COLORTERM`); `LANG`/`LC_CTYPE` kuralı ve ev dizini **aynı
commit'te** oraya girer. `bt-shell` katman satırındaki platform kütüphanesi
listesi `NSLocale`'i kapsamıyorsa düzelt.

---

## Uygulama Notları

## Yayın Etkisi

---

## Checklist

- [ ] `SessionOptions`: çalışma dizini + ek ortam; `None` bugünkü davranış; ek ortam `TERM`/`COLORTERM`'ü ezemiyor
- [ ] `bt-shell` kullanıcı oturumu `$HOME`'da başlıyor (`HOME` yoksa miras, notta)
- [ ] Yerel kararı saf fonksiyonda; ortamda yerel varsa dokunmuyor; yoksa `LANG={dil}_{ülke}.UTF-8`, o yerel yoksa `LC_CTYPE=UTF-8`
- [ ] `objc2-foundation` `NSLocale` feature'ı; `Cargo.lock` değişmedi
- [ ] `CLAUDE.md` çocuk ortamı maddesi güncel
- [ ] **(phase-3b'den devir)** `CLAUDE.md` katman tablosunun `bt-core` satırı girdi kodlamasını (`input.rs`: DECCKM'e uyan oklar, tekerlek raporu) saymıyor — aynı commit'te ekle; gövdesi `phase-3b.md` → Uygulama Notları
- [ ] Test: çalışma dizini verilince çocuğun `pwd`'si o dizin; verilmeyince miras
- [ ] Test: ek ortam çocuğa ulaşıyor, `TERM`'ü ezemiyor
- [ ] Test: yerel kararı — `LANG`/`LC_ALL`/`LC_CTYPE` tanımlıysa hiçbir şey; boş değer tanımsız sayılıyor; geçerli sistem yereli → `LANG`; geçersiz → `LC_CTYPE=UTF-8`
- [ ] `[elle]` göz kontrolü: `make kur`, `bateri.app`'i Dock'tan aç → `pwd` ev dizini, `echo $LANG $LC_CTYPE` beklenen değer, `ğüşıöç İ` yazınca doğru görünüyor; `cargo run` ile terminalden açınca da ev dizininde başlıyor ve kendi `LANG`'ını koruyor
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**; `make kur` koşar)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
