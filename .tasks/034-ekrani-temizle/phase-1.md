# Phase 1 — Temizleme çekirdeği (`bt-core`)

## Özet

`Session`'a ⌘K ve ⌥⌘K'nin tek yöntemini, korunan ilk satırın yürüyüşünü ve
temizlemenin `2J` nesline, seçime ve aramaya bıraktığı izleri ekle; çağıran
henüz yok.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R1.5_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** —
  - Yeni `pub` yöntem(ler): ⌘K "başlangıca kadar", ⌥⌘K "yalnız geçmiş" —
    tek gövde, kip argümanı ya da iki ince sarmalayıcı; dönüş "temizlendi mi"
    (`false` alternatif ekran). İsim önerisi `clear_to_start` /
    `clear_scrollback`; menü seçicileriyle aynı sözlük.
  - Sıra, **tek** `Term` kilidi turunda: alternatif ekran → erken dönüş;
    `Scroll::Bottom` + `reset_scroll` (`send_input`'un ikilisi, gerekçesi
    `write_owned`'ın doc'unda; `Grid::scroll_up` ofseti büyüttüğü için
    kaydırmadan **önce**); korunan ilk satır `k`; `grid_mut().scroll_up(
    Line(0)..Line(rows), k)` (DECSTBM'yi bilerek atlıyor: temizleme bölgeye
    değil ekrana ait — yorumda); imleç ve `saved_cursor` `k` düşer (DECSC
    0'a kırpılır) — yalnız geçmiş kipinde bu kaydırma **hiç çağrılmaz**, `k = 0` ile değil; `clear_screen(ClearMode::Saved)` (`Handler` zaten
    import'ta, `ClearMode` eklenir); ızgara seçimi `clear_selection_locked` ile (her yolun kullandığı deyim), dock seçimi
    (`ShellLog::dock_selection`) temizlenir; `screen_clears` adlı yöntemle
    artar; defter nesli (`adapter.0.ledger`) artar ve aramanın geçerli
    eşleşmesi düşer. Kilit bırakıldıktan sonra `search_changed` ve
    `request_frame`.
  - **Yaprak kilit sırası:** safha/blok kimliği için `ShellLog`'a bakmak
    gerekirse kopya `Term` kilidinden **önce** alınır (modül başlığı); çıpa
    taraması kilidin altında.
  - **Korunan ilk satır:** imlecin satırındaki hücrelerin blok kimliği
    (prompt'un OSC 8 çıpası; okuma `frame()`'in çıpa okumasıyla aynı yardımcı
    üzerinden) — aynı kimliği taşıyan bitişik satırlarda yukarı yürü, en üstü
    `k`. `anchor_row_at_or_above` **değil**: o imlece en yakın çıpalı satırı
    veriyor ve bağlantı `preexec`'e kadar açık (Muhakeme). Yürüyüş 0. satıra
    dayanırsa (bloğun başı geçmişte) `k = 0`. İmlecin satırı çıpasızsa
    (komut koşuyor, entegrasyonsuz kabuk) `k` = imlecin satırı. Yalnız geçmiş
    kipinde `k = 0`.
  - **`screen_clears`'ın ikinci yazarı:** artırım adlı tek bir yöntemden
    (ör. `note_screen_clear`), `Term` kilidi altında ve temizleme
    uygulandıktan **sonra** — okuyucunun "uygulamadan önce say" kuralının
    tersi yönde ama aynı sonuç: kare yolu nesli gördüğünde ızgara zaten
    temiz. `TappedPty`'nin alanındaki "artıran yalnız burası" doc'u (~1312)
    ve `observe_screen_clear`'ın doc'u iki yazara göre güncellenir.
  - `request_frame`'in doc'u "defter neslini artırmaz" diyorsa bu yöntemin
    istisna olduğunu (defteri gerçekten değiştirdiği için) tek cümleyle
    söyle.
- **`crates/bt-core/src/search.rs`** (gerekirse) — geçerli eşleşmeyi
  "kayboldu"ya düşüren kol: geçmiş 0 → 0 temizlemesinde `ledger_shift`
  `Still` döner ve kaymış satırı geçerli gösterirdi; düşürme ölçütü
  "yalnız kesin kaynaklar" kuralının içinde yazılır (defter nesli bir
  **temizleme** nesliyle değiştiyse kayıp).

## Kabul

- Sınamalar (hermetik oturum, `session.rs` test modülünün emsalleri):
  - Dolu ekran + geçmiş, prompt son satırda → ⌘K: `history_size() == 0`,
    ızgarada yalnız prompt satırı, 0. satırda; imleç aynı sütunda, satırı
    0; sonraki `frame()`'de `fill == 0`, `scrolled == 0`.
  - Sarılan (ya da `PREBUFFER`'lı) giriş: bütün satırları korunur, üstleri
    gider (çıpa yürüyüşünün bekçisi — `anchor_row_at_or_above` ile kırmızı).
  - Çıpasız imleç satırı (koşan komut benzetimi): yalnız imlecin satırı
    kalır; PTY'ye yazılan bayt sayısı değişmez (kabuğa bayt gitmiyor).
  - ⌥⌘K: ızgara bayt bayt aynı, geçmiş sıfır.
  - Alternatif ekran: iki kip de `false` döner, ızgara ve iki ızgaranın
    geçmişi değişmez.
  - Temizlemeden sonra tek satır çıktı geçmişe düşünce doldurma **yalnız o
    satırı** verir (silinen ekran geri gelmez).
  - Seçim (ızgara ve dock) temizlenir; kaydırılmış pencere dibe döner ve
    kesir sıfırdır.
  - Arama etkin, geçmiş 0 iken ⌘K → geçerli eşleşme yok (`Still` değil),
    sayım yeniden başlar (`Wake::search_changed` sayacı artar).

## Checklist

- [ ] Temizleme yöntemi (iki kip) ve korunan ilk satır yürüyüşü
- [ ] `screen_clears`'ın adlı ikinci yazarı + iki doc güncellemesi
- [ ] Seçim, arama (defter nesli, geçerli eşleşme, `search_changed`) ve kare talebi
- [ ] Test: yukarıdaki yedi senaryo
- [ ] Doğrulama geçti (kapı komutu + tetiklenen koşullu komutlar: `make test-yaris` — ana thread `Term` kilidini alıp paylaşılan atomiğe ikinci yazar oluyor)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
