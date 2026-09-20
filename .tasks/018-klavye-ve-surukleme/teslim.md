# Klavye ve dosya sürükleme — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [context.md](context.md) ·
> [discussion.md](discussion.md) · [phase-1](phase-1.md) · [phase-2](phase-2.md)
> · [phase-3](phase-3.md)

Metin girişi artık AppKit'in kendi yığınından geçiyor (`NSTextInputClient`),
yani Türkçe Q'da `~` ve `` ` `` **yazılabiliyor** — ikincisi bugüne kadar
hiçbir yolla yazılamıyordu. Option+←/→/⌫ kelime geziyor ve siliyor, Cmd+⌫
satırı siliyor, Finder'dan bırakılan dosyanın kaçırılmış yolu giriş satırına
düşüyor. `keyDown:` tek kapı olmaktan çıkıp **dört kollu bir arbitraja**
dönüştü ve ilk üç kol yığına hiç girmiyor: Cmd'li olay (izin listesi),
Shift+PgUp/PgDn (terminalin kaydırması) ve Control'lü olay (numpad Enter'ın
U+0003'ü ile Ctrl-Y'nin U+0019'u AppKit'e bırakılamazdı). Dışarıya görünen
yüzey `CLAUDE.md`'nin `bt-shell` satırı + giriş özeti, `keys.rs`/`view.rs`
doc'ları ve `docs/YOL-HARITASI.md`'nin "Klavye kalanları" maddesi. Ayar
şeması, tema, terminfo, shell betiği, app bundle ve `Cargo.lock` **hiç
değişmedi** — iki yeni feature (`NSTextInputClient`, `NSDragging`) var olan
bağımlılıkların bayrakları.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
```

`make shader` / `make terminfo` / `make test-yaris` / `make kur` bu sette
**tetiklenmiyor** (`plan.md` → Doğrulama tablosu): yeni `.metal` yok,
`TERM` sabit, paylaşılan durum yok (`consumed` ana thread ivar'ı), bundle
içeriği değişmedi.

### Beklenen çıktı

- `make hepsi` → exit 0 (kapıdan **sonra** koşuldu, `0c96228`; `bt-shell`
  126 → 127 sınama, `e0e4660` ile son hâl yine yeşil).
- `make duman` → **koştu ve yeşil** (2026-09-20, üç ardışık koşu `exit 0`):
  `kare=30 hucre=8 glif=6 kural=15 yuva=13/1984 yuk=smoke istek=4 icerik=3
  hareket=27 kayma=0 sessiz≈1740ms kapanis=clean profil=debug ornek=off
  pipeline=ok`. Bu sette sıradan bir kutu değildi: süreli koşu klavyeye
  **kör** (tuş sentezi yok) ama **sınıf kaydına kör değil** — hiçbir sınama
  `BateriView` üretmiyor, yani `NSTextInputClient`'ın 11 zorunlu metodunun
  eksiksizliğini iddia eden `define_class!` assertion'ı ilk kez orada koştu.
  **Patlamadı**, yani R1.1'in protokol uyumu artık iddia değil ölçüm.
  Klavye davranışının kendisi hâlâ elle turda (B.2).
- Ölçüm değişmedi; `docs/OLCUMLER.md`'ye bu setten giren sayı yok.

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `/code-review` faz 1 sonunda koştu (5 bulgu: 1 giderildi, 2 gerekçe,
      2 waive)
- [x] `/code-review` set aralığında koştu (8 bulgu: 2 giderildi, 1 orkestratör
      düzeltti, 5 waive)
- [x] `/audit` koştu (0 bulgu; `make denetim` temiz, mercek 1/3/4/5/7 temiz,
      2/6 ilgisiz)
- [x] `make duman` — **koştu, yeşil** (üç koşu `exit 0`); protokol uyumu geçti
- [ ] Elle tuş turu (B.2) — üç tablo
- [ ] Finder damlası turu (B.2 §3)

## B. Yayın (doğrulamadan SONRA)

### B.1 Commit'ler `[oto]`

| Phase | Commit | Ne getirdi |
|---|---|---|
| phase-1 | `05ff10c` | `NSTextInputClient`'ın 11 metodu, dört kollu `keyDown:`, ölü tuşlar |
| — | `c57ff68` | Kol sırası takasının plan'a işlenmesi + iki waive'in kaydı (orkestratör) |
| phase-2 | `def915f` | Option'ın Meta dizileri (`\eb`/`\ef`/`\e\x7f`), Cmd'nin tek istisnası (`\x15`) |
| phase-3 | `b316fff` | Finder damlası: `NSDragging`, saf `shell_quote` (`quote.rs`), `Session::paste` |
| kapı | `0c96228` | `/code-review` + `/audit`; `only_char` tek sahibe indi, damla asimetrisi doc'landı |
| — | `e0e4660` | Web adresi damlası dosya sanılmıyordu artık (`isFileURL`; orkestratör) |

**Geri alma birimi commit değil `set`:** girdi kaydının imzası (`KeyPress`)
phase-2'de değişiyor ve phase-1'in arbitrajının üstüne oturuyor, yani phase-1'i
tek başına geri almak derlemeyi kırar. Geri alınacaksa altı commit birlikte.

`/ship` doğrulama + `main`'e push'u kapsar. **Bu sette push edilmedi.**

### B.2 Gözle kontrol ve elle tur `[elle]`

Hermetik yarıları sınamalarda çivili; aşağıdakiler **yalnız gerçek pencerede**
görülebilir, çünkü ajan kabuğunda klavye ve damla sentezi yok.

**1. Kazanç tablosu** (`plan.md` R2, R3) — varsayılan zsh, Türkçe Q:

| tuş | beklenen |
|---|---|
| `Option+ü` sonra `Boşluk` | `~` |
| `Option+,` sonra `Boşluk` | `` ` `` — **bugüne kadar hiçbir yolla yazılamıyordu** |
| `Option+ü` sonra `n` | `ñ` |
| `Option+←` / `Option+→` | kelime geri / ileri |
| `Option+Delete` | kelime siler |
| `Cmd+Delete` | satırın tamamı gider |
| `Option+7` / `Option+b` | `{` / `∫` — **değişmemeli** |
| `Cmd+T`, `Cmd+Shift+A` | hiçbir şey; kabuğa harf yazılmamalı |

**2. Sıfır regresyon listesi** (R1.7) — doğruluğu artık AppKit'in hangi kolu
seçtiğine bağlı olan tuşlar: Enter, Tab, Esc, Backspace, oklar, Shift+Tab,
fn+Backspace, Shift+PgUp, **numpad Enter / Fn-Return** (U+0003 — yığın onu
eklerse her komut kesilir), **Ctrl+Shift+harf**, **Ctrl+Y** (U+0019'u
Shift+Tab ile paylaşıyor), **düz çok baytlı harf** (`ğ`, `İ`) ve
`^A/^C/^D/^E/^K/^U/^W`. Hiçbiri sınamayla çivilenemiyor — hiçbir test
"hangi olay `encode_key`'e ulaştı"yı göremiyor.

**3. Finder damlası:** tek dosya · **adında boşluk olan dosya**
(`İki\ Kelime` — kabuk tek argüman görmeli) · birden çok dosya · klasör
(yalnız yol, `cd` yok) · dock satırın sahibiyken damla.

**4. Press-and-hold — setin tek açık tasarım sorusu, ama artık yarısı
ölçüldü.** `bateri`'de `e`'yi basılı tut:

- **Popover çıkmıyorsa** bastırma tutuyor ve B.4 §2 kapanır.
- **Popover çıkıyor ve `é` seçince kabuğa `eé` gidiyorsa** düzeltme gerekli;
  iki çare var ve ikisi de senin kararın: (a) değeri uygulamanın **kendi
  kalıcı domain'ine** yazmak (iTerm2'nin yolu — `bateri`'nin plist'i,
  kullanıcının değil, yani R1.6'nın yasağının dışında), (b) `insertText:`in
  `replacementRange` uzunluğu kadar `\x7f` basması.

**Ölçülen yarı (2026-09-20):** set kapısının itirazı "registration domain en
düşük öncelikli halka, NSGlobalDomain ya da MDM eziyor olabilir" idi. Bu
makinede `defaults read -g ApplePressAndHoldEnabled` → **anahtar yok**, yani
ezecek bir halka bulunmuyor ve itiraz bu kurulumda konusuz.

**Ölçülmeyen yarı — testin asıl sebebi bu:** popover kararının değeri
`NSUserDefaults` üzerinden okuduğu. AppKit `CFPreferences`'a doğrudan
bakıyorsa registration domain'i büsbütün atlar ve ezilme sorusu anlamsızlaşır
— iTerm2'nin *kalıcı* domain değeri tutması tam da bu ihtimalin ipucu.
Kapatan sınama makineye bağlı olurdu; kapatan şey on saniyelik bir tuş.

### B.3 Ölçüm bekleyen iddialar `[komut]` — `/measure`

1. **Press-and-hold bastırması** (B.2 §4) — tek ölçüm, elle.
2. **Tuş başına `String` ayırma** (`characters().to_string()`): kazanç
   ölçülmedi, o yüzden sayı yazılmadı. Ertelemenin yolu yok — Cmd izin listesi
   ile `page_scroll` `chars`'ı yığın kolundan **önce** istiyor.
3. **(Kapandı)** `dropped_paths`'in eleme davranışı artık hermetik olarak
   çivili — `only_file_urls_become_dropped_paths`, benzersiz bir panoya bir
   `file://` ve bir `http://` yazıp yalnız birincisinin döndüğünü sınıyor.
   Kapının bulduğu belirti **iki yönde ölçüldü**: kademesiz hâlde dönen liste
   `["/tmp/bir dosya.txt", "/foo"]`, kademeli hâlde yalnız birincisi. Bekçi
   `e0e4660`'ı kapıya bağlıyor — o düzeltme set kapısından **sonra** geldiği
   için testsiz kalmıştı. B.2 §3'ün elle tablosunun yerine geçmiyor (gerçek
   Finder, gerçek dock sahipliği).

### B.4 Bilinen sınırlar (adıyla) `[elle]`

1. **Yığını atlayan hiçbir yazıcı bekleyen bileşimi yıkmıyor.** Üç kol:
   Control'lü tuş (phase-1 waive #3), ⌘⌫ (phase-2) ve Finder damlası
   (phase-3). Belirti hep aynı: `Option+ü` → o kol → `a` = `ã`. **Tek bilinen
   sınır olarak yazılıyor çünkü çare de tek** — bileşimi yıkacak ortak bir
   kapı, ve o kapı ancak preedit **çizildiğinde** (tam IME, kapsam dışı)
   dürüst olur: bugün preedit çizilmediği için bileşimi sessizce düşürmek
   ekranda hiçbir iz bırakmaz, oysa bugünkü hâlde aksan sonraki harfe biniyor,
   yani kayıp **görünür**. Aynı gerekçe ölü tuş + Backspace koluna da geçerli
   (phase-1 waive #1).
2. **`insertText:` `replacementRange`'i yoksayıyor** — B.2 §4'ün ölçümüne
   bağlı. Kapının itirazının **ezilme yarısı ölçüldü ve düştü** (bu makinenin
   `NSGlobalDomain`'inde anahtar yok); açık kalan şey popover kararının bu
   yolu okuyup okumadığı.
3. **IME sözleşmesi asgari:** `attributedSubstringForProposedRange:` hep
   `nil` (PTY'ye akmış baytı geri okuyacak belge yok; alacritty ve ghostty de
   aynı), `setMarkedText:` bilinmeyen tipte erken dönüyor. İkisi de
   `plan.md` → Kapsam Dışı'nın "Tam IME" maddesinin içinde; sözleşmenin tam
   hâli o yüzey doğduğunda yazılır.
4. **`draggingEntered:` koşulsuz `Copy` diyor**, `performDragOperation:`
   oturumsuz pencerede `false` — imleç kabul gösterip damla reddedilebiliyor.
   Koşul eklemek yanlış ölçüt olurdu: metot sürüklemenin **başında** koşuyor.
5. **Adında satır sonu taşıyan dosya** damlatılırsa iki parçası birleşmiş
   yazılır: `\` + satır sonu kabukta *satır devamı*. Kaçmamak daha ağır olurdu
   (ham satır sonu tamponda bir komut sınırı).
6. **Ctrl+Option+ok artık `\eb`/`\ef` gönderiyor** (eskiden düz ok): izin
   listesi ve Option sınıfı yanındaki değiştiricileri sormuyor, gerekçesi
   CapsLock — bayrağı tesadüfen açık olan kullanıcıda ⌘⌫ sessizce yutulurdu.
   R1.7'nin listesinde değil ve zsh'in o tuşta bağlaması yoktu.
   Aynı kuralın ikinci yüzü: **⌘⌥⌫ satırı siler**, kelimeyi değil.
7. **Temizlik borcu** (kapıda waive): ⌘⌫ izin listesi iki yerde
   (`reaches_terminal` + `encode_key`'in guard'ı). Klavye arbitrajını **elle
   tuş turu koşmadan** yeniden şekillendirmek riski kazancın üstüne çıkarırdı.
8. **Kapsam dışı kalanlar** (`plan.md`): Home/End, Ctrl+Shift+Tab,
   Ctrl+numpad Enter, tam IME, `[keyboard]` bölümü ve Option'ın topluca Meta
   olması, Cmd+←/→, metin/URL damlası, kitty klavye protokolü, DEC 1004.
   Home/End'in **şekli** `docs/YOL-HARITASI.md`'nin borç satırında bağlı.

### B.5 Belge etkisi `[oto]` — hepsi kendi commit'lerinde

- **`CLAUDE.md`** — `bt-shell` satırı (klavye yığını + sürükleme hedefi) ve
  giriş özeti: "Cmd'li olay yutulur" cümlesi izin listesiyle çelişir hâle
  gelmişti, Option'ın iki sınıfı da yazıldı.
- **`crates/bt-shell/src/keys.rs`** — kapsam-dışı listesi **bölündü,
  eksilmedi**: Option+Backspace kapsam içi; Ctrl+Backspace ve Option/Ctrl'lü
  ileri silme dışarıda; "Option-as-Meta" ibaresi yeniden yazıldı (gezinme
  tuşları Meta, harf değil).
- **`crates/bt-shell/src/view.rs`** — modül başlığı (sürükleme hedefi),
  `keyDown:`in dört kolunun gerekçeleri, `dropped_paths`'in üç kademeli
  elemesi.
- **`docs/YOL-HARITASI.md`** — "Klavye kalanları" maddesi 30 satırdan 20'ye
  indi ve içinde yalnız **borç** kaldı; durum cümleleri çıktı, çünkü durumun
  tek sahibi `.tasks/README.md`.
- **`docs/AYARLAR.md`** — **değişmedi** (yeni anahtar yok, bilinçli).

### Yayın Checklist

- [x] `make duman` yeşil — `NSTextInputClient`'ın 11 metotluk uyum
      assertion'ı gerçek pencerede patlamadı (2026-09-20)
- [ ] B.2 §1 kazanç tablosu
- [ ] B.2 §2 sıfır regresyon listesi
- [ ] B.2 §3 Finder damlası tablosu
- [ ] B.2 §4 press-and-hold → çıkan sonuca göre B.4 §2 kapanır ya da bir
      düzeltme açılır
- [ ] `/ship`

## Geri Alma

1. **Tamamı:** `e0e4660 0c96228 b316fff def915f c57ff68 05ff10c` sırasıyla
   revert. Ara basamak **yok** — `KeyPress` imzası phase-2'de değişiyor ve
   phase-1'in arbitrajının üstüne oturuyor (B.1).
2. **Yalnız sürükleme** ayrı alınabilir: `e0e4660` + `b316fff`'in `view.rs`
   damla metotları + `quote.rs`. Klavye yığını etkilenmez.
3. Ayar şeması, terminfo, tema, shell betiği ve app bundle değişmediği için
   **geri alınacak kullanıcı verisi yok**; `Cargo.lock` de değişmedi, yani
   bağımlılık grafiği aynen duruyor.
