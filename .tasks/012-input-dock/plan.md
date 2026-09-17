# Input Dock ve prompt'un devri

## Hedef

Giriş satırı terminalin olsun: prompt'u kabuk değil bateri çizsin ve yazdığın
şey ızgarada değil, pencerenin altındaki **kendi yüzeyinde** — iki satırlık bir
dock'ta — dursun. Üst satır giriş (`>` + metin + imleç + öneri + renklendirme),
alt satır bağlam (`[tam klasör yolu] | [git dalı]`).

Dock **yalnız entegrasyonlu zsh oturumunda** var; alternatif ekrana geçen bir
uygulama (vim, htop) pencereyi **tamamen** geri alır.

## Gereksinimler

- **R1** — Terminal ZLE'nin görüntü durumunu biliyor.
  - **R1.1** — Kabuk `PREDISPLAY`, `BUFFER`, `POSTDISPLAY`, `region_highlight`
    ve `CURSOR`'ı `add-zle-hook-widget line-pre-redraw` ile bildirir;
    `line-finish` durumu kapatır. `zle -N` **kullanılmaz**
    (zsh-syntax-highlighting ve autosuggestions aynı widget'ı istiyor).
    Kurulum idempotan nöbetli; kullanıcının rc dosyasına **yazılmaz**.
  - **R1.2** — Taşıma yeni bir OSC numarasıyla, base64 ile. Tarayıcıya ikinci
    kol açılır; payload sınırı `133`'ünkinden ayrı ve aşımı **görünür**
    (bugünkü `Skip` kolu çağırana sinyal vermiyor).
  - **R1.3** — Durum `bt-core`'da yaşar ve `frame()` sınırından **çözülmüş**
    geçer (metin + imleç sütunu + renk aralıkları). Kayıt yeniden kullanılan
    bir tampondur; ölçüt kare başına maliyet.
- **R2** — Dock çizilir.
  - **R2.1** — Dock **ikinci bir `setViewport`** ile ayrı koordinat uzayıdır;
    ötelemeden yapısal olarak muaf. Kendi listeleri var, yani
    `Frame::move_cursor`'ın `truncate(bg_count)`'u onları görmez.
  - **R2.2** — Dock içeriği sink'ten **sonra** basılır (`Frame::clear`
    `origin_px`'i sıfırladığı için sink içinde öteleme emsali kullanılamaz).
  - **R2.3** — Üst satır: `>` işareti (safha rengiyle), metin, caret,
    `POSTDISPLAY` (sönük), `region_highlight` aralıkları.
  - **R2.4** — Alt satır: `[tam klasör yolu] | [git dalı]`, sol altta yan yana.
    Taşmada yol **soldan** kısaltılır, dal asla kısalmaz.
  - **R2.5** — Dock'un kendi caret'i `CursorBlock`'un ikinci bir bağlamasıdır
    (aynı uniform slot'u, ayrı encode çağrısı); shader değişmez.
- **R3** — Giriş satırı ızgarada **çizilmez**.
  - **R3.1** — Safha `Input` iken `prompt_row..=cursor_row` aralığının hücreleri
    glyph listesine girmez. Safha kopyası `Term` kilidinden **önce** alınır
    (`Theme` ile aynı örüntü).
  - **R3.2** — Çıpa taraması bastırmadan **etkilenmez**: `cell.hyperlink()`
    okuması glyph üretiminden bağımsız, yani blok şeridi yerinde kalır.
  - **R3.3** — ZLE'nin özel kipleri (`bck-i-search`, `menu-select`, `zle -M`,
    `CORRECT`'in `[nyae]`'i) beş değişkenin dışında çiziyor; o anlarda bastırma
    **bırakılır** ve ızgara devralır.
- **R4** — Prompt terminalin.
  - **R4.1** — `PS1` **ve** `RPS1`/`RPROMPT` sıfır görünür genişliğe iner;
    dayatma aynanın ZLE kancasından yapılır, çünkü p10k/starship `PS1`'i
    `precmd`'den **sonra** kendi kancalarından kuruyor.
  - **R4.2** — Blok çıpası `preexec`'e taşınır (`anchor_close` → `preexec`;
    alacritty kaynağında doğrulandı, 011 Karar 10a).
  - **R4.3** — `prompt` ayar anahtarı: `"terminal"` (varsayılan) ya da
    `"shell"`. Kullanıcı prompt'unu entegrasyonu kapatmadan geri alabilir.
- **R5** — Dock alternatif ekranda kalkar.
  - **R5.1** — Ayırma oturum doğarken karara bağlanır (entegrasyon kuruldu mu);
    koşu boyunca oynamaz, yani `/bin/sh` koşan duman reçetesi dock almaz ve
    `smoke_shell` ile ona bağlı üç sınama **dokunulmaz**.
  - **R5.2** — Alternatif ekrana giriş/çıkışta dock kalkar/iner ve ızgara
    yüksekliği değişir. Resize render yolundan **çağrılmaz**: `dispatch2` ana
    kuyruğundan bir sonraki turda koşar (emsal `child_exit`, OSC 52).
  - **R5.3** — Bedel **komut başına değil**, alternatif ekran geçişi başına.
- **R6** — Kapı ve ölçüm dürüstlüğü.
  - **R6.1** — Prompt yolunu bugün hiçbir kapı görmüyor. Bu **yazılı** kabul
    edilir ve dock'a tanık olacak bir yol (üçüncü yük ya da elle koşu) plan
    aşamasında adıyla seçilir.
  - **R6.2** — Tuş başına O(n) bayt **ölçüm bekliyor + araç da borç**;
    `/measure` bugün kapatamaz (`BT_INPUT_LATENCY_SAMPLES` yok).

## Yaklaşım

Sıra **tek yönlü ve zorunlu**: dock çizilmeden prompt sıfır genişliğe
indirilmez. Ters sıra promptsuz bir terminal bırakır ve üç kapı da yeşil kalır.

1. **Kanal.** Yeni OSC numarası, base64, tarayıcıya ikinci kol, kendi sınırı ve
   sınır aşımının görünür sonucu. Okuma yolu → **riskli phase**
   (`make test-yaris` + kendi `/code-review`'ı).
2. **ZLE tesisatı.** `line-pre-redraw` + `line-finish`, beş değişken, idempotan
   nöbet. Kullanıcının rc dosyasına yazılmaz.
3. **Dock yüzeyi.** İkinci `setViewport`, ayrı listeler, üst satır. Prompt hâlâ
   kabuğun — yani bu adımda **çift görüntü** var ve bu bilinçli ara durum
   (gürültülü ama zararsız).
4. **Bastırma.** `Input` safhasında giriş satırı ızgaradan çıkar; çift görüntü
   kapanır. Özel kip tetiği burada.
5. **Prompt'un devri.** Sıfır genişlik `PS1`/`RPS1`, `>` dock'ta, çıpa
   `preexec`'e, `prompt` anahtarı.
6. **Bağlam satırı.** OSC 7 bağlanır (bugün sessizce düşüyor), dal `precmd`'den
   gelir, alt satır çizilir.
7. **Alternatif ekran.** Dock kalkar/iner; resize ana kuyruktan.

## Kapsam Dışı

- **Tuş vuruşu ve silme animasyonları** (`keypress`, `delete_mode`) — aynanın
  üstüne kurulur, sonraki set. Aynanın gerekçesi onlara bağlı **değil**: dock'un
  kendisi aynayı hak ettiriyor.
- **bash ve fish** — dock yalnız entegrasyonlu zsh'te. SSH ve
  `integration = "off"` oturumunda pencere tamamen ızgara (kullanıcı: "şimdilik
  ok, sonra bakarız").
- **Git dalının hızlandırılması** — dal `precmd`'de bir fork; büyük depoda
  hissedilir ve çaresi (daemon ya da önbellek) ayrı bir iş.
- **Tamamlama listesinin dock'a taşınması** — ızgarada kalır (Karar 3a).
- **Prompt'un tema alt-rolleri ve prompt ayar şeması** — `prompt` anahtarı
  dışında yok.

## Göç

- **Betik değişiyor:** açık pencereler eski betikle koşmaya devam eder; yeni
  pencere yenisini alır. `make kur` zorunlu (`proje.md` → Doğrulama).
- **Yeni ayar anahtarı `prompt`:** varsayılanı `"terminal"`, yani kurulu
  kullanıcı **prompt'unu kaybeder**. Bu görünür bir değişiklik; `docs/AYARLAR.md`
  hem anahtarı hem kurtarma yolunu yazar.
- **Bilinen sınırlar adıyla:** p10k **instant prompt** kancalarımızdan önce
  koşup ilk kareyi kendi prompt'uyla çiziyor; `psvar[9]` düşerse artık bedeli
  şerit değil **kimliğin kaybı**; `zle -I` iş bildirimi çıpa satırını bir satır
  kaydırabiliyor.

## Akış

```
zsh + ZLE                                    her tuş vuruşunda
  line-pre-redraw ──► PREDISPLAY BUFFER POSTDISPLAY region_highlight CURSOR
                          │ base64, yeni OSC
                          ▼
bt-core  Scanner ──┬─► 133 kolu  ──► ShellLog (safha, bloklar)
                   └─► dock kolu ──► DockState (yeniden kullanılan tampon)
                          │
  Session::frame() ───────┼─► Cell listesi: Input safhasında
                          │      prompt_row..=cursor_row ATLANIR        (R3.1)
                          │      çıpa taraması yine de koşar            (R3.2)
                          └─► Dock kaydı: çözülmüş metin + caret + renk (R1.3)
                          ▼
bt-gpu   encode_pass
  ├─ setViewport(originY: origin_px) ──► ızgara: bg + glyph + kural + şerit
  └─ setViewport(kimlik)             ──► DOCK: kendi listeleri, kendi caret'i
                                            (sink'ten SONRA basılır,  R2.2)
bt-shell
  alternatif ekran değişti ──► dispatch2 ana kuyruk ──► resize            (R5.2)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| phase-3 | |
| phase-4 | |
| phase-5 | |
| phase-6 | |
| phase-7 | |
| kapı | |
