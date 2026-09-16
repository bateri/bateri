# bateri — zsh sarmalayıcısı. Gövde `bateri.zsh`'te.
#
# Bu dosya yalnız `.zshrc` okunmayan bir login kabukta (etkileşimsiz) koşar:
# etkileşimlide `.zshrc` ZDOTDIR'ı çoktan geri koymuştur ve zsh `.zlogin`'i
# kullanıcının dizininde arar. Bizim oturumumuz etkileşimli, yani olağan yol
# burası değil — dosya, ZDOTDIR'ın hiçbir kolda bizde kalmaması için var.
#
# Kanca bağlanmıyor: etkileşimsiz kabukta prompt yok, dolayısıyla işaret de yok.
__bateri_load .zlogin
__bateri_restore
