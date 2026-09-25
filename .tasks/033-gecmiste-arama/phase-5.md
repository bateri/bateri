# Phase 5 — Bütün defterin sayımı

## Özet

Etiket "3 of 17" olur: çıpasız bir dizin defteri dipten yukarı parça parça
sayar, sorgu ya da defter değişince baştan başlar ve sürerken "…" gösterir;
geçerli eşleşme akan çıktıda içeriğine yapışık kalır.

_Requirements: R8_

## Değişiklikler

- **`crates/bt-core/src/search.rs`** — dizin: sıralı eşleşme listesi (en yeni
  = 1), nesil, ilerleme. `Session::search_step`: `Term` kilidini bir parça
  kadar tutar (parça boyu tasarım sabiti; doc'u "ölçülmedi" der, türetmesi
  yok), dizinin kendi desen kopyasıyla (kilitsiz sahiplik, phase-1), nesil
  eskiyse düşer. Sonucu: ilerliyor/bitti + sayım + geçerlinin sırası.
  Parça **hasar dikmez**.
- **`crates/bt-core/src/session.rs`** — defter değişimi (history büyüdü ya
  da ekran içeriği değişti) arama açıkken `Wake` üzerinden **yüksüz ve
  kenarda** bir haber (`title_changed` emsali; bayrak tüketilene kadar ikinci
  haber yok); sayım baştan. Geçerli eşleşmenin kayması: pencere
  kaydırılmışken `display_offset` farkı, dipte ve defter doymamışken
  `history_size` farkı; ikisi de tutmuyorsa yeniden sayımın sonunda
  pencereye en yakın eşleşme.
  **Yakınsama kuralı:** haber uçuştaki geçişi **kesmez**; geçiş biter, sonra
  tam olarak bir yeniden geçiş koşar (haber kenarda kurulduğu için bir
  patlama başına en çok bir ek geçiş). Sürücünün durma koşulu: geçiş bitti
  **ve** bekleyen haber yok. Kesen bir kural `yes` akarken geçişi hiç
  bitirmez ve ana kuyruk `Term`'i her turda sonsuza dek kilitlerdi. Alternatif ekrana giriş/çıkış dizini sıfırlar.
- **`crates/bt-core/src/wake.rs`** — yeni haber (varsayılan gövdeli, mevcut
  uygulayıcılar etkilenmez).
- **`crates/bt-shell/src/search_bar.rs`**, **`window.rs`** — sürücü: ana
  kuyrukta bir parça, bitmediyse bir sonraki tura yeniden kurulur (tuş
  olayları aradan girer); panel kapanınca ya da sekme kapanınca durur. Haberi
  `ShellWake` alır ve yalnız panel açıkken sürücüyü başlatır; arka sekmede de
  işler. Etiket "3 of 17", sürerken "3 of 17…", "No matches".
- **`docs/OLCUMLER.md`** → `## Bekleyen iddialar` — "parça boyu kilidi tuş
  gecikmesi hissettirmeyecek kadar kısa tutuyor" iddiası, ölçülmemiş.
- **`CLAUDE.md`** — `bt-shell` ve `bt-core` paragraflarına arama cümleleri
  (kural + tek cümle gerekçe + bu sete işaretçi): panel AppKit ve PTY'yi
  itmiyor; vurgu içerik karesinde görünür satırlarla sınırlı; sayım çıpasız
  ve parça parça; odak iki bit; `bt-gpu` satırındaki "overlay'ler (palet,
  arama)" ibaresi "palet"e iner.

## Kabul

- Hermetik: sayım bütün defterde doğru ve dipten yukarı; parça sınırında
  eşleşme kaybolmuyor ya da iki kez sayılmıyor; yeni sorgu eski neslin
  parçasını düşürüyor; parça hasar dikmiyor; **görünür tarama ile dizin aynı
  `Term`'de aynı aralıkları veriyor**; doymuş defterde geçerli eşleşme
  yanlış satıra geçmiyor; kaydırılmış pencerede çıktı gelince geçerli
  eşleşme içeriğine yapışık; bastırılan giriş satırı sayılmıyor.
- `make hepsi` ve `make test-yaris` yeşil; `make duman` jetonları değişmedi.
- Gözle kontrol (set kapısının mesajında): `seq 1 20000` sonrası arama —
  sayım oturuyor, yazarken gecikme yok; `yes` akarken panel açık — "…" ve
  çıktı durunca oturma; arka sekmede çıktı gelip öne dönünce sayım güncel.

## Checklist

- [ ] Dizin + `search_step` + nesil iptali
- [ ] `Wake` haberi ve sürücü
- [ ] Geçerli eşleşmenin kayması
- [ ] `docs/OLCUMLER.md` bekleyen iddia, `CLAUDE.md` sözleşme cümleleri
- [ ] Dizinin eşleşme kümesi vurgununkiyle aynı (phase-1'den devir): bastırılan giriş satırına değen ve mürekkepsiz eşleşme sayılmaz (`suppressed_rows`, `search::has_ink`); bekçisi aynı ekranda vurgu ile dizinin sayısını karşılaştırır
- [ ] Geçerli eşleşmenin yuvadaki mutlak `Match`'i (`SearchSlot::current`, phase-4) kaymanın konusu: `display_offset`/`history_size` farkı onu taşımalı; etiketin görünür sayımı (`SearchReport::visible`) bütün defterin "3 of 17"sine dönmeli (phase-4'ten devir)
- [ ] Gerçek pencerede gözle kontrol (phase-4'ten devir, computer-use meşguldü): panelin iki temadaki yüzeyi (zemin/kenar/gölge), açılış/kapanış animasyonunun hissi (180 ms), vurgu renkleri (eşleşme/geçerli/seçim ayrımı, odaksız solma), alan odaktayken caret'in içi boş ve vurgu tam renkli, ⏎/⇧⏎/Esc/⌘G/⌘E, ölü tuş ve ⌘V alanda, panel açık boştayken kare yok; phase-3'ün regresyon listesi (yazma, fareyle seçim, Finder damlası, boyutlandırma, ikinci sekme, Cmd +/−)
- [ ] Test: yukarıdaki senaryolar
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
