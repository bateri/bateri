# bateri — zsh sarmalayıcısı. Gövde `bateri.zsh`'te.
#
# Login kabukta `.zshrc`'den ÖNCE okunuyor ve devredilmesi zorunlu: çoğu
# kurulumda kullanıcının PATH'i (Homebrew, nvm, asdf) burada doğuyor. Yalnız
# `.zshrc`'yi devreden bir sarmalayıcı onu sessizce düşürürdü.
#
# İskeletin ilk satırı bu dosyayı KENDİ KENDİNE YETER kılıyor: gövde yüklü
# değilse kendisi yükler. Ulaşılabilir hâli, bizim `.zshenv`'imizin okunamamış
# olması (eksik ya da izni kapalı dosya — elle bozulmuş bir paket); `no_rcs`
# DEĞİL, çünkü o seçenek kapandıktan sonra zsh başka hiçbir başlangıç dosyası
# okumuyor, `/etc/zprofile` dahil (`/code-review`, 009 phase-5: ilk yazımdaki
# gerekçe yanlıştı). Çıplak bir `return` yine de olmaz — `ZDOTDIR` bizde
# asılı kalır, `BATERI_ZDOTDIR` ihraçlı durur ve `elif` kolunun yaptığı geri
# düşüş hiç koşmazdı.
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zprofile
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
