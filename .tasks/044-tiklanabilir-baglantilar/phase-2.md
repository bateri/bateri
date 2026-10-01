# Phase 2 — Hover yuvası ve alt çizgi (`bt-core`)

## Özet

Hover yuvası kurulunca bağlantının hücreleri üç yüzeyde alt çizgiyle
çiziliyor; bayat yuva çizilmiyor, düşüyor ve ana kuyruğa haber veriyor.

_Requirements: R4, R4.1, R4.2_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Session::set_link_hover(Option<LinkHover>)`
  (`LinkHover`: phase-1'in `LinkHit` aralığı + damga + stil
  `Single`/`Dashed`). Yaprak kilit, `Theme` örüntüsü: `frame()` kopyayı
  `Term` kilidinden **önce** alır; aynı değerle kurmak no-op ve uyandırmaz,
  değişim hasar diker (`Waker::wake`). `frame()` kilit altında damgayı
  (`LedgerMark` + OSC 8'de hücrenin `Hyperlink`'i) sınar; tutmazsa aralığı
  çizmez, yuvayı düşürür ve `Wake::link_hover_lost` (yüksüz, kenarda) ile
  haber verir. Alt çizgi ezmesi **tek yardımcıda** (hücre aralıktaysa
  `underline` = stil, `underline_color` = `None`) ve üç sink'ten çağrılır:
  ızgara, doldurma bandı, `Session::dock` (dock'un yuvası phase-5'te dolar).
  Hover yokken maliyet tek dal.
- **`crates/bt-core/src/wake.rs`** — `link_hover_lost` (varsayılan gövdesi
  boş, mevcut `Wake` uygulayıcıları değişmeden derlenir).

## Kabul

- Sınama: hover kurulu karede aralığın hücreleri `UnderlineStyle::Single`,
  dışındakiler değişmemiş; aynı satır yerinde yeniden yazılınca (`\r` +
  başka metin) bir sonraki karede ezme yok ve `link_hover_lost` bir kez
  çağrıldı; OSC 8 aralığında hücrenin bağlantısı değişince aynı; doldurma
  bandının hücreleri de eziliyor; aynı hover'ı ikinci kez kurmak uyandırma
  saymıyor.
- `make check`, `make linux`, `make test-race` yeşil.

## Checklist

- [x] Hover yuvası, `set_link_hover`, no-op kuralı
- [x] `frame()`'de damga denetimi, düşürme ve `Wake::link_hover_lost`
- [x] Tek ezme yardımcısı, üç sink — iki sink; dock'unki phase-5'e devredildi (Uygulama Notları)
- [x] Test: ezme, yerinde yeniden yazım, OSC 8 değişimi, bant, no-op
- [x] Doğrulama geçti (`make check` + `make linux` + `make test-race`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Sınırın biçimi.** `LinkHover { spans, stamp, style }` (`pub`), kurucusu
  `LinkHit::hover(style)` — damga opak kalıyor. Yuva `Session`'da
  `Mutex<Option<Arc<LinkHover>>>`: `frame()`'in `Term`'den önceki kopyası bir
  referans sayımı, düşürme `Arc::ptr_eq` ile yalnız denetlenen kopya hâlâ
  yuvadaysa (arada view'ın kurduğu yeni hover düşmüyor).
- **Damga denetimi glide'dan sonra**, `renderable_content`'ten önce
  (`Session::link_hover_holds`): glide ofseti oynatabiliyor ve damga onu
  taşıyor. Düşürme ve `Wake::link_hover_lost` `drop(term)`'dan sonra.
- **Boşluk da çiziliyor (ürün kararı, kullanıcı tarafı).** Bağlantının içindeki
  çizilecek bir şeyi olmayan hücre (OSC 8 metninde `click here`'ın boşluğu)
  atlama kapısından hover sayesinde geçiyor; geçmeseydi alt çizgi boşlukta
  kopardı. `ruled`'a katılmadı: `drawable` onu okuyor ve seçim fareyle
  oynamamalı. Yalnız hover yüzünden geçen hücre (`hover_only`) doluluğa
  (`drawn_rows`), çıpa taramasına ve süre sayacının `last_col`'una **girmiyor**
  — `/code-review` bulgusu: girseydi ⌘ basınca ızgara bir satır kayar ya da
  sayaç kaybolurdu (bekçi `a_hover_only_blank_row_does_not_count_as_filled`).
  `HIDDEN` hücre çizgi almıyor (tek `let` kuralı).
- **SAPMA — `Wake::link_hover_lost`'un varsayılan gövdesi yok.** phase-2.md
  "varsayılan gövdesi boş" diyordu; trait'in kendi sözleşmesi "hiçbir çağrının
  varsayılan gövdesi yok, unutulan uygulayıcıyı derleyici söylesin". Kod
  sözleşmesi izlendi: üç uygulayıcıya gövde (`ShellWake` no-op + phase-4
  işaretçisi, `SilentWake` no-op, `TestWake` sayıyor).
- **DEVREDİLEN — dock sink'i phase-5'e.** `LinkHit`'in aralıkları ekran satırı
  uzayında ve `LinkPoint::Dock` bugün `None`; `Session::dock`'a her zaman
  `None` dönen bir sarmalayıcı koymak yapısal olarak ölü kod olurdu. Yardımcı
  yüzeyden bağımsız (`hover_style` + `underline_link`), phase-5'te tek satır;
  checklist'ine yazıldı.
- **OSC 8 karşılaştırması PTY'den tek başına erişilemiyor.** Her çıktı turu
  `epoch`'u ilerletiyor, yani yerinde yeniden yazılan bağlantıyı önce işaret
  yakalıyor. Hyperlink kolunu gerçekten sınayan bekçi damgayı elle değiştiriyor
  (`a_changed_hyperlink_under_the_same_ledger_is_stale`); kol bir savunma.
- **phase-4'ün bilmesi gereken bedel:** hover kuruluyken akan çıktı her turda
  damgayı bozuyor → kare düşürüyor → `link_hover_lost` → view yeniden buluyor →
  `set_link_hover` → bir kare daha. Çıktı başına iki kare, çıktıyla sınırlı
  (boşta kare yok); `make smoke` bunu görmez.
- `make test-race`'e `race_link_hover_and_frame` eklendi (setter + frame +
  akan çıktı; kilit sırası bozulursa asılır).
