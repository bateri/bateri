# Proje profili — bateri

İş akışı skill'lerinin **projeye özgü** kısmı burada durur; skill'lerin gövdesi
generic'tir ve "kalite kapısından geçir" der, "hangi komut" demez. Başka bir
projeye taşırken `.claude/` klasörünü olduğu gibi kopyala ve yalnız bu dosyayı
(ve `/audit`, `/measure` merceklerini) yeniden yaz.

Proje sözleşmesinin tamamı `CLAUDE.md`'dedir; burada yalnızca **iş akışının
dokunduğu** kısmı özetlenir.

> Aşağıdaki `make` hedeflerinin bir kısmının girdisi henüz yok: `terminfo`
> (shell/TERM seti), `kur` (bundle seti).
> Hedef **var olur**, koşunca "henüz yok" deyip kırmızı düşer — "geçti" demez.
> O satır tetiklenirse doğrulama "yeşil" değil "koşamadı"dır: `[~]`
> işaretlenir, phase bitmiş sayılmaz. Hedef gerçek olunca bu listeden silinir.

## İçindekiler

- Doğrulama (definition of done)
- Kalite kapısı
- Yayın etkisi
- Teslim
- Bu depoya özgü tuzaklar

## Doğrulama (definition of done)

Bir phase, doğrulama yeşil olmadan bitmiş sayılmaz. Sırayla:

| durum | komut |
|---|---|
| her phase | `make hepsi` (`cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace`) |
| hızlı iç döngü | `cargo test -p {crate}` |
| `.metal` ya da `build.rs` değiştiyse | `make shader` — `build.rs` shader hatasını `cargo build`'de zaten yakalar; bu hedef cargo'nun bayatlık takibini atlayan kanaryadır (`touch` + `cargo build -p bt-gpu`), derleme reçetesi yalnız `build.rs`'te |
| `assets/terminfo/*` değiştiyse | `make terminfo` — *henüz yok, bkz. üstteki not* (`tic -x` ile geçici dizine derleme; SSH'daki uzak makine bunu **alamaz**, bkz. tuzaklar) |
| PTY okuyucu, render thread ya da paylaşılan duruma dokunulduysa | `make test-yaris` — iki zamanlama profili — yalnız `race_*` stresi (`--ignored race_`), sonra ignore'lular **dahil** bütün takım tek thread'de (`--include-ignored --test-threads=1`); ikisi de geçmeli. ThreadSanitizer nightly ister ve araç zinciri pin'li değil (`rustup` yok): TSan satırı nightly gelince Makefile'a eklenir, o güne kadar "TSan koşmadı" bir waive değil, bilinen sınırdır |
| pencereyi açan davranış değiştiyse (giriş, çizim, sekme) | `make duman` — uygulamayı `BT_RUN_SECONDS=3` ile başlatır; süre dolunca `kare=N hucre=K glif=G kural=R yuva=U/T yuk=smoke istek=I kapanis=clean profil=debug ornek=off pipeline=ok` basar ve **ilk dördü > 0 olmalıdır** (biri 0 → çıkış 1). `kare` bunun üstüne bir **üst sınır** taşır (`IDLE_FRAME_LIMIT`, bugün 8): boşta sıfır kareyi bozan değişikliğin belirtisi eksik kare değil **fazla** karedir ve `n > 0` onu göremezdi. `yuva=U/T` atlasın dolu yuva sayısı — bir kapı **değil**, sayaç: eşiği ölçülmedi (`/measure`) ve ölçülmemiş sayı kapıya yazılmaz. Aynı şey `istek=` (koşu boyunca istenen kare — çizilen değil; eşiği ölçülmedi) ve `profil=` (`debug`/`release`, `/measure`'ın release şartını satırdan okunur kılar) için de geçerli. `kapanis=` **kısmen** kapı: panik kolları koşuyu kırmızı düşürür, kayıtlı borç olan iki kol (`abandoned`, `unbounded`) düşürmez — bağlansaydı `make duman` bilinen bir borç yüzünden kırmızı düşerdi; değerlerin listesi `teardown_token`'da. `ornek=off` ölçüm kapısının (`BT_FRAME_STATS`) kapalı olduğunu söyler ve o koşuda ölçüm jetonları hiç basılmaz. `kare` GPU'nun hatasız bitirdiği kare, `hucre` son karede çizilen arka plan hücresi (imleç hariç), `glif` o karede çizilen glyph, `kural` o karede çizilen alt çizgi/üstü çizili: birincisi pipeline'ın çalıştığını, ikincisi shell çıktısının `Session::frame` sink'inden geçtiğini, üçüncüsü sınırın karakteri de taşıdığını, dördüncüsü biçimi de taşıdığını söyler. `glif` süs değil kapının kendisi: o olmasa `frame()` sınırı karakteri hiç geçirmese bile `kare=1 hucre=8 pipeline=ok` basılırdı; `kural` da aynı şekilde ayrı bir kapı, çünkü duman reçetesinin yedi kural hücresi **mürekkepsizdir** ve `glif`'i hiç oynatmaz. Duman koşusunda shell sabittir (`printf` ile `" bateri "` + yedi kural hücresi), yani `hucre=8 glif=6 kural=15` beklenir (sekiz `" bateri "` hücresi kalın **ve** altı çizili, üstüne yedi kural hücresi) ve sayılar kullanıcının `$SHELL`'ine bağlı değildir. Kapsamadığı: **GPU'nun o hücreleri gerçekten boyadığı** — dördü de CPU sayacıdır; boyamayı `make hepsi` içindeki `cell_bg_paints_pixels_on_the_gpu`, `glyph_differs_from_cell_background`, `rule_band_is_not_uniform_along_x` ve `sgr58_color_differs_from_foreground` offscreen sınamaları kanıtlar. `kural`ın ikinci sınırı: **stil ayrımını göremez** — beş çeşidi de düz çizgi çizen bir kod aynı R'yi basar; onun kapısı `bt-core`'un `smoke_shell_distinguishes_five_styles`'ı ile yukarıdaki kıvrım sınamasıdır. Pencerenin görünür ve doğru olduğunu da hiçbir hâlde kanıtlamaz: bundle'sız süreç öne çıkma hakkı taşımaz. Başsız ortamda binary `exit 78` + "ATLANDI" der ve `[~]` işaretlenir. **Üst sınırın durumu:** sayı 005 phase-3'te yeniden ölçüldü ve `2`'den `8`'e çıktı, çünkü eski değerin dayanağı (phase-1'in "sistem display link'i askıya alıyor, tavan ~3 kare" saptaması) **çürüdü** — ölçülen şey bir tavan değil, kapanış kilitlenmesiyle bozulmuş bir koşuydu. Bunun bir bedeli **ölçülerek** görüldü: eski sınır doğru bir build'i kırmızıya düşürüyordu. Gerekçenin tamamı, koşu sayıları, türetme ve rejim uyarıları `bt-shell`'de `IDLE_FRAME_LIMIT`'in doc'unda; **buraya kopyalanmıyor** (yukarıdaki `8` sınırın kendisi, yani kapının sözleşmesi — onu doğuran ölçüm değil), çünkü ölçülmüş sayının sahibi tek yerdir ve `make kur` geldiğinde yeniden ölçülecek olan da orasıdır. Kapının koştuğu tek bağlam `make duman`'dır (`report_and_exit` yalnız `BT_RUN_SECONDS` yolunda), yani etkileşimli koşu bu sınırı hiç değerlendirmez. Sınır büyüdükçe kapının algılama tabanı da yükseliyor — çok yavaş bir animasyon sızıntısı bugün yeşil geçebilir; kayıt `docs/YOL-HARITASI.md` → sete bağlanmamış borçlar |

**Türetilmiş dosya yoktur.** `default.metallib` `build.rs` üretir ve `target/`
altında kalır; depoya girmez. terminfo derlemesi de girmez. Dolayısıyla
odunluk'taki "`git status`'a bak, `.bin` değişmiş mi" refleksi burada **yok**;
buradaki karşılığı **"`Cargo.lock` değişti mi"**dir — değiştiyse ya bilinçli
bir bağımlılık kararıdır (aşağıda) ya da kusurdur.

**Ölçüm bir kapı değildir.** Kare süresi, giriş gecikmesi, bellek ve açılış
ölçümleri phase'i bloke etmez: gerçek pencere, sessiz makine ve dakikalar
isterler. Ölçümü **kullanıcı ister**, `/measure` koşturur. Kancası artık var
(`BT_FRAME_STATS=1 BT_SCROLL_TEST=1 BT_RUN_SECONDS=N`, 005) ve aynı jeton
satırını ölçüm sütunlarıyla genişletiyor; kancanın **olmadığı** yerler de
kayıtlı ve üç tane: bench (`criterion`, ayrı bir bağımlılık kararı), giriş
gecikmesi ve düşen kare sayımı. Listenin sahibi `/measure` skill'inin
tablosu — orada her satır "var" ya da "yok" diyor.

Bunun karşılığı tek bir kuraldır ve gevşemez: **ölçülmemiş sayı yazılmaz.**
"120 fps tutar", "gecikme düşer", "sekme başına bellek azalır" tahmini belgeye
girmez. Ölçüm bekleyen iş, phase'in `## Yayın Etkisi` bloğuna **"ölçüm
bekliyor: {ne}"** olarak düşer; `/measure` koşunca oradan kapanır.

## Kalite kapısı

Testler geçtikten sonra, bir sonraki phase'e geçmeden **sırayla**:

1. `/simplify` — değişen kodu reuse/sadeleştirme/verimlilik için temizle.
2. `/code-review` — sadeleşmiş son hâli hata avı için incele.
3. `/audit` — bu depoya özgü mercekler (katman yönü ve `bt-core`'un
   platformsuzluğu, panik yolu, boşta sıfır kare, hücre boyutu, ayar şeması,
   shell üçlüsü, yeni bağımlılık, ölçüm sahipliği). İlk ikisi genel kod
   kalitesine bakar ve projeye özgü kural okumaz; bu boşluğu `/audit` kapatır.

Sıra önemli: önce sadeleştir, sonra incele — böylece `/code-review`
sadeleştirmenin getirdiklerini de görür. `/audit` en sonda, son hâli görür.

### Kapıyı kim koşturur

**Ajan koşturur, Skill aracıyla.** Bunlar kullanıcının terminaline yazması
gereken komutlar değildir. Kapı düşerse sırayla in, ilk çalışanla yetin:

1. `Skill` ile `/simplify`, sonra `/code-review`.
2. Skill çağrısı gerçekten hata verirse: `code-reviewer` subagent'ını `Agent`
   ile koştur ve bunu phase dosyasına yaz.
3. O da olmuyorsa: **dur ve kullanıcıdan iste.** Sessizce yerine geçme.
4. Waive en son çaredir ve *bulgu* için tasarlanmıştır, **kapının kendisi
   için değil**. Kapı hiç koşmadıysa bu bir waive değil, atlanmış kapıdır.

**"Yok" demeden önce, aramanın o şeyi bulabilecek türden olduğunu göster.**
Yerleşik skill'ler `.claude/skills/` altında durmaz; orada bulamamak "yok"
demek değildir. Araç aradığını bulamayacak türdense, sonucu kanıt sayılmaz.

### İz

Kapı **her phase için** koşar ve izi phase dosyasındaki checklist kutularında
kalır. Kutu silinmez: koşmayan kapının kutusu da dosyada durur, yanına neden
koşmadığı yazılır. Kutusu olmayan kapı, atlandığını hiçbir yerde göstermez.

| işaret | anlamı |
|---|---|
| `- [x]` | yapıldı |
| `- [~]` | **waive / atlandı** — yanına gerekçe yazılır |
| `- [ ]` | yapılmadı |

`[x]` ile `[~]` aynı görünürse "phase bitti" sinyali sahteleşir: kapanış
taraması işaretli kutuyu sayar ve atlanmış kapıyı hiç görmez.

## Yayın etkisi

Bu depoda "deploy" bir `.app` paketidir ama phase düzeyinde dışarıya etki
**kullanıcının makinesindeki durumlar ve belgeler** üzerinden olur. Phase
dosyalarındaki `## Yayın Etkisi` bloğu şunları arar (hiçbiri yoksa "yok" yaz):

- **shader** — `.metal` değiştiyse `make shader` koştu mu; uniform/vertex
  yapıları Rust tarafındaki `#[repr(C)]` karşılığıyla **alan alan** aynı mı
- **terminfo / `TERM`** — yetenek ya da ad değiştiyse `assets/terminfo`
  güncellendi mi ve kullanıcının kurulum adımı (`[komut]`) teslim.md'ye düştü mü;
  uzak makinelerde `TERM` bilinmez, geri düşüş (`xterm-256color`) korunmalı
- **ayar şeması** — `settings.toml`'a anahtar eklendi/adı değiştiyse: varsayılan
  değer, `docs/AYARLAR.md`, eski anahtarın akıbeti (okunmaya devam mı, uyarı mı);
  **bilinmeyen anahtar asla silinmez**
- **tema / materyal biçimi** — dosya biçimi değiştiyse paketli temaların hepsi
  ve kullanıcı tema dizini için geriye dönük okuma
- **shell entegrasyonu** — `assets/shell/` altında bir kabuk değiştiyse üçü de
  (zsh, bash, fish) gözden geçirildi mi; kullanıcı rc dosyasına **dokunulmaz**
- **app bundle** — `Info.plist`, entitlements, kullanım açıklamaları
  (`NS*UsageDescription`), imza ve notarization etkisi
- ölçüm bekleyen iddia → **"ölçüm bekliyor: {ne}"** yaz, sayı uydurma
  (`/measure` ile kullanıcı koşturur, sonuç `docs/OLCUMLER.md`'ye girer)
- `CLAUDE.md` / `docs/MIMARI.md` / crate'in `lib.rs` başlık yorumu güncellenmeli mi
- **yeni bağımlılık** — `Cargo.toml`'a crate eklemek **mimari karardır**,
  kendiliğinden yapılmaz: dur ve sor

## Teslim

Tek branch: `main`. Dev branch, migration, container, panel yok.

- Commit iletisi **Türkçe, emir kipinde, tek satırlık özet**
  ("Glyph atlasını tek dokuya topla ve tahliyeyi ölç").
- Phase başına tek commit; push `/ship` kararıdır, `/implement` push etmez.
- `make hepsi` yeşil olmadan push yok.
- Depoya girmeyenler: `target/`, `*.metallib`, `*.dSYM`, `*.dmg`, `*.app`,
  `.DS_Store`. `Cargo.lock` **girer** (uygulama, kitaplık değil).

## Bu depoya özgü tuzaklar

- **Katmanlar tek yönlüdür**, hiçbir bağımlılık yukarı gitmez:
  `bateri → bt-shell → bt-gpu → {bt-atlas, bt-core}`. `bt-core` **platformsuzdur**:
  `objc2*`, `core-text`, `metal` görmez ve Linux'ta derlenebilir kalır — Vulkan
  kapısı bunun üstüne kurulur. `bt-atlas` yalnız `core-text`/`core-graphics` görür.
- **Renderer'a terminal semantiği eklenmez.** OSC/CSI ayrıştırma, komut blokları,
  seçim modeli `bt-core`'dadır; `bt-gpu` "ne çizeceğini" alır, "ne anlama geldiğini"
  bilmez. Renderer'da bir escape dizisini tanıyan dal görürsen yer yanlıştır.
- **Boşta sıfır kare.** Kirli satır yoksa frame gönderilmez; her animasyonun
  bir **durma koşulu** vardır (imleç yerleştiğinde, decay bittiğinde). Durma
  koşulu olmayan animasyon pil tüketen sonsuz döngüdür ve belirtisi sessizdir.
- **Hücre sabit boyuttadır.** Grid hücresinin boyutu bir `const` assert ile
  bağlıdır (`bt-core/src/lib.rs`); bugünkü sayı ve `frame()` sınırının ayrı
  hücresiyle farkı `CLAUDE.md`'de — buraya kopyalanmaz. Emoji, grapheme
  kümesi, alt çizgi rengi gibi seyrek veriler **yan tablodadır**; grid
  hücresine alan eklemek 10 000 satırlık scrollback'i sekme başına
  megabaytlarca büyütür.
- **PTY ve ayrıştırma yolunda panik yok.** `unwrap`/`expect` o yolda yasaktır;
  bilinmeyen dizi **yoksayılır**. Bir TUI'nin gönderdiği beklenmedik bayt
  uygulamayı düşüremez. Kuralın loglama yarısı **henüz borç**: `tracing`
  bağlanmadı, yoksayılan olay sessizce düşüyor (`CLAUDE.md`).
- **`TERM` adı bir sözleşmedir.** Özel bir terminfo dağıtılacaksa SSH'daki
  uzak makinede yoktur (Metalterm'in yaşadığı #23). Ya `xterm-256color` ile
  uyumlu kal ya da uzak tarafta geri düşüşü tasarla; ikisi de yapılmadan
  `TERM` değiştirilmez.
- **Ayar anahtarı silinmez, bilinmeyen anahtar korunur.** `settings.toml`
  kullanıcının dosyasıdır; okuyup yeniden yazarken tanımadığın anahtarı düşürmek
  veri kaybıdır.
- **Ölçümün tek sahibi `docs/OLCUMLER.md`.** Başka belgeye sayı yazma;
  niteliksel anlat ve oraya bağla.
- **Araç zinciri pin'li değil.** Homebrew rustc, `rustup` yok,
  `rust-toolchain.toml` bilinçli olarak yok (001 kararı); `rust-version`
  yalnız tabandır. `brew upgrade` sonrası yeni bir clippy lint'i dokunulmamış
  kodu kırmızıya çevirebilir — `make hepsi` bu yüzden sürümü başta basar;
  kırmızı görünce önce sürüme bak, koda değil.
- **Dil:** kod yorumları, commit iletileri ve belgeler Türkçe ve "neden"i
  anlatır. **Kod tanımlayıcılarının tamamı İngilizce** — dışa bakan ad da,
  yerel yardımcı, alan, değişken ve sınama adı da; `build.rs` dahil,
  istisnasız. Kullanıcıya görünen UI dizgileri, ayar anahtarları ve tema
  adları İngilizce (ürün uluslararası, ayar dosyası paylaşılır). İki şey
  Türkçe kalır ve ikisi de kod değildir: süreç/tanı çıktısı (stderr,
  `make duman` satırları, `assert!` gerekçeleri) UI dizgisi olmadığı için,
  `Makefile` hedefleri projenin komut yüzeyi olduğu için. `kare=`/`hucre=`/
  `glif=` jetonları makine sözleşmesidir: silinmez, eklenir.
