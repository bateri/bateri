# Input Dock ve tabana yapışık içerik — Bağlam

## Mevcut Durum

**Izgara pencereyi tepeden doldurur.** `split_into_grid` (`app.rs:396`)
`rows = height_px / cell_h` diyor; dikeyde hiçbir çıkarma, hiçbir ofset yok.
Çizim orijini tek satırda: `Frame::pos_at` (`frame.rs:523`) →
`[gutter_px + col*w, row*h]`. Sol pay (010 Karar 3) oraya **resize zamanında**
`CellMetrics` ile geliyor; dikey karşılığı yok.

Sonuç: ekran dolmadan önce içerik tavana yapışık durur, altında boşluk kalır;
ekran dolduktan sonra kayar. **İki ayrı his**, aynı terminalde.

**Dock yok, prompt kabuğun.** `bateri.zsh` her prompt'ta OSC 133 `A`/`B`/`D`
basıyor ve blok kimliğini prompt'un hücrelerine OSC 8 ile çıpalıyor
(`bateri.zsh:203-221`); `frame()` o çıpayı **ızgaradan** okuyor
(`session.rs:1361`). Kullanıcı ızgaraya yazıyor, tuşlar `view.rs:374` →
`keys::encode_key` → `Session::send_input` (`session.rs:1948`) zincirinden
PTY'ye gidiyor.

`ShellPhase` (`shell.rs:90`) dört değer tutuyor ama **üretimde tüketicisi
yok**: `shell_state()`'i bugün yalnız sınamalar çağırıyor. 009 onu "011
kullanacak" diye bilerek bıraktı; safha → UI boru hattı henüz döşenmedi.

## Motivasyon

Kullanıcı kararı (2026-09-17): **içerik tabandan tavana doğru büyümeli.**
Ekran dolmadan önce de tabana yapışık dursun, yukarıda birikmesin.

Gerekçe ürün hissi: dolmamış ve dolmuş ekranın iki ayrı görünmesi tutarsız, ve
yazılan yerin altta sabit durması Input Dock'un (`docs/ARASTIRMA.md` →
Shell entegrasyonu) tasarımıyla aynı fikrin parçası — geçmiş üstte yukarı
akar, yazılan yer altta sabit kalır.

Bu set dock'un **önünü açıyor**: dock pencerenin altından satır alacak ve
içerik zaten oraya yaslanmış olacak. Dock'un kendisi ve prompt'un devri bu
sette **yok** (bkz. `discussion.md` → Karar 1).

## Kanıt

010'un teslim ekranı (`teslim.md` → B.2): on satırlık içerik, otuz satırlık
pencerede tavana yapışık duruyor ve altında yirmi satır boşluk kalıyor. Aynı
pencere dolduktan sonra kayan bir yüzeye dönüşüyor.

**Ölçüm iddiası yok.** Bu setin kare süresi, gecikme ya da bellek sayısı
iddiası yoktur; aşağıdaki maliyet cümlesi bir ölçüm değil kod okumasıdır.

## Mevcut Mimari

```
sync_geometry (app.rs:2197)
  ├─ renderer.cell_metrics(scale) ──► CellMetrics { cell_px, gutter_px }
  └─ split_into_grid(w, h, cell)  ──► Grid { cols, rows, cell }
                                        │         (rows = h / cell_h — dikey çıkarma YOK)
                        ┌───────────────┴───────────────┐
                        ▼                               ▼
          view.set_metrics(grid)              link.resize(cols, rows, cell)
          (view.rs:446 — RESIZE ZAMANI           └─► session.resize → TIOCSWINSZ
           ÖNBELLEĞİ)                            └─► iv.cell.set(cell)
                        │                               │
                        ▼                               ▼
          point_to_cell (view.rs:63)          Frame::clear(metrics) (frame.rs:254)
          x'te pay düşülüyor, y HAM                     │
                                                        ▼
                                              Frame::pos_at (frame.rs:523)
                                              [gutter + col*w, row*h]
                                                   ▲
                                                   └── push / push_cursor / kural / glyph
                                              Frame::push_block (frame.rs:350)
                                                   └── pos_at'ten GEÇMİYOR, y'yi kendi hesaplıyor
```

**Ömür ayrımı — bu setin kalbi.** Sol pay *resize zamanı* bir değer ve
`CellMetrics` ile taşınıyor. Tabana yapışma ofseti **kare başına içerik
durumu**: her yeni satırda değişiyor. `iv.cell` yalnız `DisplayLink::resize`'da
yazılıyor (`link.rs:1013`), yani `CellMetrics`'e bir `origin_y` eklemek
**kare başına tazelenmez** — 010'un çözümü buraya doğrudan kopyalanamaz.

**Ofsetin tüketicileri (010'daki üç değil, beş):**

| # | Tüketici | Yer | Not |
|---|---|---|---|
| 1 | `rows` hesabı | `split_into_grid` (`app.rs:415`) | Yalnız dock satır alırsa; bu sette **hayır** |
| 2 | Çizim orijini | `Frame::pos_at` (`frame.rs:523`) | Arka plan, glyph, kural, imleç dördü buradan |
| 3 | Komut işareti | `Frame::push_block` (`frame.rs:357`) | **`pos_at`'i atlıyor** — ofset yalnız `pos_at`'e eklenirse işaretler tepede kalır |
| 4 | Fare eşlemesi | `point_to_cell` (`view.rs:63`) | Girdisi **resize zamanı önbelleği** (`view.rs:182`); kare başına değişen ofset orada duramaz — **tasarımın asıl çatalı** |
| 5 | İmleç hareketi | `motion.sync` (`link.rs:624`) | Ekran dolmadan Enter'da imleç satırı 0→1 olur ama orijin bir satır iner: piksel hareketi sıfır, oysa `Motion` hücre uzayında interpolasyon yapıyor |

**Ofsetin kaynağı.** "Son dolu satır" doğrudan sorulabilir bir şey değil:
alacritty'de `Row::occ` `pub(crate)` ve `Dimensions` "görünür pencerede kaç
satır kullanıldı" sorusunu yanıtlamıyor. Ucuz cevap `frame()`'in **kendi
döngüsünün içinde**: atlama kapısından geçen en büyük `row` ile `cursor_row`'un
maksimumu. İkisi birden gerekli — imleç tek başına yetmez (imleci yukarı taşıyan
ilerleme çubuğu içeriği çıpanın altına iterdi), kapı tek başına yetmez (boş
prompt satırı kapıdan geçmiyor).

**Boşta sıfır kare — yol haritasının bedel cümlesi yanlıştı.** Orada "her yeni
satır bütün ekranı iter, kirli satır takibi devre dışı kalır" yazıyordu;
devre dışı kalacak bir **satır** takibi bugün yok. Hasar tek bir `AtomicBool`
(`Adapter.dirty`, `session.rs:605`) ve `Session::frame` zaten koşulsuz tam
tarıyor (`session.rs:1123`, `Term::damage()` bilinçli reddedilmiş). Ofset
**snap ettiği sürece** tabana yapışma sıfır ek kare getirir: içerik karesi
zaten çiziliyor. Tek risk kaymanın animasyonlu yapılmasıdır — o zaman durma
koşulu ve `reduce_motion` indirgemesi gerekir.

## Setin taşıdığı iki iş

> **Kapsam 2026-09-17'de ikinci turda daraldı** (`discussion.md` → Karar,
> 2. tur). Aşağıdaki beş işten **ilk ikisi** bu sette; 3, 4 ve 5 — prompt'un
> devri, çıpanın yeniden kurulması ve Input Dock — **012'ye** taşındı. Gerekçe
> iki bulgu: `>` bugünkü `Frame`'de temsil edilemiyor (yani prompt devralınıp
> yerine bir şey konamıyor) ve üçü de yalnızca prompt devralındığı için var.
> Liste tarihli kayıt olarak duruyor, çünkü 012'nin girdisi bu.

Kullanıcı kararlarıyla kapsam önce yol haritasının yazdığından da genişlemişti.
O hâliyle set beş iş taşıyordu ve **ikisi birbirine kilitliydi**:

1. **Tabana yapışık içerik** — saf yerleşim; ofset çizim zamanı uniform (3c).
2. **Yumuşak kayma** — içeriğin kayması animasyonlu (5b). `bt-gpu::motion`
   ikinci tüketicisini kazanıyor; durma koşulu ve `reduce_motion` indirgemesi
   borç.
3. **Prompt'u terminal devralır** (2a) — kabuk prompt basmaz; prompt'un
   içeriği, teması ve ayarlanabilirliği **bizim** sorumluluğumuz olur.
4. **010'un çıpa mekanizması yeniden kurulur** — 3'ün zorunlu sonucu. Blok
   kimliği bugün prompt'un hücrelerinden okunuyor; prompt çizilmezse hiç çıpa
   doğmaz. Kimliğin yeni kaynağı akış olmak zorunda, ve tarayıcı bugün
   `advance()`'ten **önce** koşuyor (`shell.rs:13-16`) — yani okuyucu yolu
   yeniden kurulacak.
5. **Input Dock** — pencerenin altında satır editörü; girdi yönlendirmesi
   (`view.rs:374` ile `Session::write` arası), satır ayırma ve safha kapısı.

**Mekanizma soruları — dördü de ikinci turda cevaplandı ve artık 012'nin
girdisi** (`discussion.md` → Karar 8, 10, 12 ve `## Muhakeme — 2. tur`):
dock ayna olmalı (asıl model zsh'in tek-tuş prompt'larında ölümcül); safha
kapısı hem "koşan komut girdi bekliyor" hem "yazmayı devralan uygulama"
sorusunu birden çözüyor, yani `ECHO` ve `ALT_SCREEN` yoklaması düşüyor; `B`
zaten sıfır genişlikte, hiçbir yere taşınmıyor. Aşağısı o turdan önceki
hâliyle duruyor:

- **Dock "ayna" mı "asıl" mı?** Tampon dock'ta olursa Tab tamamlama,
  Yukarı-geçmiş ve Ctrl-R kaybolur (üçü de ZLE'de) ve yeniden yazılmaları
  gerekir.
- **Koşan komut girdi beklerse** (`read`, `sudo`, `ssh` parolası) dock ne
  yapar? Bugün bunu söyleyen **hiçbir sinyal yok**; `ECHO` bayrağını master
  fd'den yoklamak bir aday ama ölçülmedi.
- **Yazmayı devralan uygulamalar** (Claude, Codex, REPL) nasıl tespit edilir?
  En güçlü sinyal `ALT_SCREEN` ve zaten döşeli; ama `\e[?1049h` açmayan
  REPL'ler (python, node) hiçbir bayrak açmıyor.
- **`B` işareti nereye taşınır?** Bugün PS1'in içinde (`bateri.zsh:218`);
  PS1 boşalınca dock'un aktivasyon sinyali de gider.
