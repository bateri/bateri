# Ayarlar, tema ve font — Bağlam

## Mevcut Durum

**Ayar yok; her görünüş kararı bir sabit.** Kullanıcının değiştirebildiği tek
şey yok, `~/.config/bateri/` hiç okunmuyor:

| ne | nerede | bugün |
|---|---|---|
| geçmiş satırı | `bt-shell/src/app.rs:31` | `SCROLLBACK = 10_000` → `SessionOptions.scrollback` |
| punto | `bt-gpu/src/renderer.rs:54` | `POINT_SIZE = 13.0` → `Atlas::new/ensure` |
| aile | `bt-atlas/src/font.rs:17,22` | `PREFERRED = ["SF Mono"]`, taban `FALLBACK = "Menlo"` |
| palet | `bt-core/src/color.rs:79-95` | `BG`, `FG`, `CURSOR`, 16'lık `ANSI` — `const` |
| pencere zemini, imleç | `bt-core` `pub const DEFAULT_BG/DEFAULT_CURSOR` | `bt-gpu/src/link.rs:358,371` doğrudan okuyor |

Workspace'te `toml` ve `serde` **yok** (CLAUDE.md onları taban bağımlılık
sayıyor ama hiçbir crate bağlanmadı). `docs/AYARLAR.md` yok —
`proje.md`'nin "ayar şeması" yayın etkisi onu arar.

**Renk yolu.** `Session::frame()` her hücrenin rengini `color::resolve` ile
çözer (önce uygulamanın OSC 4/10/11 tablosu, boşsa `color::default`), sonra
sınırda lineerleştirir (`linear_rgba`). Varsayılan arka planlı hücre
çizilmez: karşılaştırma `back != color::BG_RGB` (`session.rs:895`); pencere
zemini de `DEFAULT_BG`'den gelir. İkisinin **aynı kaynaktan** beslenmesi
kural (`color.rs:59-62`) ve bekçisi `default_background_has_one_source`.
Uygulamanın renk sorgusu (`Event::ColorRequest`, `session.rs:479`) da
`color::default`'tan cevaplanır — vim/nvim açık/koyu zemine buna bakarak
karar verir. SGR 2 (sönük) `×2/3` çarpımıdır (vte'nin kuralı), yani **koyu
zemin varsayar**: açık zeminde sönük metin koyulaşır, soluklaşmaz.

**Font yolu.** `bt-shell` `Renderer::cell_metrics(scale)` çağırır →
`sync_atlas` → `Atlas::ensure(POINT_SIZE, scale)`. Atlasın anahtarı
`(point_size, scale)`: ölçek ya da punto değişince atlas ve doku yeniden
kurulur, aile anahtarda **yok**. `font::open` CoreText'in sessiz ikamesini
aile adını karşılaştırarak zaten yakalıyor. Doku kenarı sabit
(`TEXTURE_EDGE`), tahliye yok: büyük punto yuvaları daha çabuk tüketir ve
dolan atlas tofu'ya düşer; doluluk `make duman`'ın `yuva=` jetonunda görünür.
Punto `effective_point_size`'da `MIN_POINT_SIZE..MAX_POINT_SIZE`'a kırpılır.

**OSC 52.** alacritty'nin `term::Config.osc52` varsayılanı `OnlyCopy`: `Term`
yazma dizisini base64'ten çözüp `Event::ClipboardStore(tür, metin)` yolluyor.
Olay `Adapter`'da **düşüyor** (`session.rs:499`). Okuma dizisi `OnlyCopy`'de
hiç olay üretmiyor. Panoya yazan köprü hazır: `bt-shell/src/clipboard.rs`
`copy` (Cmd-C onu kullanıyor, boş metni yazmama kapısıyla).

**Ana menü yok.** Uygulama `NSMenu` kurmuyor; Command'lı tuşları
`BateriView::keyDown:` yakalıyor, yalnız Cmd-C/V'yi geçici bir köprüyle
panoya çeviriyor ve gerisini yutuyor (`view.rs:309-319`, köprünün doc'u
"menü günü silinir" diyor, `:491-493`). Sonuçları: **Cmd-Q çalışmıyor**
(`app.rs:332`), sistemin açık/koyu görünümü hiçbir şeye bağlı değil.

**About paneli erişilemiyor** (yol haritasından taşındı). 006 phase-4 atfı
AppKit'in standart About panelinin okuduğu `Credits.html` ile pakete koydu,
ama paneli açan menü öğesi yok. Menüyü getiren set
`orderFrontStandardAboutPanel:`'ı bağlar ve paneli **gözle** doğrular —
bugüne kadar hiç görülmedi.

**Ölçeğin `bt-gpu`'ya iki kapısı** (yol haritasından taşındı). `sync_geometry`
(`app.rs:1032-1047`) ölçeği bir kez okuyup komşu iki çağrıya veriyor:
`Surface::set_size(w, h, scale)` ve `Renderer::cell_metrics(scale)`. 003
phase-4 ikisini `Renderer::resize(surface, w, h, scale) -> CellMetrics`'e
birleştirmeyi 003 R5'in harfiyle ("grid ölçüsü `cell_metrics(scale)`'ten
gelir") çeliştiği için reddetti; 004 yeniden erteledi (`004/plan.md` → Kapsam
Dışı), yol haritası "ayar seti ölçeği zaten elleyecek" diye 007'ye bağladı.

## Motivasyon

Yol haritasında 007 **görünüşün temeli**: 008 (hareket + imleç) ayarlarını bu
setin dosyasından, 009 (materyal yüzey) renklerini bu setin tema rollerinden
okuyacak. İkisi de bu set olmadan ya kendi sabitini icat eder ya da bekler.

Referans davranış `docs/ARASTIRMA.md` → Görünüm (sekiz rol, düz paletler,
`themes/` dizini) ve aynı bölümün sonundaki ayar anahtarı listesi
(`appearance.theme`, `font_size`, `family`, `line_height`, `scrollback`,
`clipboard.osc52`). Metalterm'in tema değerleri ve dosya biçimi kapalı —
adları biliyoruz, şemayı biz koyuyoruz.

OSC 52 yazma yönü 006'dan ertelendi: "ayarsız açılan yazma yönü sonradan
kapı ekletir" (`006/discussion.md` → Karar 5 ve Muhakeme). Ayar anahtarı bu
setle doğuyor.

Günlük kullanımda (006 eşiğinden sonra) görünen eksik: font ve punto
değişmiyor; SF Mono Xcode'la geliyor, olmayan makinede Menlo açılıyor ve
kullanıcının bunu değiştirecek yolu yok.

## Kanıt

Sayı yok, bu set ölçüm iddiası taşımıyor. Kanıt kodun kendisi (yukarıdaki
satır numaraları) ve iki gözlem:

- `~/.config/bateri` bu makinede yok (`ls` → "No such file or directory"):
  ayar okuma yolunun ilk hâli "dosya yok"tur ve sessiz, doğru bir hâl olmalı.
- `alacritty_terminal-0.26.0/src/term/mod.rs` → `Osc52` enum'u ve
  `clipboard_store`: yazma dizisi `Term`'de hiçbir ayar dokunmadan kabul
  ediliyor; köprü yalnız bizim tarafta eksik.

## Mevcut Mimari

```
bt-shell (ana thread)                  bt-gpu                     bt-atlas / bt-core
─────────────────────                  ──────                     ──────────────────
start_session ── SessionOptions{scrollback: SCROLLBACK} ───────────▶ Session::spawn
                                                                     Term(Config{osc52: OnlyCopy})
sync_geometry ─┬ Surface::set_size(w,h,scale)                       │
               └ Renderer::cell_metrics(scale) ─ sync_atlas ───────▶ Atlas::ensure(POINT_SIZE, scale)
                                                                     font::open_chain (SF Mono→Menlo)
DisplayLink (ana thread)
  session.frame(sink) ◀──────────── color::resolve/default (const palet), BG_RGB atlaması
  draw(DEFAULT_BG) , push_cursor(DEFAULT_CURSOR)   ◀── pub const

okuyucu thread (Term kilidi tutulurken)
  Adapter::send_event ─ ColorRequest → color::default   ─ ClipboardStore → düşüyor
                      └ Wakeup → Wake::wake → ShellWake → ana kuyruk
```
