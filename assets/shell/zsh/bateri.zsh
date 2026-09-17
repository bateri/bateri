# bateri'nin zsh sarmalayıcısı — ortak gövde.
#
# Yükleyeni bizim ZDOTDIR'ımızdaki dört dosya (`.zshenv`, `.zprofile`,
# `.zshrc`, `.zlogin`). Gövde ZDOTDIR takasını ve kancaları tutuyor; `source`
# **burada değil**, her dosyanın kendi en üst seviyesinde.
#
# KULLANICININ DOSYASI FONKSİYON İÇİNDEN `source` EDİLMEZ (009 phase-5) ve bu
# kuralın bedeli ölçüldü: zsh'te fonksiyon içindeki `typeset` YERELDİR, yani
# `typeset -U path; path+=(…)` — Homebrew, asdf, pyenv ve nvm'in standart PATH
# deyimi — dönüşte silinirdi. Belirti sessiz: kullanıcının araçları yalnız
# bateri'de kaybolur, her başka terminalde çalışır. Aynı sınır konumsal
# parametreleri de bozuyordu (dosya `$#`'i 1 görüyordu). Bu yüzden gövde iki
# parçaya ayrıldı: [`__bateri_begin`] hazırlar, dosya top-level `source`
# yapar, [`__bateri_end`] toplar.
#
# SÖZLEŞME (kuran taraf `bt-shell`'in `app::shell_integration_env`'i):
#   ZDOTDIR         bu dizin
#   BATERI_ZDOTDIR  kullanıcının özgün ZDOTDIR'ı. Ortamda yoksa kullanıcının
#                   da yoktu; geri koyarken ZDOTDIR silinir, $HOME'a
#                   eşitlenmez — ihraç edilen bir ZDOTDIR ile hiç olmayan
#                   ZDOTDIR çocuklar için farklı şeyler.
#
# BİLİNEN VE SINIRLI FARK: top-level `source` içinde zsh `$0`'ı yüklenen
# dosyanın yoluna kuruyor; gerçek başlangıçta kabuğun adı olurdu. Çaresi
# `function_argzero`'yu geçici kapatmak olurdu — kullanıcının kodunun
# etrafında option çevirmek, tam da kaçındığımız görünmez mutasyon. kitty'nin
# sarmalayıcısı da aynı farkı kabul ediyor.
#
# HİÇBİR KOLDA ÖLÜMCÜL DEĞİL: `exit` yok, kullanıcının her dosyası korunarak
# okunuyor. Gerekçe sert — çocuk ölünce uygulama kapanıyor (`bt-shell`'in
# `child_exit` → `terminate:` yolu), yani düşen bir sarmalayıcı kullanıcıyı
# Settings…'e bile ulaşamaz bırakırdı.
#
# KULLANICININ DOSYALARINA YAZILMAZ, yalnız okunur (`make denetim` kapısı).
#
# SİSTEMİN rc dosyaları (`/etc/zshrc`) her aşamada bizimkinden ÖNCE okunuyor
# ve o sırada ZDOTDIR bizi gösteriyor. Yazan tek kalem `HISTFILE` ve
# [`__bateri_begin`] onu düzeltiyor. Kalan kalem salt okunur ve bilerek
# bırakıldı: `/etc/zshrc` `${ZDOTDIR:-$HOME}/.zkbd/${TERM}-${VENDOR}`
# arıyor, yani `~/.zkbd` ile tuş bağlaması üretmiş bir kullanıcı onu
# yükleyemez ve terminfo'dan gelen varsayılana düşer. Çaresi sistemin rc
# mantığını kopyalamak olurdu — macOS sürümüne bağlı, kırılgan bir
# tekrar; veri kaybı yok.
#
# `.zlogout` bizde YOK ve bu bir eksik değil: ZDOTDIR en geç `.zlogin`'de
# kullanıcıya geri konuyor, yani çıkışta zsh zaten kullanıcının kendi
# `.zlogout`'unu okuyor. Beşinci bir dosya koysaydık hiçbir kolda koşmazdı —
# geri koymayı ertelemek ise ZDOTDIR'ı bütün oturum boyunca çocuklara
# sızdırmak olurdu (tmux, iç içe kabuk).

# Bizim dizinimiz. zsh bu dosyayı bulmak için ZDOTDIR'ı kullandı, yani değer
# şu an bizimki; `${0:A:h}` yalnız `unsetopt function_argzero` kenarı için
# yedek.
: ${__bateri_dir:=${ZDOTDIR:-${0:A:h}}}

# Kullanıcının dizini ve ZDOTDIR'ının VAR OLUP OLMADIĞI — ikisi ayrı bilgi.
# Bir kez saptanıyor: `.zshenv` her zsh'te okunuyor ve ortam değişkenini
# oradan alıp siliyoruz.
if (( ! ${+__bateri_had} )); then
  # KENDİNE DÖNÜK DEĞER REDDEDİLİYOR: `BATERI_ZDOTDIR` bizim dizinimizi
  # gösteriyorsa "kullanıcının özgün değeri"ni değil kendimizi geri koyardık
  # — `__bateri_begin` kendi `.zshenv`'imizi yeniden yükler ve zsh'in
  # FUNCNEST sınırına kadar özyineler (ölçüldü: 336 satır hata, oturum
  # ZDOTDIR'sız kalıyor). Kapının ilk katı Rust tarafında
  # (`shell_integration_env`); bu ikinci kat, ortamı elle kuran hâller için.
  # Sağ taraf TIRNAKLI: `[[ ]]` içinde tırnaksız sağ işlenen bir **glob
  # deseni**, düz metin değil. Paket `/Applications/[dev] bateri.app/…` gibi
  # bir yolda dursaydı desen kendi düz değeriyle eşleşmez, kapı açılır ve tam
  # da önlediği özyinelemeye düşerdik (`/code-review`, 009 phase-5).
  if [[ -n ${BATERI_ZDOTDIR} && ${BATERI_ZDOTDIR:A} != "${__bateri_dir:A}" ]]; then
    __bateri_had=1
    __bateri_user=$BATERI_ZDOTDIR
  else
    __bateri_had=0
    __bateri_user=$HOME
  fi
  unset BATERI_ZDOTDIR
fi

# Kullanıcının aynı adlı başlangıç dosyasını yüklemeye HAZIRLAR; yüklemeyi
# çağıran dosya kendi en üst seviyesinde yapar.
#
# ZDOTDIR yükleme boyunca KULLANICININ değerini taşıyor ve bunun iki sebebi
# var: dosyanın kendisi `$ZDOTDIR`'ı doğru görsün, ve o dosyadan doğan alt
# süreçler (brew shellenv, nvm, direnv) bizim dizinimizi miras almasın.
__bateri_begin() {
  if (( __bateri_had )); then
    ZDOTDIR=$__bateri_user
  else
    unset ZDOTDIR
  fi
  # SİSTEMİN rc dosyası bizden ÖNCE okundu ve ZDOTDIR o sırada bizi
  # gösteriyordu: macOS'un `/etc/zshrc`'si `HISTFILE`'ı
  # `${ZDOTDIR:-$HOME}/.zsh_history` diye kuruyor. Düzeltmezsek kullanıcının
  # komut geçmişi uygulamanın paketine yazılır ve kendi dosyası donardı —
  # belirtisi de sessiz olurdu. Düzeltme kullanıcının dosyası yüklenmeden
  # ÖNCE, çünkü onun rc'si `HISTFILE`'ı okuyup üstüne kurabiliyor. Kendi
  # yolunu yazmış kullanıcıya dokunulmuyor: koşul yalnız BİZİM dizinimizi
  # gösteren değeri yakalıyor.
  if [[ -n $HISTFILE && $HISTFILE == "$__bateri_dir"/* ]]; then
    HISTFILE=${ZDOTDIR:-$HOME}/${HISTFILE#"$__bateri_dir"/}
  fi
  # `typeset -g`: değeri okuyacak olan, bu fonksiyon değil ÇAĞIRAN dosyanın
  # en üst seviyesi. `-r` okunamayan dosyayı (yok, izin yok) boş değerle
  # eler; `source`'un kendi hatası da ölümcül değil — sözdizimi hatası o
  # dosyayı bırakır, kabuğu değil.
  local file=${ZDOTDIR:-$HOME}/$1
  if [[ -r $file ]]; then
    typeset -g __bateri_file=$file
  else
    typeset -g __bateri_file=
  fi
}

# Yükleme bitti: kullanıcının değerini yeniden okur ve ZDOTDIR'ı bize alır.
#
# Değer YENİDEN OKUNUYOR: bir kullanıcının ZDOTDIR'ı olmasının en yaygın yolu
# `~/.zshenv` içinde onu atamaktır. Okumasaydık kalan dosyaları eski dizinden
# arar, yani tam da kullanıcının taşıdığı yapılandırmayı kaçırırdık.
__bateri_end() {
  if (( ${+ZDOTDIR} )); then
    __bateri_had=1
    __bateri_user=$ZDOTDIR
  else
    __bateri_had=0
    __bateri_user=$HOME
  fi
  ZDOTDIR=$__bateri_dir
  unset __bateri_file
}

# OSC 133 işaretlerini zsh'in kendi kancalarına bağlar.
#
# `.zshrc`'den, kullanıcının dosyası yüklendikten SONRA çağrılıyor:
# `add-zsh-hook` sona ekliyor, yani bizim kancamız kullanıcının kancalarının
# arkasında koşuyor ve onların PS1'e yaptığını görüyor.
__bateri_hooks() {
  autoload -Uz add-zsh-hook
  # "Son prompt'tan beri bir komut koştu mu": `D` yalnız gerçekten koşan bir
  # komutun ardından basılır. Boş satıra basılan Enter yeni bir prompt doğurur
  # ama biten bir komut yoktur.
  typeset -g __bateri_ran=0
  # Blok sayacı. Her prompt bir blok açar ve kimliği hem OSC 133'e
  # (`bt_block=`) hem prompt'un hücrelerine (OSC 8) girer; terminal bloğun
  # hangi satırda başladığını böyle ÖĞRENMEZ, her karede IZGARADAN OKUR.
  # `__bateri_restore` bunu silmiyor: yükleyicinin izleri gidiyor, kancaların
  # oturum durumu değil.
  #
  # ALAN ADI BİZE ÖZEL, `aid` DEĞİL: şartnamede `aid` "uygulama kimliği"dir ve
  # genellikle pid taşır, yani oturum boyunca SABİTTİR. Sayacımızı oraya
  # yazsaydık şartnameye uyan başka bir entegrasyonun sabit değeri bizim
  # defterimizle karışırdı.
  typeset -g __bateri_block=0
  add-zsh-hook precmd __bateri_precmd
  add-zsh-hook preexec __bateri_preexec
  # AYNA: ZLE'nin görüntü durumu her satır çiziminde terminale gidiyor.
  #
  # `zle -N zle-line-pre-redraw` DEĞİL: o bağlama tek sahiplidir ve
  # zsh-syntax-highlighting ile zsh-autosuggestions aynı widget'ı istiyor —
  # son yazan ötekini düşürürdü (011 ölçtü). `add-zle-hook-widget` yerine bir
  # dağıtıcı kurup hepsini sırayla çağırıyor.
  #
  # NÖBET KANCANIN KENDİSİNDE: `add-zle-hook-widget` aynı widget'ı iki kez
  # eklemiyor (`zstyle` listesinde içerme sorar), yani `PS1`'in eklerinde
  # elle yazdığımız nöbetin karşılığı burada hazır. Doğrulandı: iki kez
  # kaydedip `add-zle-hook-widget -L line-pre-redraw` listesi tek satır.
  #
  # BİZDEN SONRA KAYIT OLAN EKLENTİ: `add-zle-hook-widget` sona ekliyor, yani
  # bizim kancamız kullanıcının eklentilerinden SONRA koşuyor ve onların
  # `region_highlight`/`POSTDISPLAY` katkısını görüyor. Bunun iki bilinen
  # sınırı var ve ikisi de belirtisiz: (1) kaydını erteleyen bir eklenti
  # (zsh-defer) bizden sonra gelir ve katkısı aynaya bir çizim GEÇ düşer;
  # (2) `zle -N zle-line-<kanca>` diyen bir eklenti dağıtıcının kendisini ezer
  # ve o kancaya bağlı her şey — bizimki dahil — sessizce ölür. Üç kancadan
  # hangisi ezilirse o kol susuyor ve belirtileri ayrı: `line-pre-redraw`
  # giderse dock donar, `line-init` giderse prompt anında boş kalır,
  # `line-finish` giderse biten komut aynada asılı durur. `line-init` en
  # muhtemel olanı — kullanıcı rc'lerinde imleç şekli için yaygın.
  #
  # `line-init` DE BAĞLI ve bu bir süs değil: `line-pre-redraw` yalnız satır
  # DEĞİŞİNCE koşuyor, prompt'un ilk (boş) çiziminde değil — gerçek bir
  # oturumda gözlendi, ilk ayna ancak ilk tuş vuruşunda geliyordu. Onsuz dock
  # prompt anında ölü kalır ve ilk harfte birden belirirdi.
  autoload -Uz add-zle-hook-widget
  add-zle-hook-widget line-init __bateri_dock_redraw
  add-zle-hook-widget line-pre-redraw __bateri_dock_redraw
  add-zle-hook-widget line-finish __bateri_dock_finish
}

# Prompt çizilmeden önce: biten komutun kodu (`D`), sonra prompt başlangıcı (`A`).
__bateri_precmd() {
  # İLK satır olmak zorunda: sonraki her komut `$?`'ı ezer — `emulate` dahil,
  # o yüzden o da bunun ALTINDA.
  local code=$?
  # Kancanın gövdesi kullanıcının seçenekleriyle koşuyor ve aşağıdaki
  # `psvar[9]` bir DİZİ İNDEKSİ: `KSH_ARRAYS` açıkken atama zsh'in 10.
  # yuvasına düşerken `%9v` hâlâ 9.'yu okur, yani çıpa boş kimlik taşır ve
  # bloklar TANISIZ kaybolur (`/code-review`, 010 phase-2; `zsh -f` ile
  # doğrulandı). `-L` fonksiyon yereldir, dönüşte geri alınır.
  emulate -L zsh
  # `D` BİTEN bloğu kapatıyor, yani kimliği sayaç artmadan ÖNCEKİ değer.
  if (( __bateri_ran )); then
    __bateri_ran=0
    print -nr -- $'\e]133;D;'$code$';bt_block='$__bateri_block$'\a'
  fi
  (( __bateri_block++ ))
  # Kimliği prompt'a taşıyan yuva; `%9v` aşağıdaki çıpada onu okuyor. İndeks
  # iki yerde geçiyor ve birlikte değişmek zorunda.
  #
  # YÜKSEK İNDEKS BİLEREK: `psvar` kullanıcının ad alanı ve alışıldık kullanım
  # baştan birkaç yuva. Kancamız `add-zsh-hook` ile SONA eklendiği için
  # kullanıcının precmd'lerinden sonra koşuyor — `psvar`'ı toptan kuran bir
  # tema bizim yuvamızı ezemiyor.
  psvar[9]=$__bateri_block
  print -nr -- $'\e]133;A;bt_block='$__bateri_block$'\a'
  # ÇIPA: prompt'un hücrelerine binen, kimlik taşıyan bir OSC 8 bağlantısı.
  # Terminal bloğun hangi satırda başladığını hatırlamıyor, her karede
  # ızgaradan okuyor — bu yüzden satır kaydırmadan, pencere yeniden
  # akıtmasından (reflow) ve geçmiş dolduktan sonra da doğru kalıyor.
  #
  # DEĞER GÖMÜLMÜYOR, `%9v` ile prompt anında genişliyor: ek SABİT kalınca
  # aşağıdaki "zaten var mı" nöbeti çalışıyor. Kimlik URI'ye yazılsaydı ek her
  # prompt'ta değişir, nöbet tutamaz ve PS1 her seferinde yıkıcı biçimde
  # sökülüp yeniden kurulurdu — PS1'i kendisi kuran temalarla tam da kaçtığımız
  # yarış. Açılışı `print` ile basmak da çözüm DEĞİL: zsh prompt'u precmd
  # koşmadan yeniden çiziyor (SIGWINCH, Ctrl-L, `zle reset-prompt`) ve o
  # hücreler çıpasız yazılırdı.
  #
  # Açılış ÖNEKTİR: OSC 8 iç içe geçmiyor, yeni URI öncekini değiştiriyor —
  # kendi prompt'unda bağlantı kullanan bir tema varsa ondan ÖNCEKİ hücreler
  # bizim çıpamızı taşır. Temanın bağlantısı ilk karakterde başlıyorsa çıpa
  # hiç doğmaz ve şerit çizilmez; bilinen sınır, yanlış çizim değil.
  #
  # Sağ taraf TIRNAKLI: `[[ ]]` içinde tırnaksız sağ işlenen glob desenidir.
  local anchor_open=$'%{\e]8;;bateri://block/%9v\a%}'
  local anchor_close=$'%{\e]8;;\a%}'
  # Nöbet İÇERME sorar, konum değil (`/code-review`, 010 phase-2): `B` ekinin
  # nöbetiyle aynı biçim. Önek testi PS1'e BAŞKASI dokunduğunda idempotan
  # değil — her precmd'de PS1'i süsleyen bir tema (virtualenv, git bilgisi)
  # ekimizi başa taşımaz, biz de her turda bir yenisini eklerdik ve PS1
  # oturum boyunca sınırsız büyürdü. Ek hâlâ önce basılmaya ÇALIŞIYOR
  # (aşağıdaki gerekçe), ama araya giren bir önek yüzünden ikinci bir çıpa
  # doğurmuyor.
  [[ $PS1 == *"$anchor_open"* ]] || PS1=$anchor_open$PS1
  # `B` prompt'un SONU, yani bir kanca değil prompt'un kendisi.
  # `%{…%}` "sıfır genişlik" demek; olmasaydı zsh kaçış dizisini basılan
  # karakter sayar ve satır kaydırma bozulurdu. Her prompt'ta yeniden
  # denenmesinin sebebi temalar: PS1'i her precmd'de yeniden kuran bir tema
  # bizim ekimizi siler. Koşul da onun için — aynı ek iki kez girmesin.
  [[ $PS1 == *$'\e]133;B\a'* ]] || PS1=$PS1$'%{\e]133;B\a%}'
  # Çıpanın kapanışı en SONDA: prompt'un bütün hücreleri kimliği taşısın.
  # Nöbet yine içerme sorar, sonek değil — açılışla aynı gerekçe.
  [[ $PS1 == *"$anchor_close"* ]] || PS1=$PS1$anchor_close
}

# Komut koşmadan hemen önce: çıktı burada başlıyor (`C`).
__bateri_preexec() {
  __bateri_ran=1
  print -nr -- $'\e]133;C\a'
}

# ── ZLE'nin görüntü aynası ───────────────────────────────────────────────
#
# TEL BİÇİMİ (çözücüsü `bt-core`'un `parse_dock`'u; ikisi birlikte değişir):
#
#   ESC ] 8133 ; u ; CURSOR ; b64(PREDISPLAY) ; b64(BUFFER) ;
#                             b64(POSTDISPLAY) ; b64(region_highlight) BEL
#   ESC ] 8133 ; e BEL   satır bitti (`line-finish`)
#   ESC ] 8133 ; o BEL   görüntü aynaya sığmıyor (aşağıdaki kapı)
#
# GÖVDELER base64: kullanıcının yazdığı metnin içinde `;`, `ESC` ve C0
# baytları olabilir ve üçü de dizinin çerçevesini bozar. base64'ün alfabesinde
# üçünden hiçbiri yok.

# base64 alfabesi, indeks sırasında.
typeset -ga __bateri_b64_table
__bateri_b64_table=( {A..Z} {a..z} {0..9} + / )

# Aynanın taşıyacağı en uzun görüntü, KARAKTER.
#
# Sayı türetildi, seçilmedi — ve terminal tarafındaki `DOCK_PAYLOAD_LIMIT`
# (64 KiB) ile AYNI bütçenin öteki ucu: o sınır "4096 karakter × en kötü 4
# bayt UTF-8 × base64'ün 4/3 şişmesi" aritmetiğinden çıkmıştı, bu onun
# karakter cinsinden hâli.
#
# NEDEN BU UÇTA DA BİR KAPI VAR: kodlama saf zsh ve maliyeti girdinin
# uzunluğuyla doğrusal — üstelik her TUŞ VURUŞUNDA ödeniyor. Kapı olmasaydı
# yapıştırılmış bir blok terminalin zaten reddedeceği bir yükü kodlamak için
# harcanır, yani bedeli öder karşılığını alamazdık. Aşımda ayna "gösteremiyorum"
# diyor ve giriş satırı ızgarada kalıyor; kullanıcı yazdığını yine görüyor.
typeset -g __bateri_dock_limit=4096

# `$1`'i base64'e çevirir; sonuç `REPLY`'de.
#
# FORK YOK: kodlama tuş başına koşuyor ve bir `base64` süreci doğurmak bu
# yolun en pahalı kalemi olurdu. `nomultibyte` her elemanı bir BAYT yapıyor —
# base64 baytların kodlaması, karakterlerin değil.
#
# BAYT DEĞERİ ÖNCE SKALERE ALINIYOR (`x=$bytes[i]`, sonra `#x`), doğrudan
# `##${bytes[i]}` ile DEĞİL: aritmetiğin `##` biçimi kaçış dizisi yorumluyor
# ve ters bölü baytı (`\`) 92 yerine 32 okunuyordu — komut satırında sık geçen
# bir bayt için sessiz bir bozulma.
#
# DOLGU BASILMIYOR: çözücü dolgulu ve dolgusuz gövdeyi birlikte okuyor
# (`decode_base64`'ün doc'u) ve basmamak tuş başına birkaç bayt eksiltiyor.
__bateri_b64() {
  emulate -L zsh
  setopt nomultibyte
  REPLY=
  [[ -n $1 ]] || return 0
  local -a bytes
  bytes=( ${(s::)1} )
  local -i n=$#bytes i v rest
  local out= x y z
  for (( i = 1; i <= n; i += 3 )); do
    rest=$(( n - i + 1 ))
    x=$bytes[i]
    v=$(( #x << 16 ))
    if (( rest > 1 )); then
      y=$bytes[i+1]
      v=$(( v | (#y << 8) ))
    fi
    if (( rest > 2 )); then
      z=$bytes[i+2]
      v=$(( v | #z ))
    fi
    out+=${__bateri_b64_table[$(( (v >> 18 & 63) + 1 ))]}
    out+=${__bateri_b64_table[$(( (v >> 12 & 63) + 1 ))]}
    (( rest > 1 )) && out+=${__bateri_b64_table[$(( (v >> 6 & 63) + 1 ))]}
    (( rest > 2 )) && out+=${__bateri_b64_table[$(( (v & 63) + 1 ))]}
  done
  REPLY=$out
}

# ZLE'nin görüntü durumunu aynaya basar; kancası `line-init` ve
# `line-pre-redraw` (ilki prompt'un ilk çizimi, ikincisi her değişiklik).
#
# BEŞ DEĞİŞKEN, biri eksik olsa ayna kullanıcının gördüğünden az gösterirdi:
# `POSTDISPLAY` autosuggestions'ın önerisi, `region_highlight` de syntax
# highlighting'in rengi.
#
# `emulate -L zsh` ZORUNLU: gövde kullanıcının seçenekleriyle koşuyor ve
# aşağısı hem dizi indeksine (`KSH_ARRAYS`) hem de çok baytlı `${#...}`
# sayımına bağlı. `-L` fonksiyon yereldir, dönüşte geri alınır.
#
# `REPLY` YEREL: kullanıcının ad alanında yaşayan bir değişken ve kancamız
# onun satır düzenlemesinin ortasında koşuyor.
__bateri_dock_redraw() {
  emulate -L zsh
  # Kayıtlar satır sonuyla ayrılıyor; çözücü gövdeyi `lines()` ile okuyor.
  # Birleştirme kapıdan ÖNCE, çünkü dördüncü gövde de kapıya tabi.
  local REPLY entries=${(F)region_highlight} pre buf post highlights
  # Kapı KODLAMADAN ÖNCE, çünkü bütün anlamı kodlamadan kaçınmak — ve DÖRT
  # gövdeyi birden ölçüyor. `region_highlight` ayrı sayılıyor, toplama
  # girmiyor: sözdizimi vurgusu jeton başına bir kayıt bırakıyor, yani uzun
  # bir satırda metnin kendisiyle aynı mertebede ve **kendi başına** sınırı
  # aşabilir (`DOCK_PAYLOAD_LIMIT`'in türetmesi de onu metnin yanında ayrı bir
  # terim sayıyor).
  if (( ${#PREDISPLAY} + ${#BUFFER} + ${#POSTDISPLAY} > __bateri_dock_limit
        || ${#entries} > __bateri_dock_limit )); then
    print -nr -- $'\e]8133;o\a'
    return 0
  fi
  __bateri_b64 "$PREDISPLAY"; pre=$REPLY
  __bateri_b64 "$BUFFER"; buf=$REPLY
  __bateri_b64 "$POSTDISPLAY"; post=$REPLY
  __bateri_b64 "$entries"; highlights=$REPLY
  # `$CURSOR` KARAKTER ofsetidir ve teli de karakter istiyor — `BUFFER`'ın
  # başından sayılan hâli olduğu gibi gidiyor, `PREDISPLAY`'e kaydırmayı
  # sınırın öteki tarafı yapıyor (`DockState::cursor`'ın doc'u).
  print -nr -- $'\e]8133;u;'$CURSOR';'$pre';'$buf';'$post';'$highlights$'\a'
}

# `line-finish`: ZLE satırı bıraktı, ayna kapanıyor.
#
# OLMASAYDI son `BUFFER` asılı kalırdı: Enter'dan sonra dock koşan komutun
# satırını göstermeye devam ederdi.
__bateri_dock_finish() {
  emulate -L zsh
  print -nr -- $'\e]8133;e\a'
}

# Kullanıcının ZDOTDIR'ını KALICI olarak geri koyar ve izlerimizi siler.
#
# Çağıranı `.zshrc` ile `.zlogin`, hangisi okunursa; ayrıca `.zshenv`, bizim
# dosyalarımızdan başkasının okunmayacağı kabuklarda (`no_rcs`; ya da ne
# etkileşimli ne login olan `zsh -c`).
__bateri_restore() {
  if (( __bateri_had )); then
    export ZDOTDIR=$__bateri_user
  else
    unset ZDOTDIR
  fi
  unset __bateri_dir __bateri_user __bateri_had __bateri_file
  # Kancalar kalıyor, yükleyici gidiyor: ilki oturum boyunca çalışıyor,
  # ikincisinin işi bitti ve kullanıcının ad alanında durmasının anlamı yok.
  unfunction __bateri_begin __bateri_end __bateri_hooks __bateri_restore
}
