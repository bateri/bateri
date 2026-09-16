# Phase 3 — Sol kenar payı, tek sabitten

## Özet

Şeridin oturacağı pay pencerenin solundan ayrılır; genişlik **tek sabit**
olarak yaşar ve `cols` hesabı, çizim orijini ile fare eşlemesi onu aynı
yerden okur.

_Requirements: R5, R5.1, R5.2_

## Değişiklikler

- **`crates/bt-gpu`** — gutter genişliği burada doğar ve hücre ölçüsünün
  yanında yayımlanır (`Renderer::cell_metrics` emsali: ölçek uygulanmış
  piksel değeri tek kaynaktan çıkar). Katman yönü bunu zorunlu kılıyor —
  `bt-shell` `bt-gpu`'yu görüyor, tersi değil.
- **`crates/bt-shell/src/app.rs`** — grid ölçüsü payı düşerek hesaplanır.
  Bu `cols`'u bir azaltabilir, yani PTY `winsize`'ını da değiştirir;
  sabit ölçülü sınamalar (`split_into_grid`) mekanik olarak düşer ve aynı
  commit'te düzelir.
- **`crates/bt-shell/src/view.rs`** — `point_to_cell` payı çıkararak böler.
  Doc'undaki "pencere kenar payı yok" cümlesi artık yanlış, düzelir.
  Payın içine düşen tıklama ilk sütuna kırpılır; seçim payda başlamaz.
- **`crates/bt-gpu/src/frame.rs`** — hücrelerin piksel konumu paydan sonra
  başlar. Orijin ile `cols` **aynı** sabitten beslenmezse belirti "fare bir
  sütun kayıyor" olur ve sessiz değildir ama geç fark edilir; üç tüketicinin
  tek kaynağı bu phase'in asıl işi.

## Kabul

- Pay **her zaman** ayrılır ve oturum ortasında değişmez: entegrasyonsuz
  oturumda (bash/fish, `shell.integration = false`, SSH) boş kalır. Kabul
  edilen bedel — alternatifi ilk prompt'ta bir SIGWINCH ve üç tüketicinin
  aynı anda güncellenmesiydi.
- Alternatif ekranda da pay ayrılmış kalır; `cols` vim açılışında oynamaz.
- Tıklama ve sürükleme doğru sütunu seçer: metnin ilk karakterine tıklamak
  ilk sütunu verir, payın içine tıklamak da.
- `make duman` yeşil: `hucre=8 glif=6 kural=15` **oynamaz** (şerit ızgaranın
  dışında ve `smoke_shell` ilk satırda sekiz hücre basıyor), `icerik` ve
  `sessiz` sınırları yerinde kalır.
- Pencere paydan dar kalırsa mevcut davranış **korunur**, yeni bir alt sınır
  getirilmez: çıkarma `f64`'te yapılır, negatif değer `as u16`'da sıfıra
  doygunlaşır ve sıfır sütunu `Session::resize` zaten yoksayıyor (simge
  durumundaki pencerenin yolu). Çıkarmayı tam sayıda yapan bir uygulama bu
  zinciri **taşmayla** kırardı.

## Yayın Etkisi

- **`make duman` zorunlu** (`proje.md`: pencereyi açan davranış değişti).
  Şerit henüz çizilmediği için jeton sayıları değişmemeli; değişirse pay
  ızgaraya sızmış demektir.
- **shader yok:** `.metal` dosyaları ve `#[repr(C)]` düzenleri bu phase'de
  değişmiyor, `make shader` gerekmiyor.
- Ayar şeması: `command_gutter` bu sette **yok** (Karar 6), yani anahtar
  eklenmiyor ve `docs/AYARLAR.md` değişmiyor.
- Tema biçimi, shell entegrasyonu, terminfo, bundle: değişiklik yok.
- Yeni bağımlılık yok. Ölçüm iddiası yok.

## Checklist

- [ ] Gutter genişliği `bt-gpu`'da tek sabit, ölçekle birlikte yayımlanıyor
- [ ] `cols` hesabı payı düşüyor
- [ ] Çizim orijini paydan sonra başlıyor
- [ ] Fare eşlemesi payı çıkarıyor; `view.rs` doc'u düzeldi
- [ ] Test: `point_to_cell` payın içinde ve ilk sütunda doğru sonuç
- [ ] Test: `split_into_grid` yeni ölçülerle; dar pencerede `cols` alt sınırı
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı
