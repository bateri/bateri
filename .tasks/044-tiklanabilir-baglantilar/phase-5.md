# Phase 5 — Dock yüzeyi

## Özet

Dock'un giriş satırına yazılmış URL ya da yol aynı kurallarla ⌘-hover'da
vurgulanıyor ve ⌘-tıkla açılıyor.

_Requirements: R8_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `LinkPoint::Dock` kolu: metin
  `dock::selectable`'dan, isabet son çizilen pencerenin çözümleyicisinden
  (`DockWindow`, `dock_select`'in yolu) ve dock'un tek düzen yürüyüşünden
  (`dock_layout`); `link::scan` aynı. Damga aynanın nesli / `BUFFER`'ı; dock
  hover'ı `Session::dock`'taki ezme yardımcısından çizilir (phase-2).
  Bağlam satırı bağlantı değil; uzak oturumda giriş satırı yok.
- **`crates/bt-shell-macos/src/view.rs`** — dock'un `window_point_dock`'u
  hover ve ⌘-tık yolunda; dock basışında da ⌘ + doğrulanmış bağlantı
  `pressed_dock`'tan önce.

## Kabul

- Sınama: `BUFFER` = `open https://a.dev/x` → dock noktasında `LinkHit`;
  sarılmış satırda bölünen URL tek bağlantı; `BUFFER` değişince damga
  tutmuyor.
- Gözle kontrol: dock'a `open https://example.com` yaz, ⌘-hover alt çizgi,
  ⌘-tık açar, caret yerinden oynamaz.
- `make check` + `make linux` + `make smoke` yeşil.

## Checklist

- [x] `LinkPoint::Dock` hit testi ve damgası
- [x] (phase-2'den devralındı) `Session::dock`'un sink'ine ezme yardımcısını bağla: `hover_style`/`underline_link` (`session.rs`) dock hover'ıyla; `LinkHover`'ın yüzeyi (ekran mı dock mu) ve dock damgası (aynanın nesli / `BUFFER`) burada doğuyor, `frame()`'in damga denetimi dock hover'ını bayat saymamalı
- [x] View'da dock bağlama
- [x] Test: dock hit, sarma, bayatlık
- [x] Doğrulama geçti (`make check` + `make linux` + `make smoke`)

## Uygulama Notları

- **SAPMA — ezme sink sarmalayıcısında değil, `dock::render_with`'in
  içinde.** Pencerenin tepesi (`top`) yürüyüşün **içinde** hesaplanıp
  sonra dönüyor; `(satır, sütun)` ile eşleşen bir sarmalayıcı hücreler
  geçerken hangi pencereye karşı eşleştiğini bilemezdi (tekerlek ya da caret
  tepeyi oynatınca alt çizgi kayardı). Seçimin emsali izlendi: hover
  `render_with`'e seçilebilir metnin karakter aralığı + stil olarak giriyor,
  `underline_link` (artık `pub(crate)`, tek ezme yardımcısı) baş hücreye ve
  spacer'a uygulanıyor. Sıra: seçim koşusunun çizilirlik ölçütünden
  **sonra**, sink kapısından önce — hover seçim içeriği yaratmıyor
  (ızgaranın `ruled` kuralının ikizi). `hover_style` ekrana özgü kaldı.
- **Damga.** `LinkStamp` içte bir enum (`Screen { mark, hyperlink }` /
  `Dock { text, range, top, cols }`); dock damgası seçilebilir metin
  (`PREBUFFER ++ BUFFER`) + çizilen pencere, tutma koşulu `Live` + aynı metin +
  aynı pencere (aşağıda, `/code-review` bulgusu). Sınırda
  `LinkHit::in_dock`/`LinkHover::in_dock`. `frame()` dock hover'ına hiç
  bakmıyor (ne çiziyor ne düşürüyor — çıktı dock metnini bayatlatmaz);
  denetleyen `Session::dock`: yuvayı `shell`'den **önce** kopyalıyor, tutmayı
  aynı turda soruyor, bayatı kilitsiz düşürüyor. Düşürme tek yardımcı
  (`drop_link_hover`, `Arc::ptr_eq` + `Wake::link_hover_lost`), iki çağıran.
- **İsabet `DockWindow::hit` değil**, `dock::link_at`: `hit` payı, boşluğu ve
  öneriyi en yakın karaktere indiriyor (tık bir yere gitmeli); bağlantı yalnız
  çizildiği karakterin altında yanıyor — prompt işareti, `PREDISPLAY`, öneri
  ve satırın sağındaki boşluk bağlantı değil. Aynı `dock_layout` yürüyüşü iki
  kez (karakteri bul, aralığın hücrelerini topla); akış → seçilebilir indeks
  eşlemesi tek fonksiyon (`selectable_index`, `render_with`'in kapanışı ondan).
  Kapılar: çizilen iz var, `buffer_bytes` tutuyor (`dock_select`'in kuralı),
  satır pencerenin içinde. Uzak/`file://` kararı `link_allowed`'a çıktı, iki
  kol aynı fonksiyonu çağırıyor.
- **View.** `LinkCell { Screen(i32, u16), Dock(u16, u16) }`: dock'un 0.
  satırı ile ekranın 0. satırı karışmasın. Izgara/bant boşsa
  `window_point_dock(.., Reject)` (artık `pub(crate)`); el imlecinin
  dikdörtgenleri dock'ta `Origin::dock`'un tepesinden. Basışta ön-rota zaten
  `pressed_dock`'tan önceydi; bırakma `Release::Link` olduğu için
  `dock_click` çağrılmıyor, caret yerinden oynamıyor.
- **`/code-review` bulgusu — damga pencereyi de taşıyor.** Yalnız metin
  damgası pencere oynayınca (tekerlek, ⌘←'nün caret takibi, yeniden saran
  resize) tutmaya devam ediyordu: alt çizgi karakter aralığından doğru
  yerdeydi ama view'ın hücreleri (el imleci, tık eşleşmesi) eskiydi — ⌘-tık
  eski yerde açıyor, yeni yerde kaçırıyordu. Damgaya isabet anının tepesi ve
  genişliği girdi; `Session::dock` çizimin kendi formülüyle
  (`dock::window_of`, `render_with` de onu çağırıyor) karşılaştırıyor, tutmazsa
  çizmiyor ve düşürüyor → view yeniden buluyor. Bekçi
  `a_dock_hover_drops_when_the_window_moves`.
- `make test-race`'e `race_dock_link_hover_and_dock` eklendi (dock'un yuva
  okuması + düşürmesi, `link_at(Dock)`, `BUFFER`'ı durmadan değiştiren ayna).
- **Gözle kontrol devirde** (set kapısı): dock'a `open https://example.com`,
  ⌘-hover alt çizgi + el, ⌘-tık açar, caret yerinde.
