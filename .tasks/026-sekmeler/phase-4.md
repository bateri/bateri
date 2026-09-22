# Phase 4 — Kromu boya ve seti kapat

## Özet

Başlık çubuğu temanın zeminine ve açıklığına bağlanır, tema değişimi bütün
pencerelere uygulanır; yol haritası güncellenir ve set kapısı koşar.

_Requirements: R4, R5_

## Değişiklikler

- **`crates/bt-shell/src/window.rs`** — pencere kurulumunda ve tema
  değişiminde tek bir `apply_chrome(&Theme)`: `titlebarAppearsTransparent(true)`,
  `titlebarSeparatorStyle` = none, `backgroundColor` temanın `background`'ının
  **sRGB** hâlinden (`NSColor` sRGB; lineer değer `bt-gpu`'nun, buraya
  gitmez — `CLAUDE.md` → Renk uzayı), `appearance` temanın zemininin
  açıklığından (koyu → DarkAqua, açık → Aqua). Açıklık eşiği ve onu veren saf
  fonksiyon `bt-core`'un `Theme`'inde değil `bt-shell`'de; sınanıyor.
- **`crates/bt-shell/src/app.rs`** — temayı değiştiren iki yol
  (`reload_settings`, `apply_appearance`) `set_theme`'den sonra her pencereye
  `apply_chrome`. `apply_appearance` son görülen `NSApp` koyu/açık bitini
  tutar ve değişmediyse erken döner: pencere görünümünü kurmak her view'da
  `viewDidChangeEffectiveAppearance` doğuruyor ve bit sorulmasa tema
  değişimi başına N×N no-op tema seçimi olurdu. `dark_appearance`'ın doc'u
  "`NSApp`'ten okunması artık **zorunlu** — view'ın görünümü temayı
  yansıtıyor, sistemi değil" diye düzeltilir. Krom çağrısı `settings` ve
  `notices` ödünçleri bırakıldıktan sonra.
- **`docs/YOL-HARITASI.md`** — 026 satırı teslim diliyle tek satıra; bölme
  satırındaki bedel notu "026 sekmeyi pencereye koydu" diye güncel.
- **`CLAUDE.md`** — Bugünkü hâl'in sekme cümlesine krom (tek cümle +
  işaretçi).

## Kabul

- Açıklık fonksiyonunun sınaması (siyah → koyu, beyaz → açık, gömülü
  `bateri` ve `bateri-light`).
- `make duman` jetonları aynı.
- **Set kapısı** (`duzen.md` → Kalite kapısı): `/code-review` setin
  aralığına, `/audit` (bt-core ve bt-shell değişti; mercek 4 thread ve
  blokaj, mercek 5 boşta sıfır kare, mercek 7 belge), bulgular sonrası
  doğrulama yeniden.
- **Gözle kontrol sahnesi** (devir mesajına tek satır, üç yüzey: ızgara,
  dock, doldurma bandı her sekmede aynı çalışıyor):
  1. Tek sekme: çubuk yok, başlık çubuğu temanın zemininde, içerikle
     arasında çizgi yok; tema `bateri-light`'a geçince başlık çubuğu ve
     trafik ışıkları açığa dönüyor.
  2. ⌘T ile iki sekme: çubuk beliriyor (sistem animasyonu), ikinci sekme
     birincinin dizininde, dock ve bağlam satırı ikisinde de doğru; sekme
     başlıkları dizin adı, birinde vim açınca vim'in başlığı.
  3. İki sekmede tema değişimi: ikisi de aynı anda.
  4. Ctrl-Tab / Ctrl-Shift-Tab, ⇧⌘]/⇧⌘[ (Türkçe Q'da da), ⌘1, ⌘9; Ctrl-I
     zsh'te hâlâ sekme ekliyor.
  5. Sekmeyi sürükleyip sıralama ve pencereden koparma; View ▸ Show All
     Tabs.
  6. Arka plandaki sekmede `sleep 60`: öne gelince süre sayacı güncel,
     imleç kaymadan yerinde; iki pencereden birinde vim'e girip çıkmak
     yalnız o pencerenin dock'unu oynatıyor.
  7. ⌘+ yalnız o sekmede; yeni sekme büyütülmüş puntoyu devralıyor.
  8. Son sekmede `exit`: pencere kapanıyor, uygulama açık; Dock ikonu yeni
     pencere; ⌘Q üç sekmeyle beklemeden kapanıyor ve `ps`'te zsh kalmıyor.

## Checklist

- [ ] `apply_chrome` kurulumda ve iki tema yolunda, bütün pencerelere
- [ ] `apply_appearance` değişmeyen sistem bitinde erken dönüyor; `dark_appearance` doc'u
- [ ] Test: açıklık fonksiyonu
- [ ] `docs/YOL-HARITASI.md` ve `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
