# bateri — zsh sarmalayıcısının ilk halkası. Gövde `bateri.zsh`'te.
#
# Bu dosya her zsh'te okunuyor; kalan üç dosya yalnız ZDOTDIR hâlâ bizi
# gösteriyorsa okunur, yani buradaki geri düşüş onları da kapatır.
if [[ -r ${ZDOTDIR}/bateri.zsh ]] && source ${ZDOTDIR}/bateri.zsh; then
  __bateri_load .zshenv
  # `setopt no_rcs` diyen bir kullanıcı dosyasından sonra zsh başka hiçbir
  # başlangıç dosyası okumaz: geri koyma o kolda buraya düşer, yoksa ZDOTDIR
  # bütün çocuklara sızardı.
  [[ -o rcs ]] || __bateri_restore
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  # Gövde okunamadı: entegrasyon yok ama kabuk yine açılıyor ve kullanıcının
  # yapılandırması yerinde kalıyor — zsh kalan dosyaları onun dizininden okur.
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
