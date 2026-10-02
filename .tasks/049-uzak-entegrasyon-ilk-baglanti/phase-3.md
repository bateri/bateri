# Phase 3 — ssh'ın öbür ucuna terminal kimliği (`LC_` ailesi)

## Özet

Pane'in kabuğuna `LC_TERMINAL=bateri`, `LC_TERMINAL_VERSION` ve
`LC_BATERI_TAB_URL` koy; sarılmış oturumda uzak betik de aynılarını
dışa aktarsın — uzaktaki araçlar terminali ve sekmeyi tek bir addan tanısın
(kullanıcı isteği 2026-10-03).

_Requirements: R6, R6.1, R6.2, R7_

## Değişiklikler

- **`crates/bt-core/src/identity.rs`** (ya da kimlik ortamının kurulduğu
  yer, 038) — `TERM_PROGRAM` ailesinin yanına üç değişken:
  `LC_TERMINAL=bateri`, `LC_TERMINAL_VERSION` (workspace sürümü, mevcut
  `TERM_PROGRAM_VERSION` ile aynı kaynak), `LC_BATERI_TAB_URL` (pane'in
  `BATERI_TAB_URL`'siyle aynı değer). Miras kalan `LC_TERMINAL=iTerm2`
  **ezilir** — `TERM_PROGRAM=Apple_Terminal` mirasının emsali (038 →
  context → Kanıt); `SessionOptions.env`'in ek ortamı bunları ezemez, tıpkı
  `TERM`/`COLORTERM` gibi. Değer hiçbir zaman `iTerm2` taklidi değildir.
- **Taşıma:** macOS'un ssh istemcisi `SendEnv LANG LC_*` ile geliyor,
  Debian/Ubuntu/macOS sshd'leri `AcceptEnv LANG LC_*` ile kabul ediyor
  (ikisi de bu makinede ve Debian imajında okundu) — düz ssh'ta ek bir şey
  gerekmiyor. **Sarılmış oturumda** uzak önyükleme (`assets/shell/remote/boot.sh`)
  üç değişkeni giriş kabuğunu açmadan önce dışa aktarır; değerler sarılmış
  argv'de gider (nonce emsali, `ssh_wrap::wrap`), `ps`'te görünmeleri
  zararsız (sekme adresi yalnız odaklar, 038 Karar 5). Sunucu `LC_*`'ı zaten
  getirdiyse aynı değer, üzerine yazmak sorun değil. `unwrap` bu değerleri de
  argv'den düşürür (gidiş-dönüş sınaması).
- **Düşme kararı (R7)** — `ssh-fell-back`'in kararına "girişten sonra
  kullanıcı yazdı mı" girdisi: pane, sarılmış oturumda 047'nin giriş
  kenarından sonra girdi nesli (`key_gen`) ilerleyince nonce'un yanına
  "kullanıldı" kaydı düşer (nonce kanıtının `remote-hosts.up/{nonce}`
  emsali); kayıt varsa düşme yok. phase-2'nin ölçtüğü `ForceCommand`'lı
  CLI senaryosu sınamaya girer (çıkıştan sonra yeniden bağlanma yok).
- **`CLAUDE.md`** — kimlik paragrafına (`TERM_PROGRAM=bateri`, …) tek cümle:
  `LC_` ailesi ssh'tan geçen kimlik, iTerm2'nin `LC_TERMINAL` emsali; sarılmış
  oturumda uzak betik de koyuyor; `AcceptEnv` kısıtlı sunucuda sarılmayan
  oturumda yok (bilinen sınır).

## Kabul

- Yerel: pane'in kabuğunda üç değişken var; `LC_TERMINAL=iTerm2` mirasıyla
  doğan oturumda değer `bateri` (sınama, `identity`'nin mevcut sınamalarının
  yanında).
- Yerel ve uzakta yan etki yok: tanınmayan `LC_` adıyla `locale`, `perl -e1`
  ve `python3 -c 1` uyarı basmıyor (Docker imajında ve yerelde sınandı,
  sonuç Uygulama Notları'nda).
- Docker sshd: düz ssh'ta üç değişken uzakta görünüyor; `AcceptEnv`'i
  kısıtlı (yalnız `LANG`) bir sshd'de sarılmış oturumda yine görünüyor,
  sarılmamışta görünmüyor (bilinen sınır). mosh'un taşıyıp taşımadığı
  ölçülüp Uygulama Notları'na yazılır (mosh kurulu değilse `[~]`).
- `unwrap(wrap(x)) == x` değişkenlerle birlikte yeşil.

## Uygulama Notları

- **Kimlik `bt-core`'da, `TERM_PROGRAM`'ın katmanında** (`Session::spawn`):
  `LC_TERMINAL` (`identity::LC_TERMINAL` = `TERM_PROGRAM`) ve
  `LC_TERMINAL_VERSION` koşulsuz, `LC_BATERI_TAB_URL` yalnız `tab_id`
  varken (`BATERI_TAB_URL`'nin kuralı). Bekçi
  `lc_identity_env_overrides_an_inherited_one` (ek ortamda `iTerm2` → `bateri`).
- **Sarılmış komutun kuyruğu dört kelime**: `bateri-boot <P|-> <nonce>
  <sürüm> <sekme|->`. Sürüm `bt_core::TERM_PROGRAM_VERSION` (`ssh-argv` aynı
  ikili), sekme `ssh-argv`'nin ortamındaki `BATERI_TAB_URL` — `TabId::from_url`'dan
  geçmeyen sekme yok (`-`). İki kelime tırnağın dışında ve giriş kabuğu onları
  okuyor, o yüzden ikisi de biçimle kapılı: sürüm `[0-9A-Za-z.+-]`
  (`is_version`), sekme `TabId`'nin biçimi; `boot_tail` aynı kapıyla okuyor,
  phase-1'in iki kelimelik biçimi ve 048'inkiler hâlâ açılıyor. `tab`
  parametresi `remote_command → wrap → decide → ssh_argv_main` boyunca
  geçiyor, hiçbir kararı değiştirmiyor.
- **`boot.sh` kimliği `up`'tan hemen sonra, motd'dan ve her hatadan önce
  dışa aktarıyor** (adım 0b): düz giriş kabuğuna düşen kollar (`f;shell`,
  `f;write`) da onu taşıyor. `LC_TERMINAL=bateri` sabit (dosyayı yalnız
  bateri koşturuyor), `$3`/`$4` kendi biçimlerinde değilse dışa aktarılmıyor.
  **Bilinen sınır**: tek satırın çözücü hatası kolu (`f;decode`) dışa
  aktarmıyor — `[!…]` tek satırda yasak ve kol base64 çözücüsü olmayan sunucu;
  orada da `AcceptEnv LC_*` taşıyabilir. Bekçi
  `the_bootstrap_exports_the_identity` (zsh ve `sh` kolları, bozuk sürüm/sekme,
  miras `iTerm2` ezilir).
- **R7 — "kullanıldı" kaydı**: `{nonce}.used` (`ssh_wrap::mark_used`,
  `mark_up`'ın ikizi, aynı dizin, aynı biçim kapısı, `mark_up`'ın süpürmesine
  giriyor); `take_up` iki dosyayı da tüketiyor ve biri varsa "kullanıcınındı"
  diyor — `ssh_fell_back_main`'in iki sorusu da değişmeden. Kenar `bt-core`'da:
  `ShellLog::typed` (komut nesline bağlı, `login`'in emsali) `send_input`'un
  **zaten aldığı** kabuk kilidinde kuruluyor (`note_typed`; tuş başına yeni
  kilit yok), kilit bırakılınca `Wake::remote_typed` (yeni, yüksüz, nesil başına
  bir kez; dört uygulayıcı). Pane ana kuyrukta `Session::remote_typed`'ı
  probun bulduğu sarılmış ssh'ın nesliyle eşleyip nonce'u kendi thread'inde
  işaretliyor (`check_remote_typed`). Planın "`key_gen` ilerleyince"si ayrı
  bir sayaç istemedi: `login` kaydından sonraki her `send_input` tanım gereği
  girişten sonra (ikisi de ana thread). **Bilinen sınır**: giriş yoklaması
  (047) girişi çıktı kenarında görüyor; kullanıcı bütün işini giriş
  görülmeden yazıp çıkarsa (pratikte yok: yoklama ilk çıktıda koşuyor) kayıt
  düşmez ve bugünkü tek seferlik düz yeniden bağlanma kalır — yanlışın yönü
  bugünkü davranış.
- **Yan etki ölçüldü (eskalasyon yok)**: üç değişken set iken `locale`,
  `perl -e1`, `python3 -c 1` yerelde (macOS) ve Debian imajında hiçbir şey
  basmıyor. Tek gözlem: yerel ayar **başka bir sebeple** bozuksa (kurulu
  olmayan `LANG=tr_TR.UTF-8`) perl'ün zaten basılan uyarısı listede
  `LC_TERMINAL = "bateri"`'yi de sayıyor — iTerm2'nin `LC_TERMINAL`'ıyla aynı,
  uyarıyı doğuran o değil.
- **Uçtan uca** (`e2e_first_connection_and_the_silent_fallback`, genişletildi):
  set için iki geçici container — `bt049p3-sshd` (`127.0.0.1:2249`, `AcceptEnv
  LANG LC_*`, phase-2'nin `kapi`/`router` kullanıcıları) ve `bt049p3-sshd-lang`
  (`127.0.0.1:2251`, `AcceptEnv LANG`); ikisi de `bateri-sshd-omz` imajından
  iş bitince silindi. Sınama thread'i pane'in uzak probunu da oynuyor
  (`jobs::remote` → `set_remote`, `jobs::remote_login`, `remote_typed` →
  `mark_used`), oturumun `tab_id`'si dolu. Sonuçlar:
  - sarılmış, `AcceptEnv LC_*`: `lc=bateri|0.2.0|bateri://tab/0F1E…` (`-F
    /dev/null`, yani `SendEnv` yok — önyüklemenin dışa aktarması);
  - düz (`TMUX=x`), `SendEnv LC_*` + `AcceptEnv LC_*`: üçü de var;
  - sarılmış, `AcceptEnv LANG`: üçü de var; düz, `AcceptEnv LANG`: üçü de
    yok (bilinen sınır);
  - (c) `ForceCommand` CLI: girişten sonra `show` yazıldı → `used` kaydı;
    `exit` → **yeniden bağlantı yok, `plain` yok** (phase-2'de `true/true`'ydu);
  - (a), (a'), (b) 554 ms'de ikinci parolasız düz yeniden koşu, (255), (d)
    phase-2'deki gibi yeşil — (b)'de kullanıcı giriş sonrası yazmadığı için
    düşme sürüyor.
- **mosh** (container'da ölçüldü; yerelde mosh yok): mosh'un sarmalayıcısı
  `-l` ile yalnız standart yerel adlarını (`LANG`, `LC_CTYPE` … `LC_ALL`)
  geçiriyor, `LC_TERMINAL` o listede yok; ama `mosh-server`'ın açtığı kabuk
  sunucunun ortamını miras alıyor (`LC_TERMINAL=bateri
  LC_BATERI_TAB_URL=… mosh-server new …` → kabukta ikisi de görüldü). Yani
  mosh, kendisini başlatan ssh `AcceptEnv LC_*` ile taşıdığında taşıyor;
  mosh hiç sarılmadığı için kısıtlı sunucuda yok.

- **Set kapısı `/code-review` bulgusu — girişten sonraki 255 (orkestratör
  kararı A, R3.3 daraldı)**: exec isteğini reddeden ya da exit-status'süz
  kanal kapatan uç sarılmış çağrıyı 255 ile bitiriyordu; R3.3 gereği düşme
  yoktu ve her bağlantı yeniden sarılıp kırılıyordu. Şimdi 255 yalnız
  **girişten önce** sessiz: `says_nothing(rc, logged_in)` (sinyal kodu
  129–254 her zaman sessiz), `fell_back` `logged_in` alıyor, zsh fonksiyonu
  255'te de soruyor (`(( rc == 255 )) && return` kalktı; ayrımı ikili
  yapıyor). Giriş kanıtı iki kaynaklı:
  - pane'in `{nonce}.login`'i (`ssh_wrap::mark_login`, `mark_used`'ın ikizi;
    047'nin giriş yoklaması girişi gördüğünde, `TerminalPane::note_login`);
  - **sarılmış çağrının kendi master soketi hâlâ dinliyor**
    (`ssh_wrap::master_listening`: argv'deki `ControlPath=`, soket mi diye
    `lstat`). ssh soketi yalnız kimlik doğrulamadan sonra açıyor ve
    `ControlPersist=2` ile oturumdan sonra yaşatıyor. Bu ikinci kaynak planda
    yoktu ve **ölçümle** geldi: uçtan uca (e)'de reddeden uç oturumu
    milisaniyede kapattı, pane'in giriş yoklaması (sınamada 10 ms'lik yoklama)
    girişi hiç görmedi — `mark_login` devre dışıyken de (e) yeşil, yani düşmeyi
    taşıyan soket kanıtı. `{nonce}.login` kullanıcının kendi bağlantı
    paylaşımı olduğu (bizim `Control`'ümüz yok) ve oturumun yoklamaya yetecek
    kadar sürdüğü kolu kapatıyor; bizim master'ımız yokken 255 o işaret için
    `FELL_BACK_PATIENCE` kadar bekliyor (bedeli: o kullanıcıda gerçek ssh
    hatasından sonra 500 ms). Bilinen sınır: kendi `ControlMaster`'ı olan
    kullanıcıda yoklamanın hiç görmediği anında kapanan reddeden uç hâlâ
    sessiz (yön bugünkü).
  - **Düzeltmenin `/code-review`'u** (medium, yalnız bu düzeltme) iki bulgu:
    (1) soket yolu host başına, deneme başına değil — aynı sunucuya başka
    pane'in canlı master'ı, bu deneme parolada 255 alınca "giriş" gibi
    okunur → giderilmedi, **bilinen sınır** (`master_listening`'in doc'u):
    iki pane'in aynı anda aynı host'un parola sorusunda olması gerekiyor,
    bedel bir kez yeniden parola soran düz koşu ve yalnız `posix` olmayan
    host'ta `plain` satırı (sarmanın kazandırmadığı host); deneme başına soket
    adı phase-5'in host başına paylaşımını bozardı. Yan not giderildi: soket
    dosyası değil bağlantıyı kabul eden soket (`UnixStream::connect`,
    `remove_instance`'ın sınaması). (2) master'sız oturumda geç gelen
    `{nonce}.login` hiç beklenmiyordu → giderildi (yukarıdaki bekleme; sınama
    100 ms sonra gelen işaretle düz koşuyu görüyor).
  - **Taklit uçtan uca** (saf değil): `bt049p3-sshd`'ye `refuse` kullanıcısı —
    giriş kabuğu `-c`'ye `exec request failed on channel 0` basıp 255 ile
    çıkıyor, kabuk isteğine `sw>` prompt'u veriyor; istemci açısından exec
    reddinden ayırt edilemez. Sonuç (e): 255 → **547 ms** sonra düz yeniden
    koşu, ikinci parola yok (master'a bindi), `plain` kaydı. Girişten önceki
    255 kolları yeşil kaldı: (255) yanlış port, (d) parolada Ctrl-C (130);
    saf ve alt komut sınamaları: `--rc 255` kanıtsız → hiçbir şey ve `ssh -G`
    bile yok; başka denemenin `login`'i → hiçbir şey; `login` + `up` → hiçbir
    şey; `login` + 130 → hiçbir şey; `login` → düz koşu; dinleyen soket →
    düz koşu, soket yokken hiçbir şey.

## Checklist

- [x] Kimlik ortamına üç `LC_` değişkeni; miras ezilir
- [x] Sarılmış oturumda uzak önyükleme üç değişkeni dışa aktarır; `unwrap` düşürür
- [x] Test: `LC_TERMINAL=iTerm2` mirasında değer `bateri`
- [x] Test: Docker sshd — düz, sarılmış, `AcceptEnv` kısıtlı
- [x] Yan etki kontrolü (`locale`, perl, python)
- [x] R7: girişten sonra yazılmış oturum düşmüyor (ForceCommand CLI sınaması)
- [x] CLAUDE.md kimlik cümlesi
- [x] Doğrulama geçti (`make check`, `make linux`, `make bundle`, `make smoke`)
- [x] Set kapısı: `/code-review` (setin aralığı) + `/audit` — `/audit` temiz;
  `/code-review`'un tek orta bulgusu (girişten sonraki 255) orkestratör
  kararıyla (A) giderildi, düzeltmeye `/code-review` yeniden koştu (Uygulama
  Notları)
