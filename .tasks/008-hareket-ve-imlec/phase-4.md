# Phase 4 — Üç stil ve `[motion] cursor_motion`

## Özet

Hareket stili ayar dosyasından seçilir: `snap`, `ease`, `spring` (varsayılan
`spring`). Kayıt anında uygulanır, süreli koşu okumaz.

_Requirements: R5 (cursor_motion yarısı), R3.1_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[motion]` bölümü tanınır ve
  `cursor_motion` üç değerli bir enum olarak ayrıştırılır (`Osc52` emsali:
  dizgi → enum, kabul edilmeyen değer kendi anahtarını değiştirmez ve bir
  `Diagnostic` bırakır). `osc52`'nin "kabul edilmeyen değer kapalıya düşer"
  istisnası buraya **geçmez**: yanlış tahminin bedeli görünür bir animasyon,
  sessiz bir pano sızıntısı değil. `Settings`'e alan, `Default`'a `spring`,
  `changes`'e fark. Modül başlığındaki "bilinmeyen anahtar örneği: `[motion]`"
  cümlesi artık yalan — başka bir ad verilir.
- **`crates/bt-gpu/src/motion.rs`** — üç stil: `snap` (anında; animasyon hiç
  başlamaz, yani hareket karesi de doğmaz), `ease` (sabit süre, taşma yok),
  `spring` (phase-3'ün fiziği). Süreler ve yay katsayıları **seçilmiş**
  sayılardır, ölçülmüş değil; tek yerde, doc'larıyla dururlar.
- **`crates/bt-gpu/src/link.rs`** — `DisplayLink` stili dışarıdan alır
  (`Renderer::set_font` emsali: `bt-shell` çözülmüş değeri verir). Stil
  değişimi uçuştaki bir animasyonu ışınlamaz: `snap`'e geçiş onu yerleştirir.
- **`crates/bt-shell/src/app.rs`** — açılışta `Settings`'ten okunur;
  `reload_settings` farkta uygular (temanın ve fontun izlediği yol). Süreli
  koşu `Inputs::Hermetic` olduğu için ayarı **görmez**; bu, `timed_run_does_
  not_see_the_user` sınamasının kapsamına girer.
- **`docs/AYARLAR.md`** — yeni `### [motion]` bölümü: anahtar, değerler,
  varsayılan, hata davranışı. **`CLAUDE.md`** → ayarlar maddesine anahtarın
  adı girer.

## Kabul

- `settings.toml`'a `cursor_motion = "ease"` yazıp kaydetmek açık pencerede
  stili değiştirir; `"snap"` hareketi kapatır (hareket karesi doğmaz,
  `hareket=0`).
- Tanınmayan değer (`cursor_motion = "sprong"`) yalnız o anahtarı
  varsayılanda bırakır, pencere alt başlığında tanı gösterir, dosyayı bozmaz;
  diğer anahtarlar uygulanır.
- `[motion]` bölümü olmayan dosya `spring` ile çalışır; bilinmeyen anahtar
  (`[motion] keypress = "pop"`) sessizce yoksayılır.
- `make duman` etkilenmez: hermetik koşu dosyayı okumuyor.

## Yayın Etkisi

**ayar şeması** — yeni anahtar `[motion] cursor_motion`, varsayılan `spring`;
eski anahtar yok, silinen anahtar yok, bilinmeyen anahtar korunur.
`docs/AYARLAR.md` aynı commit'te. shader yok · terminfo yok · tema yok · shell
entegrasyonu yok · app bundle yok · yeni bağımlılık yok.

Davranış değişikliği kullanıcıya görünür: `[motion]` yazılı bir dosya bugüne
kadar sessizce yoksayılıyordu, artık uygulanır (`plan.md` → Göç).

## Checklist

- [x] `settings.rs`: `[motion] cursor_motion`, enum, varsayılan, `changes`,
      başlık yorumunun düzeltilmesi
- [x] `motion.rs`: üç stil, seçilmiş sayıların doc'ları
- [x] `link.rs`: stilin dışarıdan gelmesi, uçuştaki animasyonun akıbeti
- [x] `app.rs`: açılış + canlı uygulama, hermetiklik
- [x] `docs/AYARLAR.md` + `CLAUDE.md`
- [x] Test: üç değerin ayrıştırılması, tanınmayan değerin tanısı, hermetik
      koşunun ayarı görmemesi
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

Sapmalar; planın söylemediği ya da başka türlü öngördüğü yerler.

- **`ease`'in durma koşulu eşik değil saat.** İlk hâli yayın konum eşiğini
  paylaşıyordu ve stilin adı o zaman yalan söylüyordu: kübik yavaşlamada kalan
  mesafe eşiğin altına kısa sıçramada erken, uzun sıçramada geç iniyor, yani
  "sabit süre" sessizce mesafeye bağlanırdı (1 hücrede ~130 ms, 200 hücrede
  ~172 ms). `State::settled` stili alıyor ve `ease` kolunda `EASE_DURATION`'a
  bakıyor.
- **O kol snap hâllerini kapsamak zorundaydı ve bunu sınama yakaladı.** Yalnız
  saate bakan kural, anında oturan imleci (ilk kare, görünürlük dönüşü,
  geometri, kaydırma) `EASE_DURATION` boyunca "yerleşmemiş" sayıyordu — link
  hiçbir şeyi değiştirmeyen 22 kare çizerdi, üstelik her `sync`'te yeniden.
  İkinci koşul `from == target`: gidecek yol yoksa kayma da yok.
- **`State`'e `from` alanı girdi.** Yay hızı taşıdığı için geçmişe ihtiyaç
  duymuyor, `ease` ise konumu `from → target` arasında yeniden hesaplıyor.
  Bedeli uçuşta hedef **ve stil** değişiminde bu çıkışın tazelenmesi; yoksa
  imleç eski başlangıcına geri sıçrardı.
- **Stil değişimi kare istiyor ve bu planın söylemediği bir şeydi.**
  `set_style` "uçuştaki kayma bitirildi mi" diye `bool` dönüyor
  (`Renderer::set_font` emsali) ve `snap`'e geçişte `DisplayLink` bir kare
  istiyor. Sebep link'in "hasar yok" dalının şekli: orada yerleşmiş bir
  animasyon **hiç çizmeden** uyuyor, yani bitirilen kaymanın yeni konumu
  ekrana ancak istenen bir kareyle düşer — istenmeseydi imleç ara hücrede
  asılı kalır ve onu yerine koyan şey alakasız bir shell çıktısı olurdu.
- **Stil değişiminde "uçuşta mı" sorusu eski stille sorulmak zorunda.** İlk
  hâli stili önce yazıyordu ve durma koşulu stile bağlı olduğu için 180 ms'den
  uzun uçmuş bir yay `ease`'in saatine göre "yerleşmiş" görünüyordu: devir
  atlanıyor, sıradaki `advance` `t = 1` ile imleci hedefe atıyor ve link o
  kareyi yerleşmiş sayıp **çizmeden** uyuyordu — yani `snap`'te giderilen
  kusurun ta kendisi, başka bir yoldan. Uzun sıçramanın olağan hâli (10 hücre
  ~310 ms). Mutasyonla doğrulandı: sıra geri alınınca sınama kırmızı düşüyor.
- **Açılış çağrısı `load_settings`'e giremedi:** link o an henüz yok
  (`start_session` oturumdan sonra kuruyor). `set_font`'un yeri orası, stilin
  yeri link'in kurulduğu satırın hemen altı.
- **Phase-3'ün açık cümlesi kapandı.** `Counters::motion`'ın "gizli bağ"
  doc'u artık tahmin değil: hermetik koşunun stili
  `Settings::default().cursor_motion`, yani varsayılanların tek sahibinden
  geliyor. `Motion`'ın `style` alanı da bu yüzden kendi `Spring` literalini
  taşımıyor, `Default`'u türetiyor.
- **`EASE_DURATION < TIME_CEILING` bir `const` assert.** `ease`'in durma
  koşulu kendi saati olduğu için süre tavanının kemeri ona uygulanmıyor; iki
  sayının sırası bu yüzden bir yorum cümlesi değil bir şart.
- **Duman koşusu (kullanıcının oturumu, debug):** `kare=30 hucre=8 glif=6
  kural=15 istek=4 icerik=3 hareket=27 sessiz=1748.28ms kapanis=clean`.
  Phase-3'ün gözlemiyle tutarlı (`kare=26 hareket=23 sessiz=1745.97ms`);
  hermetik koşu `[motion]`'ı okumadığı için sayaçlar ayarın eklenmesinden
  etkilenmedi.
- **Kapı ajanın oturumunda koşamıyor ve bu bir kusur değil.** Orada koşu
  `MotionUnsettled` ile kırmızı düşüyor — **phase-3'ün commit'inde de**
  (`git stash` ile denendi): pencere compositor tarafından sürülmüyor, display
  link 3 saniyede yalnız 4 kez ateşliyor ve animasyon bir hareket karesinden
  sonra ilerleyemiyor. Kapının görünür pencere isteyen üçüncü koşulu
  (`make test-yaris`'in TSan'ı gibi) bilinen bir sınır; kapı kullanıcının
  oturumunda yeşil koştu.
