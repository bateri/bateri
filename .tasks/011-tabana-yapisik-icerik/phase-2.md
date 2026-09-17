# Phase 2 — Yumuşak kayma

## Özet

Yeni satır geldiğinde içerik anında sıçramasın, yumuşak kaysın — imleç dipteki
satırında dururken geçmiş arkasından yukarı aksın.

_Requirements: R2.1, R2.2, R2.3, R2.4, R2.5, R2.6, R2.7_

## Değişiklikler

- **`crates/bt-gpu/src/motion.rs`** — origin `Motion`'ın **içine** girer.
  Dışarıda kalamaz: hasarsız karede tek uyku kararı `motion.settled()`
  (`link.rs:492-505`), yani ayrı bir animatör kayma ortasında link'i uyutur ve
  **içerik donar**.
  - **İmlecin hedefi ekran uzayına taşınır** (`row + origin_rows`) ve imleç
    origin ötelemesinden **muaf** tutulur. Gerekçe: Enter'da grid satırı anında
    r→r+1 olurken origin animasyonla gidiyor; ikisi ayrı uzayda kalırsa imleç
    bir satır aşağı düşüp geri biner. Ekran uzayında hedef **hiç değişmiyor**.
  - **Hareketi Azalt'ta origin snap'ler**, `Mode::Fade` **değil**: fade "konum
    anında hedefte, değişen şey opaklık" demek ve her yeni satırda bütün
    ekranın belirmesi, indirgemeye çalıştığı hareketten beter olurdu.
    "İndirgemenin tek yeri `bt-gpu::motion`" kuralı yerinde kalıyor — **yer**
    aynı, **kip** iki.
  - **`display_offset` oynadıysa origin snap'ler** (`motion.rs:245`'in
    kuralının ikizi): tekerlek parmağı takip eder, 008 Karar 5 ayakta kalır.
    `clear`'dan sonra geçmişte gezinen kullanıcı bunu görüyor.
  - **Durma koşulu** yazılır ve girdisinin **monoton olmadığı** hesaba katılır:
    imleci yukarı taşıyıp alt satırı `\e[K` ile silen bir program
    `content_rows`'u daraltıp genişletebilir.
- **`crates/bt-gpu/src/link.rs`** — hasarsız kolda origin'in **ikinci yazma
  noktası** açılır (`move_cursor`'ın yanı). O kolda `frame()` de `clear` de
  çağrılmıyor, yani origin **korunuyor** ama değişemiyor; animasyonun tanımı
  iki içerik karesi arasında değişmektir. Ayrıca `kayma_frames` sayacı:
  yalnız origin animatörü yerleşmemişken artar.
- **`crates/bt-shell/src/app.rs`** — jeton satırına **`kayma=`** eklenir.
  `hareket` yalnız imleç animatörünün tanığı kalır; iki kaynak ayırt
  edilebilir olur. **Jeton silinmez, eklenir** — okuyan taraf tanımadığını
  atlar. Kapı `hareket > 0` kalır.
- **`docs/AYARLAR.md`** — `[motion]` bölümündeki **"İmlecin kendi hareketi
  kayar; altındaki ızgaranın hareketi kaymaz"** cümlesi ayrılır: tekerlekle
  kaydırma, boyutlandırma ve punto değişimi **kaymıyor** (değişmedi), içerik
  büyümesinin kaldırması **kayıyor** (yeni). `"snap"`ın "hareketi tamamen
  kapatmanın yolu bu" cümlesi **ayakta** — kayma da `cursor_motion`'ı izliyor.
- **`crates/bt-core/src/session.rs`** — `Cursor::display_offset`'in doc'u
  (008 Karar 5) aynı ayrımı yansıtacak biçimde güncellenir.
- **`CLAUDE.md`** — "her animasyonu 90 ms'lik bir **belirmeye** indirir"
  cümlesi artık yalnız imleç için doğru; origin'in indirgemesi snap.

## Kabul

- Ekran dolmadan Enter'a basınca içerik yumuşak kayıyor; **imleç sıçramıyor**
  (ne aşağı düşüyor ne geri biniyor).
- `cursor_motion = "snap"` iken içerik de anında yerine gidiyor.
- Hareketi Azalt açıkken içerik **snap**'liyor, ekran belirmiyor.
- Tekerlekle geçmişte gezinirken içerik kaymıyor (snap).
- Kayma ortasında tıklama doğru hücreyi seçiyor (R2.7 bekçisi).
- `make duman` yeşil: `hareket > 0` (imleç, phase-0'ın reçetesinden),
  `kayma` jetonu basılıyor, `icerik ≤ IDLE_FRAME_LIMIT`,
  `sessiz ≥ QUIET_FLOOR`.
- Kayma yerleştikten sonra **kare istenmiyor** (boşta sıfır kare).

## Yayın Etkisi

- **Ölçüm bekliyor: kayma animasyonunun yerleşme süresi ve `sessiz` bandına
  etkisi.** `QUIET_FLOOR`'un payı dar — `docs/OLCUMLER.md`'de ölçülen en düşük
  sağlıklı `sessiz` 1742,29 ms, türetmenin tabanı 1740 ms. Sayı **uydurulmaz**;
  `/measure` kapatır.
- **Jeton sözleşmesi büyüyor:** `kayma=` eklendi, hiçbir jeton silinmedi.
  Anahtar Türkçe ve donmuş, değer İngilizce.
- **Belge:** `docs/AYARLAR.md` (`[motion]`), `CLAUDE.md`'nin indirgeme cümlesi
  ve `Cursor::display_offset`'in doc'u **aynı commit'te** düzelir — üçü de bu
  phase olmadan doğruydu.
- **Ayar şeması:** **değişmiyor**. Yeni anahtar yok; kayma `cursor_motion`'ı
  izliyor. (`feed_lift` bilinçli olarak **reddedildi** — `discussion.md` →
  Muhakeme 2. tur, kabul 7.)
- shader: `.metal` değişmiyor. terminfo, tema, shell entegrasyonu, app bundle:
  **yok**. Yeni bağımlılık: **yok**.
- **Riskli phase tetiği yok** ve bu bilinçli: `Motion` ana thread'e bağlı bir
  `Cell` (`link.rs` callback'i ana run loop'ta), yani `make test-yaris`'in
  "paylaşılan durum" koşulu tetiklenmiyor; `.metal` ve `Cargo.lock`
  değişmiyor. Hareket saatine ve boşta sıfır kare sözleşmesine dokunan bu
  phase'i **set sonundaki kapı** (`/code-review` + `/audit`) karşılıyor —
  `proje.md`'nin "geri kalan her şey set sonunu bekler" kuralı.

## Checklist

- [ ] Origin `Motion` içinde, `settled()` kapısında
- [ ] İmlecin hedefi ekran uzayında; Enter'da sıçrama yok
- [ ] Hareketi Azalt'ta snap; `display_offset` oynadıysa snap
- [ ] Durma koşulu yazılı, `content_rows`'un monoton olmadığı hesaba katıldı
- [ ] Hasarsız kolda origin'in ikinci yazma noktası
- [ ] `kayma=` jetonu; `hareket` saf imleç tanığı
- [ ] Test: kayma ortasında `point_to_cell` tutarlı (R2.7)
- [ ] Test: kayma yerleştikten sonra kare istenmiyor
- [ ] `docs/AYARLAR.md`, `CLAUDE.md` ve `Cursor::display_offset` doc'u düzeltildi
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı ("ölçüm bekliyor" satırı dahil)
