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

- [x] Dizin + `search_step` + nesil iptali
- [x] `Wake` haberi ve sürücü
- [x] Geçerli eşleşmenin kayması
- [x] `docs/OLCUMLER.md` bekleyen iddia, `CLAUDE.md` sözleşme cümleleri
- [x] Dizinin eşleşme kümesi vurgununkiyle aynı (phase-1'den devir): bastırılan giriş satırına değen ve mürekkepsiz eşleşme sayılmaz (`suppressed_rows`, `search::has_ink`); bekçisi aynı ekranda vurgu ile dizinin sayısını karşılaştırır
- [x] Geçerli eşleşmenin yuvadaki mutlak `Match`'i (`SearchSlot::current`, phase-4) kaymanın konusu: `display_offset`/`history_size` farkı onu taşımalı; etiketin görünür sayımı (`SearchReport::visible`) bütün defterin "3 of 17"sine dönmeli (phase-4'ten devir)
- [~] Gerçek pencerede gözle kontrol (phase-4'ten devir, computer-use meşguldü): panelin iki temadaki yüzeyi (zemin/kenar/gölge), açılış/kapanış animasyonunun hissi (180 ms), vurgu renkleri (eşleşme/geçerli/seçim ayrımı, odaksız solma), alan odaktayken caret'in içi boş ve vurgu tam renkli, ⏎/⇧⏎/Esc/⌘G/⌘E, ölü tuş ve ⌘V alanda, panel açık boştayken kare yok; phase-3'ün regresyon listesi (yazma, fareyle seçim, Finder damlası, boyutlandırma, ikinci sekme, Cmd +/−) — **yapılamadı**: computer-use iki denemede de başka bir Claude oturumunca meşguldü; setin kapanış mesajında kullanıcıya bırakıldı
- [x] Test: yukarıdaki senaryolar
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Set kapısı: `/code-review` (on bulgu: yedisi düzeltildi, ikisi waive, biri CLAUDE.md kısaltması) ve `/audit` (yedi mercek temiz; hücre/shader merceği ilgisiz) koştu, doğrulama yeniden yeşil

## Uygulama Notları

- **Test-first sırası:** sınamalar uygulamadan sonra yazıldı; ısırdıkları
  mutasyonla gösterildi (parçanın "son satırı içeride" süzgecini kaldırmak
  dikiş sınamasını, doymuş dipteki kaybı `Still`'e çevirmek doymuş defter
  sınamasını kırdı).
- **Dizin eşleşmeleri saklamıyor, sayıyor** (plan "sıralı eşleşme listesi"
  diyordu): `.` gibi bir desen on bin satırda milyonlarca eşleşme. Geçerli
  eşleşmenin sırası geçiş ona varınca yazılıyor, gezinmede ±1 taşınıyor
  (sarmada 1 ya da — dizin bittiyse — toplam); sırası bilinmeyen yeni bir
  eşleşme bitmiş dizinde bir geçiş daha istiyor.
- **`Wake::search_changed` varsayılan gövdesiz** (plan "varsayılan gövdeli"
  diyordu): `wake.rs`'in sözleşmesi "hiçbir çağrının varsayılan gövdesi yok,
  derleme söylesin"; üç uygulayıcı (`ShellWake`, `SilentWake`, `TestWake`)
  yazıldı.
- **Haberin kaynağı alacritty'nin `Wakeup`'ı** ve `Session::request_frame`
  o kolu artık kullanmıyor (`Adapter::wake_frame`): seçim ve kaydırmanın
  kare isteği her tekerlek çentiğinde sayımı baştan başlatırdı. Aynı kol
  çıktının neslini (`AdapterInner::ledger`) `Term` kilidi altında artırıyor —
  kaymanın "arada çıktı var mı" sorusu. Resize da haber veriyor.
- **Kaymanın kuralı `search::ledger_shift`'te** ve planınkinden bir adım
  sıkı: `display_offset` farkı yalnız doymuş defterde ve kullanıcının kendi
  kaydırması düşülerek (`Session::scroll_user`: `scroll_locked`'ın altı
  çağrısı ve kesirli yol oradan geçiyor) — yoksa tekerlekle birlikte gelen
  çıktı eşleşmeyi kullanıcının kaydırması kadar yanlış satıra taşırdı.
  Boyut, alternatif ekran geçişi ve silinen geçmiş de kayıp. Kayıpta
  geçerli eşleşme **hemen** düşüyor (vurgu yanlış satırda kalmasın) ve son
  geçişin sonunda en yakına dönüyor — tek kare isteği o.
- **Etiket:** "3 of 17", sıra bilinmiyorsa "17 matches", sürerken sonda "…";
  sayım etiketi 84'ten 96 pt'ye genişledi ("999 of 9999…").
- `index_chunk` parça boyunu argüman alıyor: sınama dikişi 1–11 satırlık
  parçalarla görüyor, üretim `CHUNK_LINES` (500, ölçülmedi).
- **Set kapısının `/code-review`'u** on bulgu verdi; yedisi düzeltildi:
  ⏎'nin istediği ek geçiş rapordan sonra kuruluyordu (etiket sırasız
  kalıyor, bekleyen bayrak sonraki haberleri yutuyordu); sonradan gelen
  eşleşmede etiket "No matches" diyordu (`found` artık dizinin sayısını da
  soruyor); desensiz sorgunun bayat başlangıcı; `scrollback` küçülünce haber
  yoktu; senkron güncellemenin kilit dışı `Wakeup`'ı doymuş+kaydırılmış
  kaymayı kaybettirebiliyordu (ofset kuralı artık nesle bakmıyor);
  ⌘E'nin menü doğrulaması bütün seçimi dizgiye çeviriyordu
  (`Session::has_selection`); `view.rs`'te kayan doc yorumu. CLAUDE.md
  paragrafları kısaltıldı. Waive'ler aşağıda.
- **Waive — yazarken ve ⏎'de sınırsız `search_next`** (phase-4'ün
  `nearest_match`/`next_eligible`'ı): eşleşmesi olmayan bir önek on bin
  satırda her tuşta defterin tamamını `Term` kilidi altında tarıyor.
  Sınırlamak "yazarken geçerli eşleşme yukarıdaki ilk eşleşme ve pencere ona
  gider" davranışını (Karar 3, 4) değiştiren bir ürün kararı; alacritty'nin
  kendi aramasının yolu da bu. Süresi ölçülmedi, `CHUNK_LINES` iddiasının
  yanında bekliyor.
- **Waive — `SKIP_LIMIT` (64) ile dizin ayrışabilir**: yalnız boşluktan
  oluşan eşleşmeleri de bulan bir desende (`\s+|x`) gezinme 64 dışlanan
  eşleşmeden sonra vazgeçiyor, dizin saymaya devam ediyor. Patolojik desen;
  sonuç sessiz bir yanlış değil "eşleşmeye gidilemiyor".
