# Phase 9 — Dock'un yüzü: nefes payı, tek caret, kendi işareti

## Özet

Dock'u bakılabilir hâle getir: satırların nefes payı olsun, caret **her yerde**
animasyonlu olsun (ızgarada, dock'ta ve ikisi arasında) ve `>` işareti fonttan
alınan bir harf değil bizim çizdiğimiz bir şekil olsun.

_Requirements: R2.3 eki, R2.5 eki_

## Bağlam

Gerçek pencerede üç kusur, üçü de kullanıcıdan:

1. **"dock padding top yok resmen ve çok çirkin duruyor."** İki satır saç
   çizgisine yapışık; dock'un `dock_px`'i tam `DOCK_ROWS * cell_h`, yani hiç
   pay yok.
2. **"sleep 5 bitince caret animasyonla gelmedi geri"** ve **"ızgarada yazarken
   animasyonla sağa sola smooth giderdi cursor, dock kısmında neden bunlar yok.
   kazanımlarımızı niye korumuyoruz."** İkisi **tek** kusurun iki yüzü:
   `bt-gpu::motion` yalnız ızgaranın imlecini biliyor, dock'un caret'i
   (`Dock::caret`) animatöre hiç uğramıyor. `Motion::sync` caret ızgarayı
   bırakınca durumu **düşürüyor** (`visible = false` → `state = None`), yani
   devir bir ışınlanma.
3. **"> kısmında direkt harf mi kullandık? bunu daha hoş kendin çizebilir
   misin?"** Evet, harf: `SIGIL: char = '>'` atlastan `Sprite::Char` olarak
   geliyor, yani kullanıcının fontunun `>`'ü.

## Kararlar

- **Dock bitişik + nefes payı** (kullanıcı seçti): saç çizgisi ızgarayı
  ayırıyor, **altında** ve dock'un dibinde boşluk kalıyor. Ayrık yüzey
  reddedildi.
- **Tek caret, tek animatör.** Çapraz geçiş (fade) reddedildi: kusur yalnız
  devir değil, dock'ta **hiç** hareket olmaması. Fade ikincisini çözmezdi.
  `Motion` dock'u ikinci bir hedef olarak görüyor ve hedef **ekran satırı**
  cinsinden — ızgaranın altındaki dock, `grid_rows + pay` satırından başlıyor.
- **Dock hedefini `bt-core` hesaplamıyor.** O "caret dock'ta, sütun c" diyor
  (`Dock::caret`, bugün de öyle); ekran konumuna çeviren `bt-gpu`. Koordinat
  çevirisi boyamadır, karar değil — "karar burada, boyama orada".
- **Pay ölçülmez, türetilir:** `cell_h`'den. Sabit bir piksel sayısı puntoyla
  ölçeklenmez ve ölçülmemiş bir sayı olurdu.
- **İşaretin çizgi kalınlığı alt çizgi kuralının metriğinden.** İkinci bir
  kalınlık sayısı uydurmak iki kaynak demekti.

## Değişiklikler

- **`crates/bt-gpu/src/frame.rs` + `renderer.rs` + `bt-shell` geometrisi** —
  dock payı `DOCK_ROWS * cell_h + 2 * pad`. Üç yerde birden, yoksa
  yeniden boyutlandırmada bir kare uyuşmazlık doğar: `split_into_grid`,
  `Frame::dock_px`, dock listelerinin `y`'si (`push_dock`, `push_dock_caret`,
  `dock_ground`). Saç çizgisi dock viewport'unun **tepesinde** kalıyor; pay
  onun altında.
- **`crates/bt-gpu/src/motion.rs`** — `Motion::sync` caret'in dock'a gitmesini
  bir **hedef** olarak alıyor, durumu düşürmüyor. `settled()` dock ayağını da
  sayıyor: saymasaydı link kayma ortasında uyur ve caret boşlukta donardı.
  `reduce_motion`/`snap` üçüncü bir kip doğurmuyor — mevcut iki kip aynen.
- **`crates/bt-gpu/src/renderer.rs`** — caret **bir kez** çiziliyor, dock
  geçişinde ve pencere uzayında (`shifted_y` emsali). Dock viewport'u
  kırpmıyor, yani boşlukta da doğru yere düşüyor; ters çevirme dikdörtgeni iki
  glyph encode'una da gidiyor ki caret üstünden geçtiği harfi okunur bıraksın.
- **`crates/bt-atlas`** — yeni bir yordamsal sprite (chevron). `Face::Regular`'a
  çivili (`Sprite::Rule` emsali), kendi yuva payı var. Dikey ortalaması hücrenin
  değil **x-height**'ın merkezi.
- **`crates/bt-core/src/dock.rs`** — işaret artık **hücre değil**: sınırdan
  yalnız rengi geçiyor (`Dock::sigil`), şekli `bt-gpu`'nun kararı. Aynı sprite
  ızgaranın blok işaretini de çiziyor — kullanıcının istediği birleşme.

## Kabul

- Dock'un iki satırı saç çizgisine yapışmıyor; üstünde ve altında boşluk var.
  Punto değişince (Cmd +/−) pay da ölçekleniyor.
- **Dock'ta yazarken caret sağa sola süzülüyor** — ızgaradakiyle aynı stil.
- `sleep 5` bitince caret ızgaradan dock'a **kayarak** iniyor; başlarken
  kayarak çıkıyor.
- Kayma ortasında pencere uyumuyor (caret boşlukta donmuyor).
- `reduce_motion`/`cursor_motion = "snap"` caret'i dock'a da **snap**'liyor;
  belirme kipi belirmeye indiriyor.
- `>` yerine bizim çizdiğimiz chevron; fontu değiştirince şekli değişmiyor.
- Alternatif ekranda ve dock'suz pencerede hiçbir şey değişmiyor.

## Yayın Etkisi

- **`make duman` zorunlu** (yalnız caret commit'inden sonra): `hareket=`
  jetonu kaymanın tanığı ve sıfır olmamalı; `icerik=` sınırı aşılmamalı.
  Dock'suz olduğu için jeton sözleşmesi (`hucre=8 glif=6 kural=15`)
  dokunulmadan kalıyor — süreli koşu dock almıyor.
- **`make kur`**: paket geometrisi değişiyor.
- **`CLAUDE.md`:** dock payının formülü, caret'in tek animatörü, işaretin
  yordamsal olduğu.
- Atlas yuva payı büyüyor (`RULE_RESERVE`); `yuva=U/T` jetonu bunu gösterir.
- shader: **ölçülecek** — caret tek geçişte çizilecekse `cell_bg`'nin
  bağlaması değişebilir; değişirse `make shader` zorunlu.
- Yeni bağımlılık: yok. terminfo, tema, ayar şeması: yok.

## Checklist

- [x] Pay sol paydan (`gutter_px`) türetiliyor; formülün tek kopyası `bt_gpu::dock_px`
- [ ] Punto değişince pay ölçekleniyor (gözle)
- [x] Test: `split_into_grid` payı düşüyor; dejenere yükseklik kırpılıyor
- [x] `Motion` dock'u hedef olarak alıyor; `visible=false` durumu düşürmüyor
- [x] `settled()` dock ayağını sayıyor
- [x] Test: dock'a devir animasyonu sonlu adımda yerleşiyor
- [x] Test: snap ve belirme kipleri dock ayağında da geçerli
- [x] Caret tek kez çiziliyor; ters çevirme iki encode'a da gidiyor
- [x] Chevron yordamsal, `Face::Regular`'a çivili, kendi payı var
- [x] Test: chevron'un şekli, dikey hizası ve paya sığması bağlı
- [ ] Gerçek pencerede gözle: yazma, `sleep 5`, punto değişimi, vim gir-çık
- [ ] Doğrulama geçti (`make hepsi` + `make kur` + `make duman`)
- [ ] Yayın etkisi yazıldı
