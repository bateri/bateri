# Phase 4 — Betik ürüne girer ve girdiği denetlenir

## Özet

Sarmalayıcı `.app` paketine kopyalanır ve kopyalandığı **denetlenir**; eksik
betiğin belirtisi, tasarlanmış sessizlikle karışmaz.

_Requirements: R6.1_

## Değişiklikler

- **`Makefile` → `kur`** — iki satır, ikisi de zorunlu: `assets/shell/`
  paketin `Contents/Resources`'ına kopyalanır **ve** içerik denetimine bir
  kalem eklenir (betik pakette var mı, girdiyle aynı mı). Emsal `Credits.html`
  ile `THIRD-PARTY-LICENSES.txt`'in `cmp -s` satırı. Kopya ile denetim iki
  ayrı elle yazılmış liste; biri eklenip öteki unutulursa kapı yine kör kalır.
- **`crates/bateri/src/bundle_assets.rs`** — girdi denetimi: betik `assets/`
  altında var ve okunabilir. Lisans metinlerinin denetimiyle aynı örüntü;
  `make hepsi`'de ajansız koşar.
- **`Makefile` → `denetim`** — rc dosya adı listesi genişletilir:
  `.zshenv`, `.zlogin`, `.zlogout` bugün listede **yok**, oysa bu setin
  yönlendirdiği dosyalar tam olarak onlar. Kapı, "betik kullanıcının rc
  dosyasına yazıyor mu" sorusunu ancak adı listede olan dosya için soruyor.
- **`.claude/is-akisi/proje.md`** — doğrulama tablosuna `assets/shell/*`
  satırı: betik değişince `make kur` koşulur. Bugün böyle bir satır yok, yani
  betik değişikliği hiçbir koşullu komutu tetiklemiyor.
- **`CLAUDE.md`** — "Shell entegrasyonu" maddesi bugüne kadar üç kabuğu
  anlatıyordu; bugünkü hâl (zsh indi, bash/fish sonraki sette) ve komut
  durumunun nerede yaşadığı yazılır.

**Neden ayrı phase:** eksik betiğin belirtisi ile tasarımın bilerek sessiz
yaptığı geri düşüş **aynı** — blok yok, hata yok. Üstelik betiğin debug'da
depo yolundan bulunması, bozulabilen tek derlemeyi (release) hiçbir kapının
dokunmadığı tek derleme yapıyor. Bu phase o asimetriyi kapatıyor.

## Kabul

- `make kur` betiği pakete koyuyor ve koymazsa **düşüyor** (çıkış 2):
  betiği geçici olarak kopyalamayan bir tarifle denenip doğrulanır.
- `make hepsi` girdi eksikse düşüyor (`bundle_assets` sınaması).
- `make denetim` genişletilmiş listeyle hâlâ `temiz`; listeye eklenen
  dosyalardan birine yazan **kasıtlı** bir betikle kırmızı düştüğü gösterilir.
- Paketten açılan bir oturumda (`open ... bateri.app`) entegrasyon kuruluyor:
  betik paket yolundan bulunuyor, depo yolundan değil.

## Yayın Etkisi

**app bundle** — pakete yeni bir kaynak türü giriyor (`Contents/Resources`
altında shell betiği). `Info.plist`, entitlements ve imza etkilenmiyor; imza
zaten yok (006 Karar 6).

**shell entegrasyonu** — betiğin ürüne giden yolu bu phase'de kapanıyor;
kullanıcı rc dosyasına dokunulmuyor ve denetimin listesi artık zsh'in beş
dosyasını da kapsıyor.

Türetilmiş dosya yok: betik **kaynaktır**, üretilmez.

shader yok · terminfo yok · ayar şeması yok (anahtar phase-3'te) · tema yok ·
yeni bağımlılık yok.

## Uygulama Notları

- **Kopya `cp -R assets/shell` değil, adları tek tek sayan bir liste.**
  Özyinelemeli kopya dizinde **ne bulursa** onu ürüne sokar ve bu dizin
  yazılabilir bir hedef: 009 phase-3'te `/etc/zshrc` orada bir kez gerçekten
  `.zsh_history` doğurdu. Kullanıcının komut geçmişinin `.app`'e girmesi,
  betiğin eksik girmesinden daha sessiz bir kusur olurdu.
- **Girdi denetimi "var mı" değil **envanter** oldu.** Plan "betik `assets/`
  altında var ve okunabilir" diyordu; o iddiayı `bt-shell` zaten koruyor
  (`the_zsh_wrapper_ships_with_the_crate`, beş dosyanın beşini de arıyor) ve
  kopyası buraya ikinci bir sahip getirirdi. Bu sınamanın tek başına gördüğü
  yarı **fazlalık**: `kur` elle yazılmış iki liste (kopya + `cmp`) taşıyor,
  yani listelere düşmemiş yeni bir girdi pakete hiç girmez ve **iki kapı da
  yeşil kalır**. Aynı sınama yukarıdaki artık dosyayı da görüyor.
  `.DS_Store` dışarıda: Finder üretiyor, depoya girmiyor ve kopya listesi
  adları saydığı için pakete sızamıyor — kapının kod doğruyken düşmesi
  gördüğü kusurdan pahalı olurdu.
- **`proje.md`'ye yeni satır değil, var olan satırın koşulu genişledi.**
  `assets/shell/*` aynı komutu (`make kur`) tetikliyor; ikinci bir satır
  "hangi değişiklik `kur`'u gerektirir" sorusuna iki sahip verirdi.
- **`make duman` ön plana gelmeyen pencerede kırmızı düşüyor ve tanısı yanlış
  yeri gösteriyor.** Ajanın kabuğundan (arka planda açılan pencere) üç koşu da
  `Verdict::MotionUnsettled` verdi: hareket karesi **1**, sonrasında ~1,95 sn
  sessizlik. Aynı kırmızı `2af9d87`'de (009 hiç başlamadan önce) da
  tekrarlıyor, yani setin diff'inden gelmiyordu; kullanıcının kendi
  terminalindeki koşu **yeşil** (`kare=29 hareket=27 icerik=2
  sessiz=1748.85ms kapanis=clean`). Sebep vsync: pencere hiç görünür olmadan
  link callback vermiyor, `advance` bir daha koşmuyor ve durum yerleşmemiş
  kalıyor. Örtülme kolu bunu kapatıyor (`set_visible(false)` →
  `Motion::finish`) ama o kol **örtülme bildirimi geldiğinde** koşuyor; hiç
  görünmemiş pencerede bildirim yok. `Motion::finish`'in doc'u bu sonucu
  zaten tarif ediyor — "tanı 'bir durma koşulu bozuk' diye yanlış yeri
  gösterirdi" — yani bilinen bir kenar, yalnız kapanmamış. Kapsamı 009
  değil: kapının kendi davranışı, ayrı ele alınır (`docs/YOL-HARITASI.md`).

## Checklist

- [x] `make kur`: kopya + içerik denetimi
- [x] `bundle_assets`: girdi denetimi (envanter — gerekçe Uygulama Notları'nda)
- [x] `make denetim`: rc dosya listesi genişletildi
- [x] `proje.md` doğrulama tablosu + `CLAUDE.md`
- [x] Test: betiksiz paket `make kur`'u düşürüyor (kopya satırı kapatıldı →
      `kur: içerik denetimi düştü — shell/zsh/.zshenv pakette yok ya da
      girdiden farklı`, çıkış 2, önceki paket `$(STAGE)` sayesinde yerinde
      kaldı); rc'ye yazan betik `denetim`'i düşürüyor (`~/.zlogout`'a yazan
      geçici dosya); envanter sınaması fazladan `.zsh_history` ile kırmızı
- [x] Doğrulama geçti: `make hepsi`, `make kur` ve `make duman`
      (`kare=29 hucre=8 glif=6 kural=15 yuva=13/2048 istek=4 icerik=2
      hareket=27 sessiz=1748.85ms kapanis=clean`) — duman'ı kullanıcı kendi
      terminalinde koşturdu; neden ajanın kabuğunda kırmızı düştüğü Uygulama
      Notları'nda
- [x] Paketten açılan oturumda entegrasyon kuruluyor (kullanıcı, gerçek
      pencere: `print -l $precmd_functions` → `omz_termsupport_precmd`,
      `iterm2_precmd`, `__bateri_precmd` — kancamız **en sonda**). Release'te
      depo kolu hiç derlenmiyor, yani betiğin bulunması paket yolunun kanıtı
- [x] Yayın etkisi yazıldı
