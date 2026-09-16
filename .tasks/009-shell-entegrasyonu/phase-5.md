# Phase 5 — Sarmalayıcı kullanıcının dosyasını doğru bağlamda okur

## Özet

Sarmalayıcı kullanıcının başlangıç dosyasını **fonksiyonun içinden** `source`
ediyor. zsh'te fonksiyon içindeki `typeset` yereldir: dönüşte silinir. Yani
`typeset -U path; path+=(…)` — Homebrew, asdf, pyenv ve nvm'in standart PATH
deyimi — bateri'de sessizce kayboluyor, her başka terminalde çalışıyor.
Bu phase `source`'u dosyaların **en üst seviyesine** çıkarıyor ve aynı
yeniden yazımda sarmalayıcının üç kaçak kolunu kapatıyor.

_Requirements: R3.5_

Set yürürken `/code-review` buldu (dört bulgu); ölçüldü ve kendi kılavuzunu
hak ediyor (`duzen.md` → Ek phase eşiği): beş dosyaya yayılıyor, kendi
doğrulaması ve yayın etkisi var.

## Ölçüm

Sarmalayıcıyla ve sarmalayıcısız, aynı sahte ev dizini
(`typeset -U path; path+=(/opt/probe)`, `typeset -A probe_map=(k v)`):

| | `path` içindeki indeks | `probe_map` türü |
|---|---|---|
| sarmalayıcı üzerinden | `0` (yok) | yok |
| düz `ZDOTDIR` | `15` | `association` |

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — gövde `__bateri_load`'ı bırakıp iki
  parçaya ayrılır: `__bateri_begin {ad}` (ZDOTDIR'ı kullanıcıya takas eder,
  `HISTFILE`'ı düzeltir, yüklenecek dosyanın yolunu `__bateri_file`'a
  **`typeset -g`** ile yazar) ve `__bateri_end` (kullanıcının dosyası
  ZDOTDIR'ı değiştirmiş olabilir, yeniden okur; sonra bizimkini geri kurar).
  `source` artık gövdede değil. `local file=` gölgesi de böyle biter.
- **`assets/shell/zsh/.zshenv`, `.zprofile`, `.zshrc`, `.zlogin`** — dördü de
  aynı iskelet: gövde yüklü değilse **kendisi yükler** (atlanmış `.zshenv`
  kolu), sonra `__bateri_begin` → top-level `source` → `__bateri_end`.
  Gövdeye hiç ulaşılamıyorsa dördü de `.zshenv`'in bugünkü geri düşüşünü
  yapar (`BATERI_ZDOTDIR` varsa geri koy, yoksa `unset`) — çıplak `return`
  **olmaz**: `ZDOTDIR` bizde asılı kalırdı.
- **`.zshenv` — yalnız kendisinin okunduğu kabukta geri koyma.** `zsh -c`
  (etkileşimsiz, login değil) bizim dosyalarımızdan yalnız `.zshenv`'i
  okuyor: bugün `BATERI_ZDOTDIR`'ı tüketip `ZDOTDIR`'ı bizde bırakıyor ve o
  kabuktan doğan her zsh kullanıcının yapılandırmasını kaybediyor. Pencere
  gerçek: `/etc/zprofile` ve `/etc/zshrc` tam orada koşuyor.
- **`__bateri_begin` — kendine dönük `ZDOTDIR` kolu.** `BATERI_ZDOTDIR` bizim
  dizinimizi gösteriyorsa "kullanıcının yoktu"ya düşer; yoksa kendi
  `.zshenv`'imizi yeniden `source` eder ve zsh'in `FUNCNEST` sınırına kadar
  özyineler (ölçüldü: 336 satır hata, oturum `ZDOTDIR`'sız kalıyor).
- **`crates/bt-shell/src/app.rs` → `shell_integration_env`** — aynı kapının
  Rust yarısı: özgün `ZDOTDIR` betiğin dizinine eşitse `BATERI_ZDOTDIR`
  gönderilmez. Ayrıca `env::var` yerine `var_os`: UTF-8 olmayan bir
  `ZDOTDIR` bugün `None`'a düşüyor, yani "kullanıcının yoktu" sayılıyor ve
  `__bateri_restore` onu **siliyor** — komşu her kenar (UTF-8 olmayan betik
  yolu, `$SHELL`) entegrasyonu reddederek geri düşüyor, bu ondan ayrışıyordu.
- **`crates/bt-shell/src/child.rs` (sınama)** — e2e testi `ZDOTDIR`'ı **depo
  ağacındaki** `assets/shell/zsh`'e bağlıyordu: oraya düşen bir
  `.zsh_history` çalışma kopyasını kirletir ve `bateri` crate'indeki envanter
  sınamasını (`zsh_wrapper_inventory_is_exactly_what_the_bundle_copies`)
  başka bir test binary'sinde, kalıcı olarak kırmızıya çevirir. Beş dosya
  `TempRoot`'a kopyalanır. Aynı sınamaya R3.5'in pini girer (`typeset -U
  path`, `typeset -A`, `$#`) ve `.zsh_history` tuzağının yarışı kapanır:
  bugün `exit` yazıldıktan mikrosaniyeler sonra koşuyor, oysa zsh geçmişi
  **çıkışta** yazıyor.

## Kapsam Dışı

`$0` farkı: top-level `source` içinde zsh `$0`'ı dosyanın yoluna kuruyor,
gerçek başlangıçta kabuğun adı olurdu. Çaresi `function_argzero`'yu geçici
kapatmak olurdu — kullanıcının kodunun etrafında option çevirmek, tam da
kaçındığımız görünmez mutasyon; kitty'nin sarmalayıcısı da aynı farkı kabul
ediyor. Bilinen ve sınırlı fark olarak `bateri.zsh`'in başlığına yazılır.
`$#`/`$1` farkı ayrıca ele alınmıyor: argümansız top-level `source` onu
zaten düzeltiyor.

## Kabul

- Sahte ev dizini `typeset -U path; path+=(/opt/probe)` ve `typeset -A` ile:
  sarmalayıcılı oturumun gördüğü indeks ve tür, sarmalayıcısızla **aynı**;
  kullanıcının dosyası `$#` = 0 görüyor.
- `ZDOTDIR=<sarmalayıcı> zsh -c 'echo ${ZDOTDIR-unset}'` → `unset`.
- `BATERI_ZDOTDIR=<sarmalayıcı>` verilen kabuk sessizce açılıyor (özyineleme
  yok) ve kullanıcının dizinini kaybetmiyor.
- Bozuk `.zshrc` hâlâ kabuğu düşürmüyor; işaretler (`A`/`B`/`C`/`D`) geliyor.
- E2e sınaması depo ağacına **hiçbir şey yazmıyor** (`git status` temiz).
- `make hepsi` + `make kur` + `make duman` (duman kullanıcıda).
- Kullanıcı gerçek pencerede: `echo $PATH` entegrasyonsuz oturumla aynı,
  `print -l $precmd_functions` kancayı en sonda gösteriyor.

## Yayın Etkisi

**shell entegrasyonu** — betiğin yüklenme deseni değişiyor; kullanıcının rc
dosyalarına hâlâ yazılmıyor ve dosya adları (beş dosya) aynı kalıyor, yani
`make kur`'un kopya/`cmp` listeleri ve envanter sınaması **değişmiyor**.
bash ve fish hâlâ kapsam dışı (Karar 6) ama bu phase'in dersi onların
betiğine de geçer: kullanıcının dosyası fonksiyon içinden `source`
edilmez.

**`CLAUDE.md`** — "Shell entegrasyonu" maddesindeki sarmalayıcı tarifi
phase-4'te yazıldı; `bateri.zsh`'in "mantığın tamamı burada, dört dosya
birkaç satır" cümlesi bu phase'de yanlışlanıyor ve aynı commit'te düzelir.

ayar şeması yok · shader yok · terminfo yok · tema yok · app bundle yok
(dosya adları değişmiyor) · yeni bağımlılık yok · ölçüm bekleyen iddia yok.

## Uygulama Notları

- **Pin kırmızıyı tek satırda gösterdi ve üç kusur da oradaydı:**
  `… 0 yok 1` (PATH girdisi yok, `typeset -A` yok, konumsal parametre sızmış)
  → `… 1 association 0`. Plan yalnız `typeset`'i konuşuyordu; `$#` bedava
  düzeldi, çünkü argümansız top-level `source` konumsal parametreleri
  değiştirmiyor.
- **`path`'teki indeks sınamaya konulamaz.** İlk yazımda `${path[(I)/opt/probe]}`
  doğrudan basılıyordu; değer kalıtılan `PATH`'in uzunluğuna bağlı, yani
  makineye göre değişir. Basılan şey **varlık** oldu (`> 0`).
- **Guard çıplak `return` değil, "kendini yükle".** `/code-review` "her
  dosyanın başına `(( $+functions[…] )) || return`" öneriyordu; o kol
  `ZDOTDIR`'ı bizde, `BATERI_ZDOTDIR`'ı ihraçlı bırakırdı — yani sızıntıyı
  düzeltmek yerine sabitlerdi. Dosya gövdeyi kendisi `source` ediyor;
  gövdeye hiç ulaşılamıyorsa dördünde de `.zshenv`'in geri düşüşü tekrarlanıyor.
  Dört kopya bilinçli: gövde okunamıyorsa gövdedeki yardımcıya sığınılamaz.
- **`.zshenv`'in iki geri koyma koşulu tek `if`'te.** `__bateri_restore`
  kendini `unfunction` ediyor, yani ayrı iki satır ikinci tetiklemede
  `command not found` verirdi.
- **`$0` farkına dokunulmadı** (Kapsam Dışı). `function_argzero`'yu geçici
  kapatmak kullanıcının bilerek yaptığı `unsetopt`'u ezerdi — kullanıcı
  kodunun etrafında option çevirmek tam da bu phase'in kapattığı kusurun
  türü.
- **Beş kabul senaryosu gerçek zsh'te ölçüldü** (`bateri.zsh` değişmeden önce
  ve sonra): A/B probu artık **aynı** (`idx=1 map=association argc=0`);
  `zsh -c` `ZDOTDIR=unset` veriyor (önce sarmalayıcının yolunu veriyordu);
  kendine dönük `ZDOTDIR` sessiz (önce 336 satır `FUNCNEST` hatası);
  `.zshenv` atlanmış kol kullanıcının dosyasını yüklüyor ve hata basmıyor;
  gövdesiz kol `ZDOTDIR`'ı kullanıcıya geri koyuyor.
- **Phase diff'inin `/code-review`'u üç bulgu verdi, üçü de düzeldi.**
  (1) `${__bateri_dir:A}` `[[ ]]` içinde **tırnaksızdı**, yani glob deseni
  sayılıyordu: `/Applications/[dev] bateri.app/…` gibi bir yolda kendine
  dönük `ZDOTDIR` kapısı açılır ve tam da önlediği özyinelemeye düşerdi.
  Köşeli parantezli yolla ölçüldü, düzeltmeden sonra `ZDOTDIR=unset`.
  (2) Kapanış senkronu `reader_alive()` ile yazılmıştı; `shutdown()` işe
  okuyucuyu `take` ederek başladığı için o sorgu dönüşte zaten `false`.
  (3) Kendi kendine yetme kolunun **gerekçesi** yanlıştı: `no_rcs`
  kapandıktan sonra zsh `/etc/zprofile` dahil hiçbir başlangıç dosyası
  okumuyor, yani "sonra geri açılır" senaryosu imkânsız. Kol duruyor
  (ulaşılabilir hâli okunamayan bir `.zshenv`), gerekçe düzeldi.
- **`Teardown::Clean` iddiası ölçümle çürüdü.** İnceleme kapanış senkronu
  için onu öneriyordu; koşuda `Abandoned` geliyor — çıkışın içinde takılan
  çocuk kapanışı asamıyor ve bu kayıtlı borç (`CLAUDE.md` → Kapanış).
  Senkron **olayın kendisine** bağlandı: geçmiş dosyasının kullanıcının
  dizininde belirmesi. Aynı bekleyiş incelemenin ikinci isteğini de
  karşılıyor — pozitif iddia, "doğru yere yazıldı" ile "hiç yazılmadı"yı
  ayırıyor.
- **Tuzağın ısırdığı gösterildi:** `HISTFILE` düzeltmesi geçici olarak
  kapatılınca sınama kırmızı düşüyor ve `.zlogin`'in gördüğü yol
  sarmalayıcının dizinini gösteriyor. Kapanış artık bekleniyor
  (`shutdown` + `reader_alive`), çünkü zsh geçmişi **çıkışta** yazıyor.

## Checklist

- [x] Test-first: R3.5 pini e2e sınamasına girdi ve **kırmızı** görüldü
      (`… 0 yok 1` → `… 1 association 0`)
- [x] `bateri.zsh`: `__bateri_begin` / `__bateri_end`, `source` gövdeden çıktı
- [x] Dört dosya: kendi kendine yeten iskelet + gövdesiz geri düşüş
- [x] `.zshenv`: yalnız kendisinin okunduğu kabukta geri koyma
- [x] Kendine dönük `ZDOTDIR`: betik kolu + `shell_integration_env` kolu
      (`a_self_referential_zdotdir_is_not_handed_back`)
- [x] `shell_integration_env`: `var_os`
      (`a_non_utf8_zdotdir_refuses_the_integration`)
- [x] E2e sınaması `TempRoot`'a taşındı; `.zsh_history` tuzağının yarışı kapandı
- [x] Doğrulama: `make denetim` tek başına, `make hepsi` (354 sınama) ve
      `make kur` geçti
- [x] `make duman` (kullanıcı koşturdu; ajan kabuğunda yanlış tanıyla
      düşüyor — `docs/YOL-HARITASI.md`'nin borç kalemi):
      `kare=29 hucre=8 glif=6 kural=15 yuva=13/2048 istek=4 icerik=2
      hareket=27 sessiz=1754.25ms kapanis=clean`. Sabitler phase-3 ve
      phase-4'tekiyle birebir: sarmalayıcının yeniden yazımı süreli koşuya
      dokunmuyor (entegrasyon orada zaten kurulmuyor)
- [x] Gözle doğrulama (kullanıcı, paketten açılan gerçek pencere): `$PATH`
      kullanıcının bütün araç dizinlerini taşıyor (Homebrew, nvm, bun, pnpm,
      gvm, go), `${ZDOTDIR:-unset}` → `unset`, `print -l $precmd_functions`
      → `omz_termsupport_precmd`, `iterm2_precmd`, `__bateri_precmd` (en sonda)
- [x] `/code-review` (phase diff'i; set kapısı bu kodu görmedi) — üç bulgu,
      üçü de düzeltildi (Uygulama Notları)
- [x] Yayın etkisi yazıldı
