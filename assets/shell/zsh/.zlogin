# bateri — zsh sarmalayıcısı. Gövde `bateri.zsh`'te.
#
# Bu dosya yalnız `.zshrc` okunmayan bir login kabukta (etkileşimsiz) koşar:
# etkileşimlide `.zshrc` ZDOTDIR'ı çoktan geri koymuştur ve zsh `.zlogin`'i
# kullanıcının dizininde arar. Bizim oturumumuz etkileşimli, yani olağan yol
# burası değil — dosya, ZDOTDIR'ın hiçbir kolda bizde kalmaması için var.
#
# Kanca bağlanmıyor: etkileşimsiz kabukta prompt yok, dolayısıyla işaret de yok.
#
# İlk satırın gerekçesi `.zprofile`'daki ile aynı: dosya kendi kendine yeter
# (ulaşılabilir hâli okunamayan bir `.zshenv`, `no_rcs` değil).
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zlogin
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
  __bateri_restore
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
