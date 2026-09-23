# Yumuşak kaydırma — Bağlam

## Mevcut Durum

Geçmişte kaydırma üç katmandan geçiyor ve üçünde de birim **tam satır**:

- **`bt-shell`** — `BateriView::scrollWheel:` (`crates/bt-shell/src/view.rs`)
  deltayı `wheel_lines` ile tam satıra kesiyor; birim trackpad'de
  (`hasPreciseScrollingDeltas`) hücre boyu (nokta), klasik tekerlekte 1.
  Satıra dönmeyen artık `ViewIvars::scroll_carry`'de bir sonraki olaya
  taşınıyor ve **ekrana hiç yansımıyor**: yavaş bir trackpad jestinde ekran
  hücre boyu kadar birikmeyi bekleyip bir satır zıplıyor. Jestin başında
  (`NSEventPhase::Began`), `Wheel::Ignored`'da ve uçta sıfırlanıyor.
  `momentumPhase` hiç okunmuyor — momentum olayları sıradan delta olarak
  geliyor ve aynı kesmeden geçiyor.
- **`bt-core`** — `Session::scroll_wheel` rotayı `Term` kilidi altında
  seçiyor (`input::wheel_route`: kaydırma / ok / rapor / yoksay); kaydırma
  kolu `scroll_locked`'a (`session.rs`) iniyor. `scroll_locked` doldurma
  bandını **sanal kaydırma** sayıyor (`band` terimi, `1..=band` aralığı hiç
  gösterilmiyor; gerekçesi fonksiyonun gövdesinde ve 017'nin setinde) ve
  alacritty'nin `display_offset`'ini tam satırla oynatıyor. Dönen
  `Wheel::Scrolled(n)` **ofset farkı**, görsel hareket değil. Ok ve rapor
  kolları (`WheelRoute::Arrows`, `Report`) tam satır sayısını tekrar sayısı
  olarak kullanıyor.
- **`bt-gpu`** — `Motion::sync` (`crates/bt-gpu/src/motion.rs`) ofsetin
  değiştiği kareyi "tekerlek" sayıp ötelemeyi (`Slide`) **snap**'liyor
  (`docs/AYARLAR.md` → `[motion]`: "Izgaranın başka sebeple yer değiştirmesi
  de kaymaz"). Yeni çıktının kayması ise zaten süzülüyor: büyüyen içerik
  (011) ve dolu ızgarada geçmişe kayan satırlar (`Motion::scroll_in`, 017
  sonrası); tepede açılan şeridi doldurma bandının geçici uzantısı kapatıyor
  (`Session::set_grid_top` → `slide_fill_rows`), ama yalnız **dibe yaslı**
  pencerede.

Kesirli çizimin yarısı hazır: öteleme zaten satır cinsinden `f32`
(`Slide::pos`), `Frame::set_origin_rows` onu aygıt pikseline yuvarlıyor,
fare eşlemesi **encode edilen** ötelemeyi `bt_gpu::Origin`'den okuyor
(`point_to_cell`) ve doldurma bandı ızgaranın üstündeki satırları kendi
`setViewport`'unda (`Frame::fill_origin_px`) çiziyor. Eksik olan: kaydırma
konumunun kesirli bir **kaynağı**, kaydırılmış pencerede ızgaranın üstündeki
satırı veren bir yol (bugün bant `display_offset == 0` dışında hiç koşmuyor;
`frame()`'in bant döngüsü "ofset terimi yok" diyor) ve tekerleği süzen bir
animatör.

## Motivasyon

Kullanıcı isteği (2026-09-23): "ızgarada smooth scroll istiyorum", ölçüt
gözle kontrolde pürüzsüz his. Bugün trackpad'le yavaş kaydırmada ekran
parmağı izlemiyor, satır satır zıplıyor; momentumlu fırlatmanın sonu da
aynı basamaklarla yavaşlıyor. Klasik tekerleğin her çentiği üç satırlık bir
sıçrama.

Referans ürün bunu varsayılan olarak yapıyor ve kapatılabilir tutuyor
(`docs/ARASTIRMA.md` → ayar envanteri: "Smooth scrolling — disable for Mos or
external scrollers", `scroll.smooth`; açık sorunlar: #30, dış kaydırıcılarla
çakışma → ayar eklendi). Mos gibi araçlar tekerleği kendileri yumuşatıp
sisteme küçük deltalar yağdırıyor; iki yumuşatma üst üste binince his
bozuluyor — ayarın sebebi bu.

Yol haritasında 008'in satırı bu işi "hareket altyapısının ikinci tüketicisi"
diye bekletiyordu; o tüketicinin yarısı (çıktı gelince kayma) 011'de geldi,
bu set kalan yarısı (`docs/YOL-HARITASI.md` → 027).

Kapsam dışı ve bugünkü hâliyle kalan: alternatif ekran (tekerlek oka
dönüşüyor) ve fare kipi (tekerlek uygulamaya rapor olarak gidiyor) — iki
yolda da uygulama tam satır istiyor.
