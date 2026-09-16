# bateri — zsh sarmalayıcısının ilk halkası. Gövde `bateri.zsh`'te.
#
# Bu dosya her zsh'te okunuyor; kalan üç dosya yalnız ZDOTDIR hâlâ bizi
# gösteriyorsa okunur, yani buradaki geri düşüş onları da kapatır.
#
# `source` bu dosyanın EN ÜST SEVİYESİNDE ve bu bir zorunluluk: fonksiyon
# içinden yüklenen bir rc dosyasının `typeset`'leri yerel kalırdı
# (`bateri.zsh`'in başlığı).
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zshenv
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
  # BİZDEN BAŞKA DOSYAMIZIN OKUNMAYACAĞI İKİ KABUK, ikisi de burada
  # toplanıyor — `__bateri_restore` kendini `unfunction` ettiği için iki ayrı
  # koşul iki kez tetiklenemez:
  #   - `no_rcs`: bir kullanıcı dosyası `setopt no_rcs` dedi; zsh o noktadan
  #     sonra başka HİÇBİR başlangıç dosyası okumuyor (`/etc/*` dahil), yani
  #     geri koymanın son şansı burası.
  #   - ne etkileşimli ne login (`zsh -c`): zsh yalnız `.zshenv` okur. Geri
  #     koymazsak ZDOTDIR o kabukta bizde asılı kalır ve ondan doğan her zsh
  #     kullanıcının yapılandırmasını kaybeder. Pencere gerçek: `/etc/zprofile`
  #     ile `/etc/zshrc` tam orada koşuyor.
  if [[ ! -o rcs || ( ! -o interactive && ! -o login ) ]]; then
    __bateri_restore
  fi
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  # Gövde okunamadı: entegrasyon yok ama kabuk yine açılıyor ve kullanıcının
  # yapılandırması yerinde kalıyor — zsh kalan dosyaları onun dizininden okur.
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
