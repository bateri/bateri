# bateri'nin zsh sarmalayıcısı — ortak gövde.
#
# Yükleyeni bizim ZDOTDIR'ımızdaki dört dosya (`.zshenv`, `.zprofile`,
# `.zshrc`, `.zlogin`); her biri birkaç satır, mantığın tamamı burada. Dört
# kopya dördüncü düzeltmede ayrışırdı.
#
# SÖZLEŞME (kuran taraf `bt-shell`'in `app::shell_integration_env`'i):
#   ZDOTDIR         bu dizin
#   BATERI_ZDOTDIR  kullanıcının özgün ZDOTDIR'ı. Ortamda yoksa kullanıcının
#                   da yoktu; geri koyarken ZDOTDIR silinir, $HOME'a
#                   eşitlenmez — ihraç edilen bir ZDOTDIR ile hiç olmayan
#                   ZDOTDIR çocuklar için farklı şeyler.
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
# [`__bateri_load`] onu düzeltiyor. Kalan kalem salt okunur ve bilerek
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
  if [[ -n ${BATERI_ZDOTDIR} ]]; then
    __bateri_had=1
    __bateri_user=$BATERI_ZDOTDIR
  else
    __bateri_had=0
    __bateri_user=$HOME
  fi
  unset BATERI_ZDOTDIR
fi

# Kullanıcının aynı adlı başlangıç dosyasını yükler.
#
# ZDOTDIR yükleme boyunca KULLANICININ değerini taşıyor ve bunun iki sebebi
# var: dosyanın kendisi `$ZDOTDIR`'ı doğru görsün, ve o dosyadan doğan alt
# süreçler (brew shellenv, nvm, direnv) bizim dizinimizi miras almasın.
#
# Dönüşte değer YENİDEN OKUNUYOR: bir kullanıcının ZDOTDIR'ı olmasının en
# yaygın yolu `~/.zshenv` içinde onu atamaktır. Okumasaydık kalan dosyaları
# eski dizinden arar, yani tam da kullanıcının taşıdığı yapılandırmayı
# kaçırırdık.
__bateri_load() {
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
  local file=${ZDOTDIR:-$HOME}/$1
  # `-r` okunamayan dosyayı (yok, izin yok) sessizce atlar; `source`'un kendi
  # hatası da ölümcül değil — sözdizimi hatası o dosyayı bırakır, kabuğu değil.
  if [[ -r $file ]]; then
    source $file
  fi
  if (( ${+ZDOTDIR} )); then
    __bateri_had=1
    __bateri_user=$ZDOTDIR
  else
    __bateri_had=0
    __bateri_user=$HOME
  fi
  ZDOTDIR=$__bateri_dir
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
  add-zsh-hook precmd __bateri_precmd
  add-zsh-hook preexec __bateri_preexec
}

# Prompt çizilmeden önce: biten komutun kodu (`D`), sonra prompt başlangıcı (`A`).
__bateri_precmd() {
  # İLK satır olmak zorunda: sonraki her komut `$?`'ı ezer.
  local code=$?
  if (( __bateri_ran )); then
    __bateri_ran=0
    print -nr -- $'\e]133;D;'$code$'\a'
  fi
  print -nr -- $'\e]133;A\a'
  # `B` prompt'un SONU, yani bir kanca değil prompt'un kendisi.
  # `%{…%}` "sıfır genişlik" demek; olmasaydı zsh kaçış dizisini basılan
  # karakter sayar ve satır kaydırma bozulurdu. Her prompt'ta yeniden
  # denenmesinin sebebi temalar: PS1'i her precmd'de yeniden kuran bir tema
  # bizim ekimizi siler. Koşul da onun için — aynı ek iki kez girmesin.
  [[ $PS1 == *$'\e]133;B\a'* ]] || PS1=$PS1$'%{\e]133;B\a%}'
}

# Komut koşmadan hemen önce: çıktı burada başlıyor (`C`).
__bateri_preexec() {
  __bateri_ran=1
  print -nr -- $'\e]133;C\a'
}

# Kullanıcının ZDOTDIR'ını KALICI olarak geri koyar ve izlerimizi siler.
#
# Çağıranı `.zshrc` ile `.zlogin`, hangisi okunursa. İkisi birden koşmuyor:
# `.zshrc` geri koyduktan sonra zsh `.zlogin`'i artık kullanıcının dizininde
# arıyor.
__bateri_restore() {
  if (( __bateri_had )); then
    export ZDOTDIR=$__bateri_user
  else
    unset ZDOTDIR
  fi
  unset __bateri_dir __bateri_user __bateri_had
  # Kancalar kalıyor, yükleyici gidiyor: ilki oturum boyunca çalışıyor,
  # ikincisinin işi bitti ve kullanıcının ad alanında durmasının anlamı yok.
  unfunction __bateri_load __bateri_hooks __bateri_restore
}
