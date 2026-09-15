# Ayarlar, tema ve font — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [discussion.md](discussion.md) ·
> [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md) ·
> [phase-4.md](phase-4.md) · [phase-5.md](phase-5.md) · [phase-6.md](phase-6.md) ·
> [phase-7.md](phase-7.md) · [phase-8.md](phase-8.md)

bateri artık kullanıcının ayarladığı bir terminal: `~/.config/bateri/settings.toml`
geçmiş uzunluğunu, temayı, fontu ve uzaktan kopyayı (OSC 52) belirler ve
**kaydedildiği an** uygulanır — kabuk ve içindeki program yaşamaya devam
eder. Açık ve koyu iki gömülü tema var, varsayılan olarak sistemin
görünümünü canlı izler; kullanıcı temaları `themes/{ad}.toml`'dan okunur.
Ayar ya da tema dosyasındaki hata pencerenin alt başlığında görünür. Ana
menü doğdu (About, Settings…, Quit; Edit ▸ Copy/Paste; View ▸ Theme ▸ ve
Cmd +/−/0). Dışarıya etki **kullanıcıya görünen davranış değişiklikleri**
(B.1), **iki yeni bağımlılık** (`toml_edit`, geçişli `toml_writer`; B.4) ve
**uygulamanın kullanıcının dosyasına ilk kez yazması** (yalnız Theme ▸,
yalnız `[appearance] theme`). Kullanıcının makinesinde taşınacak eski ayar
yok; `TERM` değişmedi; shader, terminfo ve paket girdileri el değmedi.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi        # rustc sürümü + fmt --check + denetim + clippy -D warnings + test
make test-yaris   # phase-2/3 tema kilidi, phase-4 set_terminal_options, phase-8 OSC 52 kolu okuyucu thread yolunda
make duman        # pencereyi açan davranış (menü, klavye, tema, font) değişti
```

`make shader`, `make terminfo` ve `make kur` **gerekmez**: `.metal`,
`bt-gpu/build.rs`, `assets/`, `crates/bateri` ve `Makefile` bu setin
aralığında el değmedi (`git diff --stat 24dbd5f^ HEAD` bu yollarda boş).
`/ship` Bölüm A'yı push'tan önce yeniden koşturur; aşağıdaki kutular son kod
üstündeki kayıtlı koşulardan.

### Beklenen çıktı

- `make hepsi` → exit 0. `make test-yaris` → exit 0 (iki zamanlama profili;
  `race_set_theme_and_frame`, `race_set_terminal_options_and_frame` ve
  `race_pending_copy_put_and_take` bu setle geldi).
- `make duman` → `kare=N hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke
  istek=I kapanis=clean profil=debug ornek=off pipeline=ok`, çıkış 0.
  Süreli koşu ayar dosyasını, izlemeyi, sistem görünümünü ve Theme ▸'nin
  dolmasını **görmez** (`app::Inputs`), yani jetonlar kullanıcının
  `~/.config/bateri`'sine ve makinenin açık/koyu moduna bağlı değil; sekiz
  phase'in hepsinde aynı kaldı. Derlemeden hemen sonraki ilk koşuda
  `glif=`/`kare=` yükselmesi görüldü (phase-2, phase-8), ardından gelen
  koşular tabana döndü — 006 phase-4c'de kayıtlı gürültü.
- **Bağımlılık:** `Cargo.lock` iki commit'te değişti — `0e0dbba` (phase-1,
  `toml_edit` ve geçişlileri) ve `867650c` (phase-7, `toml_edit`'in `display`
  feature'ı → `toml_writer`). İkisi de `discussion.md` → Karar'da kayıtlı
  kararın parçası.
- **Ölçüm:** bu set sayı taşımıyor; iki iddia ölçüm bekliyor (B.3).

### Set kapısı

`/code-review` `24dbd5f^..HEAD` üstünde (phase-8'in kendi incelemesinden
sonra), ardından `/audit`; düzeltmeler tek commit `ab60313`.

- **Düzelen:** CRLF satır sonlu `settings.toml`'da çok satırlı bir metin varken
  Theme ▸ dosyayı geçersiz TOML'a çeviriyordu; `theme = { … }` satır içi
  tablosu sessizce eziliyordu; boş tema dosyası (yerinde kaydın ortası)
  pencereyi koyu tabana çakıyordu; gömülü temayı gölgeleyen dosyanın eksik
  anahtarları koyu `bateri`'den geliyordu; izleme kaynakları okuma tanıtıcısı
  tutuyordu (`O_EVTONLY`'ye geçti, `libc`'nin kullanımı genişledi, yeni crate
  yok); `CLAUDE.md` dosyaya yazan tek yolu söylüyordu (Settings… da yazıyor).
  `/audit`: ölçülmemiş bir "donar" iddiası yumuşadı, `open_in_editor`'ın
  beklemesi kodda bilinen sınır.
- **Waive:**
  - *Settings…'in `open -t` yedeği ana thread'i bekletiyor* — yalnız `.toml`'u
    açan uygulama yokken, kullanıcının tıklamasında, odak editöre geçiyor;
    beklememek hatanın alt başlığa yolunu keserdi. Kodda bilinen sınır.
  - *`themes/Bateri-Light.toml` gibi büyük harfli gölgeleme menüde iki öğe
    gösterir* — nadir, görünür, veri kaybı yok; büyük/küçük harf duyarlı APFS'te
    doğru davranış tersi.
  - *US düzeninde Cmd-= Bigger'ı tetiklemiyor (Shift'siz)* — kullanıcının
    düzeni Türkçe-QWERTY-PC; B.2'nin tuş kontrolüne eklendi.
  - *Yazma yuvası her başarılı okumada boşalıyor* — reddin sebebi ayar
    yuvasında görünmeye devam ediyor (phase-7'nin bilinçli kararı).
  - *Karede tema iki kez okunuyor* — B.3'ün ölçümüne bağlı; iki okuyan da ana
    thread'de.
  - *OSC 52'nin kapalıya düşme kuralı üç yerde* — üçü ayrı karar noktası
    (değer, dosya, ev dizini), ilk ikisi sınanıyor.
  - *Canlı boyutlandırmada font bildirimi her olayda yeniden kuruluyor* — yalnız
    bildirim varken, küçük ayırma.

### Doğrulama Checklist

- [x] `make hepsi` yeşil — set kapısı düzeltmelerinden sonra (`ab60313`)
- [x] `make test-yaris` yeşil — `ab60313` üstünde, iki profil
- [x] `make duman` yeşil — `ab60313` üstünde iki koşu `kare=1 hucre=8 glif=6 kural=15`
- [x] Set kapısı — `/code-review` setin aralığında, `/audit` (aşağıda)
- [x] `Cargo.lock` değişimi kayıtlı karar (`toml_edit`)
- [~] `make shader` / `make terminfo` / `make kur` — koşulu doğmadı: girdileri el değmedi
- [ ] `[elle]` bekleyen göz kontrolleri (B.2)

## B. Yayın (doğrulamadan SONRA)

Sekiz phase'in `## Yayın Etkisi` bloklarından ve `git log 24dbd5f^..HEAD`'den
derlendi. Set dışı tek commit `525aa1d` (R3.4'ün metni ve phase-3'ün canlı
geçiş göz kontrolü); Yayın Etkisi'ne düşmemiş bir dış etkisi yok.

### B.1 Kullanıcıya görünen davranış (son hâl)

- **Ayar dosyası** `~/.config/bateri/settings.toml`: `[terminal] scrollback`,
  `[appearance] theme / light_theme / dark_theme`, `[font] family / size`,
  `[clipboard] osc52`. Dosya yoksa her şey varsayılan, uyarı yok. Anahtarlar,
  varsayılanlar ve hata tabloları `docs/AYARLAR.md`'de.
- **Kayıt anında uygulanır.** Kullanılamayan kayıt (geçersiz TOML, okunamayan,
  bir an yok olan dosya) hiçbir şeyi değiştirmez; kabul edilmeyen değer kendi
  anahtarını değiştirmez — **tek istisna `osc52`, kapalıya düşer**. Silinen
  dosya yeniden açılışa kadar etkisiz. `scrollback` küçültmesi geçmişi hemen
  siler.
- **Tema sistemi izler:** dosyasız kullanıcı açık modda artık `bateri-light`
  görür (bugüne kadar hep koyu). Koyu temada SGR 2'li (sönük) adlı renkler
  birkaç basamak kayar (kural zemine doğru karıştırma; bilinçli).
- **Kullanıcı temaları:** eksik anahtar gömülü `bateri`'den, gömülü bir temayı
  gölgeleyen dosyada o temadan gelir; boş tema dosyası kullanılamaz sayılır
  (kayıt ortasında pencere çakmaz).
- **Hata alt başlıkta:** ayar, tema, font ve yazma hataları pencere başlığının
  yanında İngilizce, kaynak başına; stderr'e de basılır. Tam ekranda
  görünmeyebilir (denenmedi).
- **Font:** `family` bulunamazsa SF Mono/Menlo ve uyarı; eşaralıklı olmayan
  aile uyarıyla kullanılır. Font değişince sütun/satır yeniden hesaplanır ve
  uzun satırlar yeniden sarılır.
- **Klavye ve menü:** **Cmd-Q artık uygulamayı kapatır ve açık programı
  sormadan kapatır** (kapatma onayı kapsam dışı); Cmd-H gizler; Cmd-C/V
  menüden geçer; Cmd , Settings… (dosya yoksa şablonla yaratır); About paneli
  açılıyor (006'nın borcu kapandı). View ▸ Bigger/Smaller/Actual Size geçici
  punto, dosyaya yazılmaz. AppKit'in pencere sekmeleri kapandı.
- **Uygulama kullanıcının dosyasına yazıyor:** yalnız View ▸ Theme ▸, yalnız
  `[appearance] theme`'in değeri; yorum, sıra, tanınmayan anahtar ve CRLF
  korunur, sembolik bağın hedefine yerinde yazılır, ayrıştırılamayan dosyaya
  yazılmaz. Bilinen yan etki: son satırda satır sonu yoksa eklenir. Bilinen
  sınır: yazma o anda hata verirse ya da süreç ölürse dosya boş kalabilir
  (phase-7 waive).
- **OSC 52 (uzaktan kopya) varsayılan açık:** terminaldeki program — ssh'taki
  vim/Neovim/tmux dahil — genel panoya yazabilir; `p`/`s` hedefleri de
  (Neovim `*` kaydını `p` yolluyor; plandan sapma, phase-8). Okuma yönü yok.
  `osc52 = "off"` kapatır. Bedeli: arka plandaki bir program panoyu
  değiştirebilir; dev bir kopya panoya yazılırken pencereyi durdurur.

### B.2 Göz kontrolleri `[elle]` — bekliyor

Yapılanlar phase dosyalarının notlarında (geçici `HOME`, System Events ile
menü tıklaması, `screencapture`, OSC 52 için gerçek pencereden `pbpaste`
yoklaması: `c` ve `p` kopyası geldi, `osc52 = "off"` iken gelmedi). Tuş ve
fare enjeksiyonu kullanıcının makinesinde başka uygulamaya düşebileceği için
şunlar bırakıldı:

1. **Cmd-C / Cmd-V / Cmd-Q / Cmd-T tuşları** (phase-6): seçip Cmd-C, başka
   uygulamaya Cmd-V; başka uygulamadan kopyalayıp bateri'de Cmd-V; Cmd-T
   kabuğa "t" yazmamalı; Cmd-Q kapatır.
2. **Cmd + / Cmd − / Cmd 0 tuşları** (phase-7; menü tıklamasıyla sınandı) ve
   bu tuşlarda View ▸ Theme ▸'nin boşuna yeniden dolmaması. US düzenli bir
   klavyede Cmd-= (Shift'siz) büyütmüyor olabilir (set kapısı bulgusu).
3. **bateri içinde vim açıkken tema değişimi** (phase-4): editörde `theme`
   değiştirip kaydet, vim'in ekranı yeni renklerle kalmalı.
4. **ssh üstünden gerçek kopya** (phase-8): uzak makinede Neovim (`"+y` ve
   `clipboard=unnamed` ile `yy`) ya da tmux copy-mode → Mac'te `pbpaste`;
   uygulama açıkken `osc52 = "off"` kaydedip aynı kopyanın gelmediğini gör.

### B.3 Ölçüm `[komut]` — bekliyor

`/measure` ile; sayı `docs/OLCUMLER.md`'ye işlenir, bu belge sayı taşımaz.

1. **Tema kilidinin kare süresine etkisi** (phase-2): `frame()` her dolu
   karede temanın yaprak kilidini alıyor, link de ikinci kez
   (`session.theme()`); `BT_FRAME_STATS` + `BT_SCROLL_TEST`'in `cpu`
   aralıkları, 006 tabanıyla karşılaştırma.
2. **Font değişiminde geçmişin yeniden sarılması** (phase-5): dolu 10 000
   satırlık geçmişte `Term` kilidi ana thread'de tutulurken reflow süresi.
   Bugünkü kancalar bunu ölçmüyor; araç kararı `/measure`'ın.

### B.4 Bağımlılık ve borç kaydı `[oto]`

- **`toml_edit`** (MIT OR Apache-2.0) — yalnız `bt-core`'da; `pub` API'de TOML
  tipi yok. Karar ve JSON/`toml`+`serde` reddi `discussion.md` → Karar.
  `CLAUDE.md`'nin bağımlılık satırı güncel.
- **Derlenen geçişliler** (`toml_parser`, `toml_datetime`, `toml_writer`,
  `winnow`, `indexmap`, `hashbrown`, `equivalent`) MIT seçilebilir; sekizi de
  `docs/YOL-HARITASI.md`'deki MIT bildirim borcuna adlarıyla yazılı.
  `Cargo.lock`'a giren `serde_core`, `serde_derive`, `syn`, `quote`,
  `proc-macro2`, `unicode-ident` hiçbir hedefte derlenmiyor (`cargo tree
  --target all -i serde_core` boş; kullanılmayan feature'ın kilit kaydı).
  `THIRD-PARTY-LICENSES.txt` değişmedi.
- **Kapanan borçlar:** ölçeğin iki kapısı (003; `sync_geometry` doc'u), About
  paneli (006).
- **Açılan sınırlar** (belgede): ana thread'de sınırsız dosya okuma (phase-1),
  yerinde yazmanın yarıda kalması (phase-7), OSC 52 metin boyu (phase-8),
  Settings…'in `open -t` beklemesi (kapı), tam ekranda alt başlık (phase-1),
  uygulama açıkken sonradan yaratılan ayar dizininin izlenmemesi (phase-4;
  Settings… kapatıyor).
- **`libc`** (var olan bağımlılık) `bt-shell`'de bekçinin yanında izlemenin
  `O_EVTONLY`'si için de kullanılıyor; `Cargo.lock` oynamadı, manifest yorumu ve
  `CLAUDE.md` katman tablosu güncel.

### B.5 `/ship` — `main` push'u `[oto]`

Bölüm A'yı yeniden koşturur, bu belgenin Yayın Checklist'ini damgalar ve
push eder. B.2 ve B.3 push'u bekletmez: koşulmadıkları için kutuları
işaretsiz kalır ve indeks "teslim bekliyor" der; ikisi de kapanınca set 🟢.

### Yayın Checklist

- [ ] B.2 göz kontrolleri (dört madde) — kullanıcı
- [ ] B.3 `/measure` (iki iddia) — kullanıcı isterse
- [x] B.5 `/ship` — push öncesi `make hepsi`, `make test-yaris` ve `make duman`
      (`kare=2 hucre=8 glif=6 kural=15`) yeşil

## Geri Alma

- **Kod:** set commit'leri tersten `git revert` (`ab60313` … `0e0dbba`);
  `Cargo.lock` revert'le `toml_edit`'siz hâline döner.
- **Kullanıcının dosyaları:** eski sürüm `~/.config/bateri/`'yi hiç okumaz;
  dosyalar yerinde kalır ve zararsızdır, silmek kullanıcının kararı. Theme ▸'nin
  yazdığı satır yalnız `[appearance] theme`.
- **Davranış:** revert Cmd-Q'yu yeniden yutulur hâle, Cmd-C/V'yi `keyDown:`
  köprüsüne, temayı sabit koyu palete ve OSC 52'yi düşen olaya döndürür.
- `TERM`, terminfo, shader ve paket değişmediği için onlarda geri alınacak
  bir şey yok.
