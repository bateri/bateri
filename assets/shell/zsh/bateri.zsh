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
#   BATERI_DOCK     `off` ise bu oturumda DOCK YOK
#                   (`[shell] integration = "blocks"`): prompt kullanıcının
#                   kalır ve dock'u besleyen kollar hiç kurulmaz. Yokluğu
#                   varsayılan, yani dock VAR. Kararı terminal veriyor,
#                   burada sorulmuyor.
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
  # BU OTURUMDA DOCK VAR MI. Tek değişken, çünkü tek karar: dock varsa giriş
  # satırı TERMİNALİN — prompt sıfırlanır, ZLE aynalanır, bağlamın dalı
  # basılır. Dock yoksa üçü de anlamsız ve üçü de KAPANIR; ayrı ayrı
  # sorulsalardı "prompt terminalin ama dock yok" gibi tutarsız bir hâl
  # mümkün olurdu — 012 phase-10 tam da onu kapattı.
  #
  # Ortam değişkeni yalnız dock'suz kademede geliyor
  # (`shell_integration_env`), yani yokluğu "dock var" demek. Tanınmayan bir
  # değer de oraya düşüyor: bu uçta tanı basacak yer yok ve varsayılana
  # düşmek GÖRÜNÜR bir sonuç (kullanıcı dock'u görür, yanlış yazdığını anlar).
  #
  # `unset`: değişken yalnız BİZE ait ve alt süreçlere sızmasının anlamı yok
  # (`BATERI_ZDOTDIR` emsali). Değeri saklayan kabuk değişkeni kancaların
  # oturum durumu, yani `__bateri_restore` onu SİLMİYOR (`__bateri_block`
  # gibi).
  if [[ $BATERI_DOCK == off ]]; then
    __bateri_dock=0
  else
    __bateri_dock=1
  fi
  unset BATERI_DOCK

  # TAŞAN TAMAMLAMA LİSTESİ EKRANI SİLMESİN. zsh'in varsayılan ölçütü
  # `LISTMAX=100` ve SEÇENEK SAYISINA bakıyor, kapladığı YERE değil: yüzün
  # altında kalan bir liste sormadan basılıyor, satır sayısı ekranı aşsa da.
  # Belirti kullanıcıda görüldü (2026-09-21, `ls -` tamamlaması): liste
  # sorulmadan basıldı, ızgarayı aştı ve satır silinince ekranda KALDI. `0`
  # ölçütü sayıdan yere çeviriyor: "ekrana sığmıyorsa sor". Kaç seçeneğin kaç
  # satır tuttuğu kullanıcının `zstyle`'ına bağlı ve burada bir sayı
  # yazılmıyor — ölçüt zaten sayı değil.
  #
  # Gerekçe ölçüldü (2026-09-21, saf PTY, aynı 37 satırlık liste iki ekran
  # boyunda): liste SIĞDIĞINDA (60 satır) zsh Tab'da `\e[37A` ile imleci
  # listenin üstüne alıyor ve satır silinince `\e[J` gönderiyor — 017'nin
  # doldurma bandı boşluğu defterden dolduruyor ve ekran Tab öncesine
  # dönüyor. Liste AŞTIĞINDA (26 satır) zsh ikisini de göndermiyor; satır
  # silmede yalnız backspace geliyor, çünkü normal bir terminalde kaydırıp
  # geçmişe giden satırları geri getiremez ve yarım temizlemek ekranı
  # bozardı. Terminale "liste bitti" diyen bir sinyal HİÇ gelmiyor: defter
  # bizde duruyor ama geri getirmenin tetiği yok. `LISTMAX=0` ekranı silmeyi
  # geri dönüşsüz bir adım olmaktan çıkarıyor — aşan listede zsh önce soruyor
  # ve `n` ekranı olduğu gibi bırakıyor.
  #
  # BİLİNEN SINIR: `y` dendiğinde liste basılıyor ve yine kalıcı oluyor.
  # Ölçüt "bozulmadan önce sor", "geri getir" değil.
  #
  # DEĞER KULLANICININ DOSYALARINDAN ÖNCE KONUYOR ve yerin kendisi bir karar:
  # `LISTMAX` zsh'te varsayılan olarak SET (`typeset -i LISTMAX=100`), yani
  # "kullanıcı mı ayarlamış" diye sınanamaz. Bu blok `.zshenv`'den bir kez
  # koşuyor, kullanıcının hiçbir başlangıç dosyası okunmadan önce; kendi
  # `LISTMAX`'ını yazan kullanıcı SONRA koşuyor ve kazanıyor. Değer
  # `__bateri_hooks` içine konsaydı tam tersi olur, kullanıcının tercihi
  # ezilirdi.
  #
  # DOCK'A KOŞULLU, çünkü koruduğu şey dock'un vaadi (017, ekranın geri
  # dönüşü) ve `integration = "blocks"` kademesinde o vaat yok — orada giriş
  # satırı kullanıcının ve kabuk klasik davranmalı. `__bateri_dock`'un
  # tükettiği üçüncü karar; ayrı bir anahtar açılsaydı "dock yok ama
  # completion bizim" gibi tutarsız bir hâl doğardı.
  if (( __bateri_dock )); then
    LISTMAX=0
  fi
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
  # ÇIPA: prompt'un hücrelerine binen, kimlik taşıyan bir OSC 8 bağlantısı.
  # Terminal bloğun hangi satırda başladığını hatırlamıyor, her karede
  # ızgaradan okuyor — bu yüzden satır kaydırmadan, pencere yeniden
  # akıtmasından (reflow) ve geçmiş dolduktan sonra da doğru kalıyor.
  #
  # DEĞER GÖMÜLMÜYOR, `%9v` ile prompt anında genişliyor: PS1'e eklenen parça
  # SABİT kalınca `shell` kolunun "zaten var mı" nöbeti çalışıyor. Kimlik
  # URI'ye yazılsaydı ek her prompt'ta değişir, nöbet tutamaz ve PS1 her
  # seferinde yıkıcı biçimde sökülüp yeniden kurulurdu — PS1'i kendisi kuran
  # temalarla tam da kaçtığımız yarış. Açılışı `print` ile basmak da çözüm
  # DEĞİL: zsh prompt'u precmd koşmadan yeniden çiziyor (SIGWINCH, Ctrl-L,
  # `zle reset-prompt`) ve o hücreler çıpasız yazılırdı.
  #
  # Açılış ÖNEKTİR: OSC 8 iç içe geçmiyor, yeni URI öncekini değiştiriyor —
  # `shell` kolunda kendi prompt'unda bağlantı kullanan bir tema varsa ondan
  # ÖNCEKİ hücreler bizim çıpamızı taşır. Temanın bağlantısı ilk karakterde
  # başlıyorsa çıpa hiç doğmaz ve şerit çizilmez; bilinen sınır, yanlış çizim
  # değil.
  typeset -g __bateri_anchor=$'%{\e]8;;bateri://block/%9v\a%}'
  # Prompt TERMİNALİN olduğunda PS1: iki sıfır genişlikli işaret ve **iki
  # gerçek boşluk**. `>` dock'ta ve ızgarada terminalin kendisi çiziyor, ama
  # ızgarada onu koyacak yer lazım — o yer bu iki sütun.
  #
  # BOŞLUKLAR ÇİZİM HİLESİ DEĞİL, GERÇEK GENİŞLİK. Alternatifi komut satırını
  # çizerken iki sütun sağa kaydırmaktı ve üç şeyi birden bozardı: fare
  # eşlemesi o satırda kayardı, tam genişlikteki bir komutun son iki karakteri
  # ekrandan taşardı ve zsh satır sarmayı yanlış hesaplardı. Prompt gerçekten
  # iki sütunsa üçü de kendiliğinden doğru — zsh zaten prompt genişliğini
  # biliyor.
  #
  # SAYI `bt-core`'un `dock::TEXT_COL`'u ile AYNI olmak zorunda: dock'un metni
  # de işaretten iki sütun sonra başlıyor ve ikisi ayrışırsa ızgara ile dock
  # farklı sütundan başlar. Sabit paylaşılamıyor (biri zsh, biri Rust), o
  # yüzden bir sınama bu satırı okuyup sayıyı bağlıyor.
  #
  # SIRA: çıpa → boşluklar → `B`. `B` prompt'un SONU, yani girdinin başladığı
  # yer; boşluklar ondan önce olmak zorunda. Çıpa en başta, çünkü açtığı
  # bağlantıyı boşluklar da taşıyor — ve bu bir yan kazanç: boş promptta bile
  # çıpalı bir hücre var, oysa sıfır genişlikli PS1'de hiç yoktu.
  #
  # `%{…%}` "sıfır genişlik" demek; boşluklar bilerek DIŞARIDA, sayılmalılar.
  typeset -g __bateri_ps1=$__bateri_anchor'  '$'%{\e]133;B\a%}'
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
  #
  # DOCK YOKSA HİÇ KURULMUYOR. Aynanın tek tüketicisi dock; `blocks`
  # kademesinde kancalar kurulsaydı her tuş vuruşunda beş değişken base64'e
  # kodlanıp akışa yazılır ve okuyan kimse olmazdı. Tuş başına ödenen bir
  # bedelin karşılıksız kalması, ölçülmemiş olsa bile kabul edilebilir
  # değil — hele maliyetin şekli zaten borç listesinde dururken.
  if (( __bateri_dock )); then
    autoload -Uz add-zle-hook-widget
    # DÜZENLEME WIDGET'I (031): terminalin tek komutu buraya gidiyor. Tanımı
    # bir kez, bağlaması her `line-init`'te (`__bateri_dock_arm`) — ve arm
    # aynadan ÖNCE kayıtlı, yani yetenek aynı prompt'un ilk aynasından önce
    # telde.
    zle -N __bateri_dock_edit
    add-zle-hook-widget line-init __bateri_dock_arm
    add-zle-hook-widget line-init __bateri_dock_redraw
    add-zle-hook-widget line-pre-redraw __bateri_dock_redraw
    add-zle-hook-widget line-finish __bateri_dock_finish
  fi
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
  __bateri_prompt_set
  # DOCK'UN BAĞLAM SATIRI. Prompt başına, tuş başına DEĞİL: ikisi de değişmek
  # için bir komut bekliyor (`cd`, `git checkout`) ve o komut bittiğinde
  # buradayız.
  # OSC 7 KALIYOR, DALIN FORK'U KALMIYOR. İkisi de bugün yalnız dock'un bağlam
  # satırını besliyor ama bedelleri kıyaslanamaz: OSC 7 tek bir `print` ve
  # STANDART bir dizi (yeni sekmeyi aynı dizinde açmak gibi işlerin yolu, yani
  # dock'tan bağımsız bir geleceği var). Dal ise prompt başına bir `git`
  # FORK'U ve tek tüketicisi dock — dock yokken ödenmesi saf israf.
  __bateri_cwd
  (( __bateri_dock )) && __bateri_branch_print
}

# Çalışma dizinini OSC 7 ile bildirir.
#
# YETKİ BÖLÜMÜ BOŞ (`file:///…`), `file://$HOST…` DEĞİL: terminal tarafındaki
# kapı adlı her host'u yabancı sayıyor (`LOCAL_AUTHORITIES`) ve bunun sebebi
# bir eksiklik değil, bir bağımlılık kararı — ad karşılaştırması `bt-core`'a
# `gethostname` demek. Boş yetkiyle basınca kapı hiçbir zaman bir ad
# uyuşmasına bağlı olmuyor: makine yeniden adlandırılsa da dizin görünür.
#
# YÜZDE KODLAMASI ZORUNLU: `$PWD` içinde boşluk, `%`, `;` ve çok baytlı
# karakterler olabilir; `;` OSC alanını, kontrol baytları diziyi bozardı.
__bateri_cwd() {
  emulate -L zsh
  local REPLY
  __bateri_percent "$PWD"
  print -nr -- $'\e]7;file://'$REPLY$'\a'
}

# Git dalını aynanın kanalından gönderir; depo değilse gövde BOŞ.
#
# TEK FORK, olağan hâlde: `--abbrev-ref` depo dışında da tek çağrı, detached
# HEAD'de ikinci bir çağrı kısa SHA için. Bedel PROMPT başına ve büyük depoda
# hissedilir — p10k'nın `gitstatusd` daemon'ı bu yüzden var; hızlandırma ayrı
# bir iş (`plan.md` → Kapsam Dışı).
#
# `command`: kullanıcının `git` alias'ı ya da fonksiyonu araya girmesin.
__bateri_branch_print() {
  emulate -L zsh
  local REPLY ref
  ref=$(command git rev-parse --abbrev-ref HEAD 2>/dev/null)
  # `HEAD` bir dal adı değil, detached HEAD'in cevabı: yerine kısa SHA.
  if [[ $ref == HEAD ]]; then
    ref=$(command git rev-parse --short HEAD 2>/dev/null)
  fi
  __bateri_b64 "$ref"
  print -nr -- $'\e]8133;b;'$REPLY$'\a'
}

# Prompt'u bu oturumun sahibine göre kurar; çağıranı `precmd`.
#
# İKİ KOL, tek fark PS1'in SAHİBİ:
#
# - `terminal` (varsayılan): PS1 bütünüyle BİZİM ve görünür genişliği sıfır.
#   Kullanıcının prompt'u çizilmiyor; yerine dock'un `>` işareti geçiyor.
#   `RPS1`/`RPROMPT` de boşalıyor ve bu bir ayrıntı değil ZORUNLU: sağ prompt
#   PS1'den bağımsız yaşıyor, yalnız PS1'i sıfırlamak ekranın sağında asılı
#   bir tema parçası bırakırdı.
# - dock YOKSA (`integration = "blocks"`): kullanıcının prompt'u yerinde, biz
#   yalnız işaretleri EKLİYORUZ (010'un yolu). Bir dönem "prompt kullanıcının
#   ama dock yine de açık" diye üçüncü bir hâl vardı ve ekranda İKİ PROMPT
#   üretiyordu; tek karara indirildi (012 phase-10).
__bateri_prompt_set() {
  if (( __bateri_dock )); then
    PS1=$__bateri_ps1
    RPS1=
    RPROMPT=
    return 0
  fi
  # Nöbet İÇERME sorar, konum değil (`/code-review`, 010 phase-2): `B` ekinin
  # nöbetiyle aynı biçim. Önek testi PS1'e BAŞKASI dokunduğunda idempotan
  # değil — her precmd'de PS1'i süsleyen bir tema (virtualenv, git bilgisi)
  # ekimizi başa taşımaz, biz de her turda bir yenisini eklerdik ve PS1
  # oturum boyunca sınırsız büyürdü.
  #
  # Sağ taraf TIRNAKLI: `[[ ]]` içinde tırnaksız sağ işlenen glob desenidir.
  [[ $PS1 == *"$__bateri_anchor"* ]] || PS1=$__bateri_anchor$PS1
  # `B` prompt'un SONU, yani bir kanca değil prompt'un kendisi. Her prompt'ta
  # yeniden denenmesinin sebebi temalar: PS1'i her precmd'de yeniden kuran bir
  # tema bizim ekimizi siler. Koşul da onun için — aynı ek iki kez girmesin.
  [[ $PS1 == *$'\e]133;B\a'* ]] || PS1=$PS1$'%{\e]133;B\a%}'
}

# Temanın geri yazdığı prompt'u geri alır; çağıranı aynanın ZLE kancası.
#
# NEDEN PRECMD YETMİYOR: p10k ve starship PS1'i `precmd`'den SONRA, kendi ZLE
# kancalarından yeniden kuruyor ve `zle reset-prompt`'luyor — precmd'de
# yazdığımız değer ekrandan siliniyor. Aynı yerden dayatılmazsa tema kazanır.
#
# NEDEN TEK BAŞINA DA YETMİYOR — ÖLÇÜLDÜ, seçilmedi: ZLE kancasından atanan
# PS1 kendiliğinden HİÇBİR ŞEY yapmıyor. Prompt `line-init` koşmadan önce
# basılıyor ve zsh genişlettiği hâli tutuyor; `zsh -i` PTY probe'unda kancadan
# atanan değer ekranda hiç görünmedi. Etkili olmasının tek yolu
# `zle reset-prompt`. Yani ikisi BİRLİKTE gerekiyor: precmd ilk basımı
# doğru yapıyor, bu kanca temanın geri yazdığını geri alıyor.
#
# PRECMD'İ ATMANIN İKİ BEDELİ ÖLÇÜLDÜ ve ikisi de precmd'yi hak ettiriyor:
# (1) yalnız `precmd`'den kuran bir temada (starship) prompt ilk basımda
# zaten doğru olur ve nöbet hiç sıfırlamaz — precmd olmasaydı HER prompt bir
# `reset-prompt` yeniden çizimi öderdi; (2) `zle -N zle-line-init` diyen bir
# eklenti dağıtıcıyı ezerse (bu dosyanın en muhtemel saydığı sınır) bu kanca
# büsbütün susar ve prompt geri gelirdi. Tersi de doğru: yalnız precmd
# kalsaydı p10k prompt'u geri yazardı.
#
# NÖBET PING-PONG'U KESİYOR: `reset-prompt` yeni bir çizim doğuruyor, o çizim
# de `line-pre-redraw`'ı yeniden çağırıyor. Koşulsuz bir sıfırlama kendi
# kendini besleyen bir döngü olurdu; nöbetle prompt başına TEK sıfırlama
# ölçüldü.
#
# DOCK'SUZ KOLDA SUSUYOR: prompt'unu geri isteyen kullanıcının temasıyla
# kavga etmenin anlamı yok.
__bateri_prompt_guard() {
  (( __bateri_dock )) || return 0
  [[ $PS1 == "$__bateri_ps1" && -z $RPS1 && -z $RPROMPT ]] && return 0
  PS1=$__bateri_ps1
  RPS1=
  RPROMPT=
  zle reset-prompt
}

# Komut koşmadan hemen önce: çıpa kapanıyor, çıktı başlıyor (`C`).
__bateri_preexec() {
  __bateri_ran=1
  # ÇIPANIN KAPANIŞI BURADA, PROMPT'UN SONUNDA DEĞİL — ve bu, sıfır genişlikli
  # PS1'in zorunlu eşlikçisi: PS1 artık hiçbir hücre yazmıyor, yani kapanış
  # PS1'in sonunda kalsaydı ÇIPAYI TAŞIYAN HÜCRE HİÇ DOĞMAZDI. Blok şeridi de
  # giriş satırının bastırılması da o hücreden türüyor (`Session::frame`),
  # yani ikisi birden sessizce ölürdü.
  #
  # Bağlantı `Input` boyunca AÇIK kalıyor: ZLE'nin yazdığı her hücre kimliği
  # taşıyor. Komutun ÇIKTISI taşımıyor, çünkü kapanış çıktıdan hemen önce —
  # işaret komutun kendi satırında, çıktısında değil.
  #
  # KOŞULSUZ, iki kolda da: `shell` kolunda da giriş satırı bastırılıyor
  # (dock kapanmıyor) ve o da aynı çıpaya bakıyor.
  #
  # BİLİNEN SINIR: `preexec` koşmayan yollarda (Ctrl-C, boş satıra Enter)
  # bağlantı bir sonraki prompt'un PS1 genişlemesine kadar açık kalıyor.
  # Ölçüldü: o pencerede yalnız kullanıcının kendi `precmd` kancalarının
  # bastığı hücreler var ve onlar ÖNCEKİ bloğun kimliğini taşıyor — kancamız
  # `add-zsh-hook` ile sona eklendiği için onlardan sonra koşuyoruz, yani
  # daha erken kapatmanın yolu yok. Yön güvenli: fazladan bir şerit işareti
  # çizilir, bastırma ise etkilenmez (o bloğun kimliği artık yazılan blok
  # değil).
  print -nr -- $'\e]8;;\a'
  print -nr -- $'\e]133;C\a'
}

# ── ZLE'nin görüntü aynası ───────────────────────────────────────────────
#
# TEL BİÇİMİ (çözücüsü `bt-core`'un `parse_dock`'u; ikisi birlikte değişir):
#
#   ESC ] 8133 ; u ; CURSOR ; b64(PREDISPLAY) ; b64(BUFFER) ;
#                             b64(POSTDISPLAY) ; b64(region_highlight) ;
#                             b64(KEYMAP) BEL
#   ESC ] 8133 ; e BEL   satır bitti (`line-finish`)
#   ESC ] 8133 ; o BEL   görüntü aynaya sığmıyor (aşağıdaki kapı)
#   ESC ] 8133 ; b ; b64(dal) BEL   bağlam satırının dalı (`precmd`)
#   ESC ] 8133 ; w BEL   bu prompt'ta düzenleme widget'ı bağlı (`line-init`)
#
# TERS YÖN — TERMİNALDEN KABUĞA, telin tek böyle dizisi (031):
#
#   ESC [ 8133 ~ d ; S ; E ; L BEL
#
# `BUFFER`'ın `[S, E)` karakter aralığını sil, caret'i `S`'e koy; `S == E`
# yalnız caret'i taşır. `L` terminalin gördüğü `${#BUFFER}`: tutmazsa
# terminal bayat bir aynaya bakıyordu ve widget HİÇBİR ŞEY yapmıyor. Dizi
# kullanıcının girdisiyle aynı PTY'den geliyor ve ZLE onu bir tuş gibi
# okuyor; `CSI 8133 ~` hiçbir klavyenin üretmediği bir tuş, numarası aynanın
# numarası. Metin tele HİÇ girmiyor: seçimin yerine yazılan harf `d`'den
# sonra olağan yoldan geliyor, yani kabukta base64 çözücü yok ve harf yine
# `self-insert`'ten geçiyor. Terminal diziyi yalnız bu prompt'ta `w`'yi
# gördüyse gönderiyor — bağlamasız bir kabukta sondaki BEL `send-break`
# olurdu (ölçüldü, 031 discussion → Muhakeme).
#
# KEYMAP ALTINCI GÖVDE ve taşıdığı şey bir POLİTİKA DEĞİL, ZLE'nin durumu:
# hangi keymap'lerin "yazılan tuş metne dönüşür" anlamına geldiğine karar veren
# taraf terminal (`bt-core`, `insert_keymap`). Adı olduğu gibi gönderiyoruz,
# çünkü `bindkey -N` ile kullanıcı kendi keymap'ini yaratabiliyor ve bu uçta
# onu sınıflandıracak bilgi yok. base64, çünkü o ad `;` taşıyabilir.
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

# Onaltılık haneler, yüzde kodlamasının iki basamağı için.
typeset -ga __bateri_hex
__bateri_hex=( 0 1 2 3 4 5 6 7 8 9 A B C D E F )

# `$1`'i yüzde kodlar (RFC 3986'nın "unreserved" kümesi + `/`); sonuç `REPLY`'de.
#
# FORK YOK, `__bateri_b64` ile aynı gerekçe ve aynı iki incelik: `nomultibyte`
# her elemanı bir BAYT yapıyor (yüzde kodlaması baytların, karakterlerin
# değil) ve bayt değeri önce skalere alınıyor (`x=…`, sonra `#x`) — aritmetiğin
# `##` biçimi kaçış dizisi yorumluyor ve ters bölüyü 92 yerine 32 okuyor.
#
# `/` KODLANMIYOR: yol ayracı ve kodlanmış bir `/` yolu tek bir bileşen gibi
# gösterirdi. Çözen taraf ikisini de okuyor, yani bu bir zorunluk değil
# okunabilirlik: kullanıcının yolu bize de insan gözüyle bakılabilir kalıyor.
__bateri_percent() {
  emulate -L zsh
  setopt nomultibyte
  REPLY=
  [[ -n $1 ]] || return 0
  local -a bytes
  bytes=( ${(s::)1} )
  local out= x
  local -i v
  for x in $bytes; do
    if [[ $x == [A-Za-z0-9/._~-] ]]; then
      out+=$x
    else
      v=$(( #x ))
      out+='%'${__bateri_hex[$(( (v >> 4) + 1 ))]}${__bateri_hex[$(( (v & 15) + 1 ))]}
    fi
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
  # PROMPT'UN DAYATILMASI, aynadan ÖNCE ve aynı kancadan: gerekçesi
  # `__bateri_prompt_guard`'ın başlığında. Aynanın kendi yük kapısının
  # üstünde, çünkü prompt'un sahipliği yükün uzunluğuna bağlı değil — taşan
  # bir satırda ayna susarken temanın prompt'u geri gelseydi belirti de
  # açıklanamaz olurdu.
  __bateri_prompt_guard
  # Kayıtlar satır sonuyla ayrılıyor; çözücü gövdeyi `lines()` ile okuyor.
  # Birleştirme kapıdan ÖNCE, çünkü dördüncü gövde de kapıya tabi.
  local REPLY entries=${(F)region_highlight} pre buf post highlights keymap
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
  # KEYMAP kapının DIŞINDA sayılıyor: en uzun keymap adı bir avuç bayt ve onu
  # yük bütçesine katmak, sınırı taşan bir satırda aynanın susmasına ikinci bir
  # gerekçe eklerdi.
  __bateri_b64 "$KEYMAP"; keymap=$REPLY
  # `$CURSOR` KARAKTER ofsetidir ve teli de karakter istiyor — `BUFFER`'ın
  # başından sayılan hâli olduğu gibi gidiyor, `PREDISPLAY`'e kaydırmayı
  # sınırın öteki tarafı yapıyor (`DockState::cursor`'ın doc'u).
  print -nr -- $'\e]8133;u;'$CURSOR';'$pre';'$buf';'$post';'$highlights';'$keymap$'\a'
}

# `line-finish`: ZLE satırı bıraktı, ayna kapanıyor.
#
# OLMASAYDI son `BUFFER` asılı kalırdı: Enter'dan sonra dock koşan komutun
# satırını göstermeye devam ederdi.
__bateri_dock_finish() {
  emulate -L zsh
  print -nr -- $'\e]8133;e\a'
}

# Düzenleme komutunun yükünü beklemenin üst sınırı, SANİYE.
#
# Terminal diziyi tek yazımda gönderiyor, yani yük widget koştuğunda zaten
# okunmayı bekliyor ve süre hiç harcanmıyor. Sınır yalnız BOZUK bir telde —
# BEL'i gelmeyen bir dizide — ZLE'nin ne kadar donacağını belirliyor:
# kullanıcının fark edeceği ama kabuğu kilitlemeyecek bir an. Ölçülmüş bir
# sayı değil, bir his eşiği (emsal `HANDOVER_HOLD`).
typeset -g __bateri_dock_edit_wait=0.5

# Terminalin düzenleme komutu (`CSI 8133 ~ d;S;E;L BEL`, tel başlığı yukarıda).
#
# HER KOŞULDA SESSİZ: bozuk yük, tutmayan `L` ya da aralık dışı sayı
# `BUFFER`'a dokunmadan dönüyor. Yanlışın yönü "düzenleme olmadı" — satırı
# bozmaktansa kullanıcının tuşu boşa gitsin.
#
# `S == E` BUFFER'A YAZMIYOR, yalnız `CURSOR`: atama boş bile olsa bir geri
# alma kaydı doğururdu. Silme ise ZLE'nin tek geri alma birimi (ölçüldü,
# `context.md` → Ölçüm).
#
# AYNA WIDGET'IN SONUNDA AÇIKÇA BASILIYOR: `line-pre-redraw` yalnız görüntü
# değişince koşuyor ve caret'i zaten olduğu yere koyan (ya da `L` tutmadığı
# için hiçbir şey yapmayan) bir komut hiç ayna doğurmazdı. Terminal her
# girdisine bir cevap bekliyor (`DockState::answers`); cevapsız kalan komut
# düzenleme kapısını bir sonraki tuşa kadar kapalı bırakırdı.
__bateri_dock_edit() {
  emulate -L zsh
  local payload= ch=
  while read -k 1 -t $__bateri_dock_edit_wait ch; do
    [[ $ch == $'\a' ]] && break
    payload+=$ch
    # Meşru yük dört sayı: sınırsız okumak bozuk bir telde satırı yutardı.
    (( ${#payload} > 64 )) && break
  done
  if [[ $ch == $'\a' && $payload == d\;<->\;<->\;<-> ]]; then
    local -a field
    field=( ${(s:;:)payload} )
    local -i start=$field[2] end=$field[3] len=$field[4]
    if (( len == ${#BUFFER} && start <= end && end <= len )); then
      if (( start < end )); then
        BUFFER=${BUFFER[1,start]}${BUFFER[end+1,-1]}
      fi
      CURSOR=$start
    fi
  fi
  __bateri_dock_redraw
}

# `line-init`: widget'ı bağla ve yeteneği bildir.
#
# HER PROMPT'TA YENİDEN, çünkü bağlama kalıcı değil: `bindkey -v`/`-e` yeni
# bir keymap'i `main`'e bağlıyor, ertelenmiş bir eklenti keymap'i
# sıfırlayabiliyor ve `bindkey -A mymap main` diyen kullanıcının keymap'i
# bizim kurulumumuzdan sonra doğuyor. Üçü de bir sonraki prompt'ta
# onarılıyor. Bedeli üç yerleşik — fork yok.
#
# `main` DAHİL: kullanıcının kendi keymap'ini `main`'e bağlaması yaygın ve
# `emacs`/`viins`'e bağlamak onu kapsamazdı. Terminal diziyi yalnız ekleme
# keymap'inde gönderiyor (`INSERT_KEYMAPS`), yani `vicmd` bilerek dışarıda.
#
# `w` BAĞLAMADAN SONRA: yetenek "şu an bağlı" demek. Terminal onu
# `line-finish`'te (`e`) unutuyor, yani betiği eski olan ya da bu kanca
# ezilmiş bir oturumda kapı kapalı kalıyor ve dizi hiç gönderilmiyor.
__bateri_dock_arm() {
  emulate -L zsh
  local map
  for map in main emacs viins; do
    bindkey -M $map $'\e[8133~' __bateri_dock_edit
  done
  print -nr -- $'\e]8133;w\a'
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
