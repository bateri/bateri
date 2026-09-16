# bateri — zsh sarmalayıcısı. Gövde `bateri.zsh`'te.
#
# Kancalar kullanıcının dosyasından SONRA bağlanıyor: `add-zsh-hook` sona
# ekliyor ve prompt'a en son dokunan taraf biz oluyoruz.
#
# ZDOTDIR burada geri konuyor, çünkü etkileşimli kabukta okunan son
# dosyamız bu: kalan `.zlogin` ve `.zlogout` artık kullanıcının dizininden
# okunur, yani onları devretmek için ayrıca bir dosya gerekmiyor.
#
# İlk satırın gerekçesi `.zprofile`'daki ile aynı: dosya kendi kendine yeter
# (ulaşılabilir hâli okunamayan bir `.zshenv`, `no_rcs` değil).
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zshrc
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
  __bateri_hooks
  __bateri_restore
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
