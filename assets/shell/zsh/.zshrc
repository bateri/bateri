# bateri — zsh sarmalayıcısı. Gövde `bateri.zsh`'te.
#
# Kancalar kullanıcının dosyasından SONRA bağlanıyor: `add-zsh-hook` sona
# ekliyor ve prompt'a en son dokunan taraf biz oluyoruz.
#
# ZDOTDIR burada geri konuyor, çünkü etkileşimli kabukta okunan son
# dosyamız bu: kalan `.zlogin` ve `.zlogout` artık kullanıcının dizininden
# okunur, yani onları devretmek için ayrıca bir dosya gerekmiyor.
__bateri_load .zshrc
__bateri_hooks
__bateri_restore
