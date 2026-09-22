# Input Dock ve prompt'un devri — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) ·
> [phase-3.md](phase-3.md) · [phase-4.md](phase-4.md) · [phase-5.md](phase-5.md) ·
> [phase-6.md](phase-6.md) · [phase-7.md](phase-7.md) · [phase-8.md](phase-8.md) ·
> [phase-9.md](phase-9.md) · [phase-10.md](phase-10.md)

Giriş satırı artık terminalin: pencerenin altında kendi yüzeyi olan bir **dock**
var, ZLE'nin tamponunu aynalıyor ve prompt'u kabuk değil terminal çiziyor
(`PS1`/`RPS1` sıfır görünür genişliğe iniyor, yerini bizim çizdiğimiz chevron
alıyor). Dışarıya görünen dört göç var ve dördü de kullanıcının ekranında:
**kurulu prompt kaybediliyor** (p10k/starship çizilmiyor), **`Last login:`
banner'ı kalkıyor**, giriş satırı ızgarada **çizilmiyor** ve imleç
dock'a taşınıyor. Prompt'unu geri isteyenin anahtarı
`[shell] integration = "blocks"`: sarmalayıcı kurulu kalır, yani **bloklar ve
işaretler yaşar**, ama dock açılmaz. Kabuk
betiği ve tel biçimi değişti (`make kur` zorunlu), jeton satırı **değişmedi**
ve yeni bağımlılık yok. `TERM`, terminfo, tema biçimi ve shader dokunulmadı.

> **Gözle kontroller 2026-09-22'de kullanıcı tarafından yapıldı**, tamamı ve
> sorun bildirilmedi. Komut şeritleri (`make kur`, `make test-yaris`,
> `make duman`) aynı gün koştu ve yeşil.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make kur          # assets/shell/* ve paket geometrisi değişti
make test-yaris   # phase-1 ve phase-7 paylaşılan duruma dokundu
make duman        # gerçek pencere ister; başsız kabukta yanlış tanıyla düşer
```

`make hepsi` her phase'in kapısıydı ve phase-10'da da yeşil. Koşullu hedeflerin
üçü de **tetiklendi**:

- **`make kur`** — `assets/shell/zsh/bateri.zsh` üç kez değişti (phase-2 aynayı
  akıttı, phase-5 prompt'u devraldı, `b8152c2` altıncı gövdeye `$KEYMAP`
  ekledi) ve phase-9 paket geometrisini oynattı (dock payı `DOCK_ROWS * cell_h
  + 2 * pad`). Hedef kopyayı `cmp` ile denetliyor, yani eski betikle kalan bir
  paket sessizce yanlış çalışırdı.
- **`make test-yaris`** — phase-1 okuma yolunu (PTY tarayıcısı) ve phase-7 kilit
  sırasını (`alt_screen` bayrağı `Term` kilidi altında yayınlanıyor, resize ana
  kuyruğun sonraki turunda) değiştirdi.
- **`make duman`** — pencereyi açan davranış iki yerden değişti: kabuk doğurma
  politikası (`login -q`, phase-8) ve caret'in tek animatöre bağlanması
  (phase-9). İkincisi hareket saatine dokunuyor.

**`make shader` gerekmiyor:** `.metal` dosyalarına hiç dokunulmadı. Dock ikinci
bir `setViewport` ve ikinci bir uniform bağlaması, ikisi de Rust tarafında;
caret'in tek instance'a inmesi de yuva seçimi, pipeline değişikliği değil.
**`make terminfo`** girdisi yok.

### Beklenen çıktı

- `make hepsi` — `denetim: temiz`, clippy uyarısız, beş crate yeşil.
- `make kur` — `bateri.app` kuruluyor, içerik denetimi (Info.plist, ikon,
  lisans, `Contents/Resources/shell`) temiz.
- `make duman` — üç sayaç **oynamamalı** (`hucre=8 glif=6 kural=15`) ve
  `yuva`'nın kullanılan yarısı da `13`'te kalmalı. Reçete `/bin/sh` koşuyor,
  yani **dock almıyor** (`SessionOptions::dock` oturum doğarken kararlaşıyor) ve
  blok da yok; chevron sprite'ı hiç istenmiyor. `RULE_RESERVE`'ün 6'dan 7'ye
  çıkması yalnız karakterlere kapalı payı büyütüyor, kurallar tembel.
  Setin son kayıtlı koşusu (phase-6):

  ```
  kare=29 hucre=8 glif=6 kural=15 icerik=2 hareket=27 sessiz=1749.36ms kapanis=clean
  ```

  `icerik` `IDLE_FRAME_LIMIT`'in (8) altında, `sessiz` tabanın (868 ms) üstünde,
  `kapanis=clean`. **`login -q` bu koşuya uğramıyor:** süreli koşu kendi sabit
  betiğini veriyor (`smoke_shell`), yani banner değişikliği jetonları
  etkilemiyor.

### Set kapısı: 8 bulgu, 5'i düzeltildi

`/code-review` (aralık `5e9c915^..HEAD`) ve `/audit` altı mercekle koştu;
`make denetim: temiz`. Kapı commit'inde **düzeltilenler**:

| # | ne | nerede |
|---|---|---|
| 2 | **Caret sahipliği ikiye ayrılmıştı** — `dock::render` yüklemi ikinci kez çağırıyor, `frame()`'in üç ön koşulunu bilmiyordu; bayat aynada ızgara imlecini gösterirken dock da sahipleniyor, çizen taraf dock'u seçince taze satır **caret'siz** kalıyordu | `Cursor::caret_in_dock` sınırdan geçiyor, `Session::dock` argüman alıyor; regresyon sınaması `a_stale_mirror_leaves_the_input_line_in_the_grid`'e eklendi (eski davranışa çevrilip kırmızı olduğu doğrulandı) |
| 5 | `dock_top_px` dock'suz pencerede de yazılıyordu; formül orada tam `viewport_height` verir ve dibe değen caret çizilmeyen yuvaya düşerdi | `link.rs` yalnız `dock_rows > 0` iken yazıyor, `clear`'ın sonsuzu duruyor |
| 6 | Tazelik kapısının iki yarısı **farklı mürekkep ölçütü** kullanıyordu (ızgara `' '`, ayna `is_whitespace()`); satır sonu NBSP kapıyı kalıcı "bayat"ta bırakırdı | ayna ızgaraya hizalandı (`shell.rs`) |
| 7 | `suppress_floor <= to` bir **totoloji**ydi ve yorumu kapının kontrol etmediği bir durumu anlatıyordu | `suppress_to.is_some()`; gerçek kapı atlama döngüsündeki `from.max(suppress_floor)` |
| 8 | Dock geometrisi sınamasının yorumu `dock_row_gap`'i atlıyordu; doğru ve yanlış formül aynı satır sayısına yuvarlandığı için sınama yeşil kalıyordu | yorum yeniden türetildi (`app.rs`) |

Ayrıca phase-9'un `push_cursor → push_caret` yeniden adlandırmasından kalan
**doc drift** kapandı: iki yetim doc bloğu, yedi kırık intra-doc link ve
`cell.metal` ile `renderer.rs`'te artık var olmayan bir mekanizmayı
(`CursorBlock::shifted_y`) anlatan iki yorum.

**Ertelenen üçü** aşağıda: #1 doğrulama listesinde **bloklayıcı**, #3 ve #4
`docs/YOL-HARITASI.md` → "Sete bağlanmamış borçlar"da kayıtlı.

### Doğrulama Checklist

`make hepsi` dışındaki her şey **kullanıcının gerçek penceresinde** koşar —
ajanın kabuğunda `make duman` yanlış tanıyla kırmızı düşüyor.

- [x] `make hepsi` yeşil (kapı commit'i; `make shader` de koştu, `.metal` yorumu değişti)
- [x] **Üç kademe** (phase-10): `integration = "auto"` dock'lu ve bizim
      prompt'umuzla; `"blocks"` dock'suz, **sizin prompt'unuz** ve bloklar
      çalışıyor; `"off"` hiçbiri. Kullanıcının iki karesi tekrar edilince
      ekranda **tek** prompt olmalı
- [x] Dosyada kalan `prompt = "shell"` satırı davranışı değiştirmiyor ve alt
      başlıkta emekli olduğu söyleniyor
- [x] `make kur` yeşil (2026-09-22, çıkış 0)
- [x] `make test-yaris` yeşil (2026-09-22)
- [x] `make duman` yeşil ve jetonlar yukarıdaki sözleşmeye uyuyor
- [x] **Gözle: açılış** — ızgara tertemiz (banner yok), caret dock'ta, ızgarada
      imleç yok (phase-8)
- [x] **Gözle: yazma** — dock'ta caret sağa sola süzülüyor, ızgaradakiyle aynı
      stil (phase-9)
- [x] **Gözle: `sleep 5`** — caret ızgaraya **kayarak** çıkıyor, bitince
      dock'a **kayarak** iniyor; kayma ortasında pencere uyumuyor (phase-9)
- [x] **Gözle: ZLE yüzeyleri** — Tab tamamlama, Ctrl-R, `CORRECT` istemi
      (phase-4)
- [x] **Gözle: kurulu prompt** — p10k/starship kuruluyken `"auto"` prompt'u
      devralmış; `"blocks"` onu olduğu gibi geri veriyor **ve blok şeritleri
      hâlâ çiziliyor** (phase-5 + phase-10)
- [x] **Gözle: alternatif ekran** — vim/htop/`less` gir-çık; dock kalkıyor,
      çıkışta geri geliyor, `git log` dock'u koruyor (phase-7)
- [x] **Gözle: punto** — Cmd +/−/0 ile dock payı ölçekleniyor (phase-9)
- [x] **Gözle: chevron** — `>` fonttan bağımsız; font değişince şekli
      değişmiyor, ızgaranın blok işareti de aynı şekil (phase-9)

## B. Yayın (doğrulamadan SONRA)

### B.1 Paketi kur `[komut]`

```sh
make kur
```

Kabuk betiği pakete kopyalanıyor ve `cmp` ile denetleniyor. **Açık pencereler
eski betikle koşmaya devam eder**; tel biçiminin altıncı gövdesi (`$KEYMAP`)
bu yüzden opsiyonel — yokluğu yükü bozmuyor, yalnız yapıştırmanın dar
istisnasını kapatıyor (güvenli yön).

### B.2 Bekleyen ölçümler `[komut]`

```
/measure
```

Üç iddia bekliyor ve **üçü de bugün kapatılamaz**, çünkü ölçecek kanca
(`BT_INPUT_LATENCY_SAMPLES`) henüz yok — borç `docs/YOL-HARITASI.md`'de:

| iddia | sahibi | belirti |
|---|---|---|
| tuş başına tel maliyeti (zsh base64 + `vte` `unhandled`) | phase-1, R6.2 | yazarken gecikme |
| tuş başına O(n) bayt (beş değişken) | phase-2 | aynı |
| prompt başına `git rev-parse` fork'u | phase-6 | büyük depoda prompt gecikmesi |

Sayı yazılmadı; `docs/OLCUMLER.md`'ye bu setten hiçbir değer girmedi.

### B.3 Göç notu `[elle]`

Sürüm notuna dört kalem girer — dördü de kullanıcının ekranında görünür:

1. **Kurulu prompt çizilmiyor.** Varsayılan `[shell] integration = "auto"`
   prompt'u terminale devrediyor ve satırı dock'a taşıyor. Geri dönüş
   `integration = "blocks"`: blokları ve işaretleri **korur**, yalnız dock'u
   bırakır. `"off"` de prompt'u geri verir ama blokları da öldürür, yani
   orantısız. İkisi de **sonraki oturumda** geçerli.
   **`[shell] prompt` emekli** (phase-10): eskiden ayrı bir anahtardı, ekranda
   iki prompt üretiyordu; dosyada kalması zararsız ama okunmuyor ve bir uyarı
   görünüyor.
2. **`Last login:` kalktı.** `login(1)` her zaman `-q` alıyor. Geri isteyen
   için bugün anahtar **yok**.
3. **Giriş satırı ızgarada çizilmiyor**, dock'ta. Ayna gösteremiyorsa
   (`Unavailable`), ZLE satırı bıraktıysa (`Idle`) ya da ayna bayatsa bastırma
   **yok** — satır ızgarada kalır.
4. **Bilinen sınırlar** (hiçbiri regresyon değil, hepsi kayıtlı): p10k
   **instant prompt** kancalarımızdan önce koşup ilk kareyi kendi prompt'uyla
   çiziyor; ZLE'nin `BUFFER` olmayan çıktısı (tamamlama listesi, `menu-select`,
   `bck-i-search`, `zle -M`) aynada yok, ızgaraya düşüyor; `zle -I` ile basılan
   iş bildirimi çıpa satırını bir satır kaydırabiliyor; `preexec` koşmayan
   yollarda (Ctrl-C, boş satıra Enter) çıpa bir sonraki `PS1` genişlemesine
   kadar açık kalıyor (yön güvenli: fazladan şerit işareti); sessizce ölen bir
   betik dock caret'ini hareketsiz bırakıyor (yanıltıcı ama **görünür**).

### B.4 Belgeler `[oto]`

Set içinde **yapıldı**, ayrı adım gerekmiyor: `CLAUDE.md` dokuz phase boyunca
kodla aynı commit'lerde güncellendi, `docs/AYARLAR.md`'ye `shell.prompt` ve
kurtarma satırı girdi, `docs/YOL-HARITASI.md`'ye iki borç yazıldı (dal için
daemon/önbellek, `psvar[9]` kaybının büyüyen bedeli).

### Yayın Checklist

<!-- `/ship` bekleyen manuel adımları BU başlık altında arar. -->

- [x] B.1 `make kur` koştu (2026-09-22, çıkış 0)
- [~] B.2 `/measure` — üç iddia **kapatılamaz** (kanca borcu) ve artık bu checklist'te taşınmıyor: bekleyen iddiaların tek listesi `docs/OLCUMLER.md` → `## Bekleyen iddialar`. Kutuyu `[ ]` tutmak seti kapatılamaz bir kalem için süresiz 🔨'da bırakıyordu
- [x] B.3 göç notu yazıldı — bu dosyanın başındaki dört göç maddesi ve `docs/AYARLAR.md`'nin emekli anahtar uyarısı

## Geri Alma

- **Tamamı:** `git revert 5e9c915..HEAD` — set tek bir zincir, dışarıya açtığı
  tek şema kalemi `[shell] prompt`.
- **Yalnız prompt devri:** ayar dosyasına `[shell] prompt = "shell"`. Kod geri
  alınmadan kullanıcının prompt'u geri gelir; dock kalır. **Sonraki oturumda**
  geçerli.
- **Yalnız dock:** `[shell] integration = "off"`. Dock, bloklar ve bastırma
  birlikte kalkar — orantısız ama tek anahtarlı çıkış.
- **Ayar şeması:** `prompt` anahtarı **eklendi**, hiçbir anahtar silinmedi;
  eski `settings.toml` aynen okunur (eksik anahtar varsayılana düşer, bilinmeyen
  anahtar korunur). Geriye dönük okuma sorunu yok.
- **Kabuk betiği:** paketten eski sürüme dönmek `make kur`'u eski commit'te
  koşmaktır. Tel biçiminin altıncı gövdesi opsiyonel olduğu için **yeni
  terminal + eski betik** çalışır; ters yön (eski terminal + yeni betik) de
  çalışır, alanı tanımayan çözücü onu yoksayar.
- **`TERM`, terminfo, tema biçimi, shader:** dokunulmadı, geri alınacak bir şey
  yok.
