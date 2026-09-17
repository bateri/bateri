# Phase 5 — Prompt'un devri

## Özet

`PS1` ve `RPS1` sıfır görünür genişliğe insin, `>` dock'ta prompt'un yerini
alsın, blok çıpası `preexec`'e taşınsın ve kullanıcıya geri dönüş anahtarı
verilsin.

_Requirements: R4.1, R4.2, R4.3_

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — sıfır görünür genişlik.
  - **`RPS1`/`RPROMPT` de boşaltılır.** Bugün dosyada hiç geçmiyor; PS1 tek
    başına yetmez (011 Karar 12'nin zorunlu ayrıntısı).
  - **Dayatma aynanın ZLE kancasından yapılır**, `precmd`'den değil:
    p10k/starship `PS1`'i `precmd`'den **sonra** kendi ZLE kancalarından
    yeniden kuruyor ve aynı yerden dayatılmazsa tema kazanır.
  - **Çıpa `preexec`'e taşınır** (`anchor_close` → `preexec`). Alacritty
    kaynağında doğrulandı (011 Karar 10a): sıfır genişlikli PS1'de çıpa hayatta
    kalıyor — `zle reset-prompt`, Ctrl-L, geçmişte gezinme ve `TRANSIENT_PROMPT`
    kırmıyor. `Cell::set_underline_color(None)` `extra`'yı ancak hyperlink de
    yokken düşürüyor; yeniden çizilen hücre PS1'in şablonundan çıpayı geri
    alıyor.
- **`crates/bt-core/src/settings.rs`** — `prompt` anahtarı:
  `"terminal"` (varsayılan) ya da `"shell"`. Tanınmayan değer **yalnız bu
  anahtarı** etkiler ve uyarı görünür (`Settings::parse_keeping` örüntüsü);
  bilinmeyen anahtar **silinmez**.
  - `"shell"` prompt'u kullanıcıya geri verir **ama dock'u kapatmaz**: dock
    kabuğun prompt'unu değil ZLE'nin tamponunu çiziyor. İkisinin ayrı olması
    kasıtlı — kullanıcı prompt'unu geri almak için dock'tan vazgeçmek zorunda
    kalmamalı.
  - Kabuk çoktan doğduğu için **sonraki oturumda** geçerli (`shell.integration`
    ile aynı sınıf) — `docs/AYARLAR.md` bunu söyler.
- **`crates/bt-core/src/session.rs`** — `>`'in rengi safhadan çözülür ve dock
  kaydında sınırı geçer (koşuyor / başarılı / hatalı).

**Bu phase'ten önce dock çizilmiş olmak zorunda** (phase-3, phase-4). Ters sıra
promptsuz bir terminal bırakır ve `make hepsi`, `make duman`, `make kur`
**üçü de yeşil** kalır — prompt yolunu hiçbir kapı görmüyor.

**R4.1 ile R4.2 aynı commit'te iner** (phase-4'ün devir notu). phase-4'ün
bastırma aralığı **çıpayı taşıyan hücreden** türüyor; sıfır genişlikli `PS1`
hiçbir hücre yazmadığı için `anchor_close` `PS1`'in sonunda kalırsa kullanıcının
yazdığı hücreler çıpasız kalır, bastırma **ve** blok şeridi sessizce ölür. R4.2
(`anchor_close` → `preexec`) tam da bunu kapatıyor; ayrı inerlerse aralarında
kör bir hâl var ve üç kapı yine yeşil.

**Aralığın alt ucu çıpadan bulunamaz, aritmetikten bulunur.** R4.2 bağlantıyı
`Input` boyunca açık bıraktığı için ZLE'nin yazdığı **her şey** çıpayı taşır —
tamamlama listesi dahil. "Son çıpa satırı" ölçütü bu yüzden listeyi de yutar;
phase-4'ün `SuppressedInput::chars_after_cursor` hesabı yerinde kalır ve alt
ucun tek kaynağı odur.

## Kabul

- Kullanıcının prompt'u ızgarada görünmüyor; yerine dock'ta `>` var.
- `>` safhaya göre renkleniyor; çıkış kodu rengi 010'un yolundan geliyor.
- **Komut blokları yaşıyor:** çıpa `preexec`'ten geliyor ve şerit komutun
  satırında duruyor.
- p10k ya da starship kurulu bir oturumda tema prompt'u **geri yazamıyor**
  (dayatma ZLE kancasından).
- `prompt = "shell"` yazıp kaydeden kullanıcı sonraki oturumda kendi
  prompt'unu geri alıyor, dock **duruyor**.
- Tanınmayan `prompt` değeri yalnız bu anahtarı etkiliyor, uyarı görünüyor.

## Yayın Etkisi

- **Ayar şeması büyüyor:** `prompt` anahtarı, varsayılanı `"terminal"`,
  tanınmayan değer davranışı ve **sonraki oturumda geçerli** notu
  `docs/AYARLAR.md`'ye girer. Bilinmeyen anahtar asla silinmez.
- **Varsayılan yıkıcı:** kurulu kullanıcı prompt'unu kaybediyor. `AYARLAR.md`'nin
  kurtarma bölümüne satır eklenir; `shell.integration = "off"` da hâlâ bir
  çıkış ama **blokları da öldürüyor**, yani orantısız olan yol adıyla yazılır.
- **`make kur` zorunlu** (`assets/shell/*` değişti).
- **Shell entegrasyonu:** yalnız zsh; bash/fish'te prompt devri **yok**,
  `CLAUDE.md`'nin ilgili maddesi bunu söyler.
- **Bilinen sınır:** p10k **instant prompt** kancalarımızdan önce koşup ilk
  kareyi kendi prompt'uyla çiziyor.
- **Bilinen sınır (bu phase'de ölçüldü):** `preexec` koşmayan yollarda
  (Ctrl-C, boş satıra Enter) çıpa bir sonraki prompt'un `PS1` genişlemesine
  kadar açık kalıyor; o pencerede kullanıcının kendi `precmd`'sinin bastığı
  hücreler önceki bloğun kimliğini taşır. Yön güvenli (fazladan şerit işareti;
  bastırma etkilenmez) ve betikte adıyla duruyor.
- **`CLAUDE.md` güncellendi** (dosyanın kendi kuralı): "Prompt hâlâ kabuğun"
  cümlesi devrin anlatımıyla değişti, ayar envanterine `shell.prompt` girdi ve
  "kayıt anında uygulanır"ın istisnası tek anahtardan `[shell]` bölümüne
  genişledi.
- shader, terminfo, tema biçimi: yok. Yeni bağımlılık: yok.

## Uygulama Notları

- **"Dayatma `precmd`'den değil" çürüdü — ölçüm.** Phase, sıfır genişliğin
  **yalnız** aynanın ZLE kancasından dayatılmasını istiyordu. Üç `zsh -i` PTY
  probe'u bunu çürüttü: **ZLE kancasından atanan `PS1` tek başına hiçbir şey
  yapmıyor.** Prompt `line-init` koşmadan **önce** basılıyor ve zsh
  genişlettiği hâli tutuyor; kancadan atanan `OURSINIT>`/`OURSREDRAW>` ekranda
  hiç görünmedi. Etkili olmasının tek yolu `zle reset-prompt`.
  - Çare **iki yerden dayatmak**: `precmd` ilk basımı doğru yapıyor,
    `__bateri_prompt_guard` temanın geri yazdığını `reset-prompt` ile geri
    alıyor. Sahte bir p10k (`PS1`'i `precmd`'de kurup `line-init`ten
    `reset-prompt`'layan) ile ölçüldü: `PS1` de `RPS1` de ekrana hiç ulaşmıyor.
  - **`precmd`'yi atmanın iki bedeli de ölçüldü** ve ikisi de onu hak
    ettiriyor: (1) yalnız `precmd`'den kuran bir temada (starship) nöbet hiç
    sıfırlamıyor — `precmd` olmasaydı **her** prompt bir `reset-prompt`
    yeniden çizimi öderdi; (2) `zle -N zle-line-init` diyen bir eklenti
    dağıtıcıyı ezerse (betiğin en muhtemel saydığı sınır) kanca büsbütün
    susar ve prompt geri gelirdi.
- **Nöbet ping-pong'u kesiyor, ölçüldü.** `reset-prompt` yeni bir çizim
  doğuruyor, o çizim de `line-pre-redraw`'ı yeniden çağırıyor; koşulsuz bir
  sıfırlama kendi kendini besleyen bir döngü olurdu. Nöbetli hâlde prompt
  başına **tek** sıfırlama sayıldı.
- **Ctrl-C'de ne `preexec` ne `line-finish` koşuyor** (probe'da doğrulandı).
  Çıpa o yolda bir sonraki prompt'un `PS1` genişlemesine kadar açık kalıyor.
  Daha erken kapatmanın yolu yok: kancamız `add-zsh-hook` ile **sona**
  eklendiği için kullanıcının kendi `precmd`'lerinden sonra koşuyoruz. Yön
  güvenli — o pencerede basılan hücreler önceki bloğun kimliğini taşır (bir
  fazla şerit işareti), bastırma etkilenmez. **Bilinen sınır** olarak betikte
  adıyla duruyor.
- **`>`'in rengi zaten vardı.** Phase'in değişiklik listesi `session.rs`'te
  "`>`'in rengi safhadan çözülür" diyordu; `dock::sigil_color` bunu phase-3'te
  yapmış ve `Session::dock` safhayı zaten geçiriyor. `session.rs` bu phase'de
  **hiç değişmedi**.
- **Sınamanın ilk hâli boş yere yeşildi — ve ayrıca kırılgandı.** "Boştaki
  prompt ızgarada yok" iddiası yük taşımıyor, çünkü **phase-4'ün bastırması
  kullanıcının prompt'unu zaten gizliyor** (aralık çıpa satırından imlecin
  satırına, prompt o aralıkta). Devri tümden kaldıran bir regresyonda bile
  yeşil kalıyordu; üstelik ayna `Live` olmadan alınan bir kare prompt'u görür
  ve iddia **rastgele** kırmızıya düşerdi. İddia komut koştuktan **sonraya**
  taşındı: koşmuş bir komutun satırı bastırmanın dışında, yani cevap kesin
  (`true` mü, `ZSHPROMPTXY> true` mü).
- **Tek dayatma noktasını kaldırmak sınamayı kırmıyor ve bu bir kusur değil.**
  Yük sınamasında `precmd` kolunu tek başına kaldırmak testi kırmadı: nöbet
  hâlâ tutuyor. Yükün doğrulanması için devir **tümden** kapatıldı
  (`__bateri_prompt=shell`) ve iddia o zaman düştü. Yani yedeklilik gerçek,
  sınama da yük taşıyor — ikisi ayrı ayrı gösterildi.
- **Yük sınamasının üç kolu da koşturuldu:** çıpayı `PS1`'in sonuna geri
  almak `the_terminal_takes_the_prompt_and_the_block_survives_it`'i tam da
  kendi gerekçesiyle düşürüyor ("blok doğmadı — `anchor_close` `preexec`'te
  mi?"); `BATERI_PROMPT`'u yoksaymak
  `the_shell_keeps_the_prompt_when_the_user_asks_for_it`'i düşürüyor.
- **phase-4'ün bastırması yeni çıpa biçimi altında sınanmamıştı — açık
  kapatıldı.** R4.2 teli değiştirdi (çıpa `Input` boyunca açık, ZLE'nin
  yazdığı **her** hücre kimlik taşıyor) ama phase-4'ün bütün birim bekçileri
  çıpayı prompt'un sonunda kapanan **eski** biçimle kuruyor
  (`anchored_prompt`), yani yeni biçime özgü bir regresyonu hiçbiri göremezdi:
  bastırma yazarken ölse ızgara ile dock aynı satırı birden gösterirdi — tam
  da phase-4'ün kapattığı çift görüntü — ve üç kapı yeşil kalırdı. Sınamaya
  **Enter'dan önceki** bir adım eklendi: `true` yazılıyor, ayna `Live` olup
  tamponu eşleşene kadar bekleniyor, ızgarada `true` **olmamalı**. Yük
  taşıdığı doğrulandı: çıpa `PS1`'e geri alınınca "yazılmakta olan satır
  ızgarada da çizildi (çift görüntü)" ile düşüyor.
  - `last_ink_in_row` **hyperlink okumuyor** (yalnız karakter, `HIDDEN` ve
    spacer bayrakları), yani tazelik kapısı çıpanın yayılmasından
    etkilenmiyor. Şüphe buradan doğmuştu; kod okunarak ve sınamayla ayrı ayrı
    kapatıldı.
- **Çıpa dizgileri `precmd`'nin yerelinden `__bateri_hooks`'un genel
  değişkenine taşındı:** `__bateri_ps1` şablonu iki koldan da okunuyor ve
  nöbet onu **eşitlikle** soruyor, içermeyle değil (terminal kolunda `PS1`
  bütünüyle bizim).
- **`Prompt::name` `pub`**, komşularından farklı: aynı dizgi hem ayar
  dosyasındaki yazılış hem `BATERI_PROMPT`'un değeri. Sınama da teli o
  fonksiyondan kuruyor, yani iki uç birlikte değişiyor.
- **`copy_wrapper` yardımcısı çıkarıldı:** üç gerçek-zsh sınaması aynı
  kopyalamayı yapıyordu ve gerekçesi (depo dizinini `ZDOTDIR` vermemek) tek
  yerde durmalı.

## Checklist

- [x] `PS1` **ve** `RPS1`/`RPROMPT` sıfır görünür genişlikte
- [x] Dayatma ZLE kancasından (p10k/starship geri yazamıyor) — **ayrıca
      `precmd`'den**; "yalnız kancadan" ölçümle çürüdü (Uygulama Notları)
- [x] Çıpa `preexec`'e taşındı; bloklar ve şerit yaşıyor
- [x] `prompt` anahtarı: varsayılan, tanınmayan değer, sonraki oturum notu
- [x] `prompt = "shell"` dock'u **kapatmıyor** (`shell_integration_env` dock
      payını yalnız entegrasyondan türetiyor; `prompt` o karara girmiyor)
- [x] Test: çıpanın `preexec`'ten gelmesi; blok kimliği akışta
      (`the_terminal_takes_the_prompt_and_the_block_survives_it`, gerçek zsh;
      yük taşıdığı regresyonla doğrulandı). Aynı sınama phase-4'ün
      **bastırmasını da** yeni çıpa biçimi altında doğruluyor — yazma anında
      ızgara satırı göstermiyor
- [x] Test: `prompt` anahtarının ayrıştırılması ve round-trip (bilinmeyen
      anahtar korunuyor) — `prompt_is_read`,
      `unrecognized_prompt_keeps_its_own_key`,
      `unrecognized_prompt_leaves_integration_alone`, `prompt_is_not_a_live_change`
- [ ] Gerçek zsh oturumunda gözle: p10k/starship kurulu, prompt devredilmiş —
      **kullanıcıda**, ajanın kabuğunda gerçek pencere yok
- [x] `docs/AYARLAR.md` (anahtar + kurtarma satırı)
- [x] Doğrulama geçti (`make hepsi` + `make kur`; `make test-yaris`
      tetiklenmedi — paylaşılan duruma dokunulmadı, `session.rs` değişmedi)
- [x] Yayın etkisi yazıldı
