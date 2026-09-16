# Phase 4 — Şerit çizilir

## Özet

Blok aralıkları temanın durum renkleriyle sol paya çizilir: komut blokları
ilk kez ekranda görünür.

_Requirements: R4, R4.1, R4.2, R4.3_

## Değişiklikler

- **`crates/bt-gpu/src/frame.rs`** — şerit `Frame`'de **kendi listesi**.
  `bg`'ye girmiyor: `push`'taki `debug_assert_eq!(bg.len(), bg_count)` sıra
  sözleşmesi ve `move_cursor`'ın `bg.truncate(bg_count)`'u dokunulmadan
  kalır. `bg`'ye girip sayılmasaydı imleç her kaydığında şerit silinir ve
  **titrerdi**; sayılsaydı `hucre=` jetonunun anlamı kayardı. Hareket karesi
  listeyi **korur** (`move_cursor` yolu): ızgara değişmediği için şerit de
  değişmemeli.
- **`crates/bt-gpu/src/renderer.rs`** — şerit için ayrı draw call, **mevcut
  `cell_bg` pipeline'ı**. `Instance` genel bir piksel dörtgeni
  (`pos`, `size`, `rgba`), yeni shader ya da `#[repr(C)]` düzeni yok.
  Renk lineer geçer — hedef `BGRA8Unorm_sRGB` ve kodlamayı ROP yapıyor;
  ikinci bir gamma düzeltmesi paleti iki kez kodlar.
- **`crates/bt-gpu/src/link.rs`** — `frame()`'in verdiği blok aralıkları
  `Frame`'in şerit listesine aktarılır. **Animasyon yok** (Karar 5): şerit
  anında belirir, `motion` ikinci bir tüketici kazanmaz, `motion_settled()`
  kapısı ve `Mode::Fade`'in "tek yer" kuralı dokunulmadan kalır.

## Kabul

- Başarısız komut kırmızı, başarılı komut sakin bir şerit alır; koşan komut
  `accent`. Renk **sınırdan geliyor**, bu katmanda üretilmiyor.
- Tema değişince şerit **aynı karede** yeni palete geçer: kaynak
  `Session::frame`'in `Term` kilidinden önce aldığı kopya, hücrelerinkiyle
  aynı.
- `make duman` yeşil ve jetonlar **oynamaz**: `smoke_shell` OSC 133
  basmadığı için blok yok, şerit yok. Bu aynı zamanda bu yolun duman
  kapısının **dışında** olduğu anlamına gelir (aşağıda).
- Boşta sıfır kare korunur: şerit hiçbir kare istemiyor, yalnız çizilen
  karede görünüyor. `icerik` ve `sessiz` sınırları yerinde.
- Gerçek bir zsh oturumunda `false` koşmak kırmızı, `true` koşmak sakin
  şerit bırakır; geçmişe kaydırınca şeritler satırlarıyla birlikte gider;
  pencereyi yatay boyutlandırmak onları prompt satırlarında tutar.

## Yayın Etkisi

- **`CLAUDE.md`** — iki cümle eskiyor: "ürün yüzeyi (blok, dock) henüz yok"
  ve "satıra çıpalanması … henüz yok". Aynı commit'te düzelir. (Tema
  rollerinin cümleleri phase-2'de düzeldi.)
- **`docs/YOL-HARITASI.md`** — 010 satırı kapanır; "(+ blok animasyonları)"
  **ertelenmiş borç** olarak "Sete bağlanmamış borçlar"a iner, gerekçesiyle.
- **`make duman` kapının dışında kalıyor:** `smoke_shell` OSC 133 basmadığı
  için şerit yolu jetonlara hiç girmiyor. Bilerek — kapıyı görür kılmak
  reçeteye OSC 133 eklemek, o da `QUIET_FLOOR` ile `IDLE_FRAME_LIMIT`'in
  yeniden türetilmesi demekti (`proje.md`: ayrı commit). Şerit animasyonsuz
  olduğu için kapının asıl koruduğu şey (boşta sıfır kare) bu yoldan
  tehdit altında değil.
- **shader yok:** `.metal` değişmiyor, `#[repr(C)]` düzeni aynı;
  `make shader` gerekmiyor.
- Ayar şeması, shell entegrasyonu, terminfo, bundle: değişiklik yok.
  Yeni bağımlılık yok. Ölçüm iddiası yok.

## Checklist

- [ ] `Frame`'de şerit için ayrı liste; `bg`/`bg_count` ve `move_cursor`
      dokunulmadı
- [ ] Renderer'da ayrı draw call, mevcut `cell_bg` pipeline'ı
- [ ] `link.rs` blok aralıklarını aktarıyor; animasyon eklenmedi
- [ ] Test: `Frame` — şerit listesi `bg_count`'a girmiyor, `move_cursor`
      şeridi silmiyor
- [ ] Test: offscreen çizim — şerit pikselleri doğru renkte (ara ton bir
      renkle; `0.0`/`1.0` sRGB'nin sabit noktaları)
- [ ] `CLAUDE.md` ve `docs/YOL-HARITASI.md` güncellendi
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı
