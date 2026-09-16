# bateri — zsh sarmalayıcısı. Gövde `bateri.zsh`'te.
#
# Login kabukta `.zshrc`'den ÖNCE okunuyor ve devredilmesi zorunlu: çoğu
# kurulumda kullanıcının PATH'i (Homebrew, nvm, asdf) burada doğuyor. Yalnız
# `.zshrc`'yi devreden bir sarmalayıcı onu sessizce düşürürdü.
__bateri_load .zprofile
