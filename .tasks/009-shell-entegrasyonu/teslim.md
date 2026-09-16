# Shell entegrasyonu ve komut durumu (zsh) — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md)
> · [phase-3.md](phase-3.md) · [phase-4.md](phase-4.md) · [phase-5.md](phase-5.md)

Kabuk artık ne yaptığını terminale söylüyor: zsh oturumunda `ZDOTDIR` bizim
sarmalayıcımızı gösteriyor, betik kullanıcının beş başlangıç dosyasını da
devrediyor ve OSC 133 işaretlerini basıyor; `bt-core` PTY okuma yolunu tarayıp
durumu `Session::shell_state()`'te tutuyor. Ürün yüzeyi **yok** — blok da dock
da sonraki setlerin işi. Kullanıcıya görünen iki şey var: yeni ayar anahtarı
`[shell] integration` (varsayılan `"auto"`, **sonraki oturumda** geçerli) ve
`.app` paketine giren yeni bir kaynak türü (`Contents/Resources/shell/`).
Kullanıcının rc dosyalarına hiçbir şey yazılmıyor ve entegrasyonu kapatmak iz
bırakmıyor.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi        # sürüm + fmt + denetim + clippy + test
make test-yaris   # PTY okuyucu ile paylaşılan durum değişti (TappedPty, Session.shell)
make kur          # assets/shell/* ve assets/bundle/* pakete giriyor
make duman        # pencereyi açan davranış değişti
```

`make duman`'ı **kullanıcı koşturur**: ön plana hiç gelmeyen bir pencerede
display link callback vermiyor ve kapı `Verdict::MotionUnsettled` ile yanlış
tanı veriyor (kayıt: `docs/YOL-HARITASI.md` → Sete bağlanmamış borçlar).

### Beklenen çıktı

- `make hepsi`: `denetim: temiz`, 354 sınama yeşil.
- `make test-yaris`: iki zamanlama profili de yeşil; `race_*` ailesi **beş**
  (009 `race_shell_state_and_frame`'i ekledi). TSan koşmadı — nightly ister,
  araç zinciri pin'li değil; bilinen sınır, waive değil.
- `make kur`: `kur: …/bateri.app (sürüm 0.1.0, taban macOS 14.0)` ve pakette
  `Contents/Resources/shell/zsh/` altında beş dosya.
- `make duman`: `kare=29 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke
  istek=4 icerik=2 hareket=27 sessiz=1754.25ms kapanis=clean`. Sabitler
  phase-3'tekiyle birebir: entegrasyon süreli koşuda **hiç** kurulmuyor.

Ölçüm sayısı değişmedi; `docs/OLCUMLER.md`'ye bu sette yeni satır girmiyor.

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make test-yaris` yeşil (iki profil; TSan bilinen sınır)
- [x] `make kur` yeşil ve betik pakette
- [x] `make duman` yeşil (kullanıcı, gerçek pencere)
- [x] Gerçek pencerede gözle: `$PATH` bozulmamış, `${ZDOTDIR:-unset}` → `unset`,
      `__bateri_precmd` kancalarda ve en sonda
- [x] `/code-review` (set aralığı + phase-5 diff'i ayrıca) ve `/audit` koştu;
      bulgular phase-5 ile `009 kapı` commit'inde kapandı

## B. Yayın (doğrulamadan SONRA)

### B.1 Push `[oto]`

`/ship`: `main`'e gider. Branch, migration ya da paket yayını yok.

### B.2 Tarayıcının akış maliyeti `[komut]`

phase-2 bir **ölçüm bekleyen iddia** bıraktı: baytlar artık iki kez geziliyor
(tarayıcı + ayrıştırıcı) ve maliyeti ölçülmedi. Kanca hazır:

```sh
BT_SCROLL_TEST=1 BT_FRAME_STATS=1 BT_RUN_SECONDS=10 cargo run -q -p bateri
```

Sayı `/measure` ile `docs/OLCUMLER.md`'ye girer. Aynı ölçüm phase-2'nin ikinci
açık kalemini de cevaplıyor: tarayıcının `position`'ı skaler, `vte`'nin
`memchr`'ı SIMD — `memchr`'ı doğrudan bağımlılık yapmak **ikinci** bir
bağımlılık kapısı ve ölçüm olmadan açılmıyor.

Bu bir kapı değil: ölçüm kullanıcı istediğinde koşar, sevki bloklamaz.

### B.3 Paketi kur `[komut]`

Betik `.app`'e `make kur` ile giriyor; geliştirmede (`cargo run`) depo
yolundan bulunuyor. Sevk edilen bir paket isteniyorsa:

```sh
make kur
```

### Yayın Checklist

- [ ] B.1 `/ship` — `main`'e push
- [ ] B.2 `/measure` — tarayıcının akış maliyeti (ölçüm bekliyor; kapı değil)
- [ ] B.3 `make kur` — sevk paketi gerekiyorsa

## Geri Alma

- **Tamamı:** setin **kod** commit'leri altı tane (`dfb71cc^..39158d0`:
  `dfb71cc`, `1b0ce60`, `a5a5eb9`, `e94b5f9`, `4ace8a8`, `39158d0`) ve ters
  sırayla revert edilir. Kalan üçü yalnız defter (`b14a9bd`, `c01e093`,
  `970c178`); revert'e girmeleri gerekmez. `bt-core` ve `bt-shell` bugünkü
  hâllerine döner,
  kullanıcının makinesinde geri alınacak **hiçbir dosya yoktur** — entegrasyon
  yalnız çocuğun ortamına iki değişken koyuyordu.
- **Yalnız entegrasyonu kapatmak (kod geri almadan):** kullanıcı
  `~/.config/bateri/settings.toml`'a `[shell] integration = "off"` yazar;
  **sonraki oturumda** geçerli olur. Uygulama hiç açılmıyorsa dosyasız tarif
  `docs/AYARLAR.md` → `[shell]`'de.
- **Ayar şeması:** anahtar yalnız **eklendi**; silinen ya da adı değişen
  anahtar yok, bilinmeyen anahtar korunuyor. Revert sonrası eski
  `settings.toml`'lar aynen okunur — `[shell]` bölümü bilinmeyen bölüm olarak
  yerinde kalır, dosya bozulmaz.
- **Paket:** `Contents/Resources/shell/` kalkar; `make kur`'un denetimi de
  aynı commit'te geri gittiği için kapı kırmızı düşmez.
- **`TERM` değişmedi** (`xterm-256color`), terminfo'ya dokunulmadı: SSH'ın öte
  tarafında geri alınacak bir şey yok.
