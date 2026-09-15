# Phase 4 — Canlı izleme

## Özet

Ayar ve tema dosyalarını izle; kayıt anında yeniden oku, öncekiyle
karşılaştır ve yalnız değişeni uygula.

_Requirements: R5, R1.1, R1.2, R10_

## Değişiklikler

- **`crates/bt-shell/src/watch.rs` (yeni)** — `dispatch2` vnode kaynakları:
  - **Dizin kaynakları** (`WRITE`): `~/.config/bateri/` ve varsa `themes/` —
    dosya doğumu, silinmesi, yeniden adlandırılması (editörün "yeni dosya
    yaz + rename" kaydı).
  - **Dosya kaynakları** (`WRITE | EXTEND | DELETE | RENAME`):
    `settings.toml` ve **etkin kullanıcı teması** (gömülü tema seçiliyse
    yok). Yerinde yazmayı (`>>`, nano) ve symlink hedefindeki kaydı bunlar
    görür: dosya `std` ile salt okunur açılır, açılış symlink'i izler.
  - **Her olayda hepsi yeniden kurulur:** tüm kaynaklar iptal → yeniden oku →
    uygula → kaynakları yeniden kur. Birleştirme ayrı mekanizma değil (fark
    yoksa uygulanacak şey yok); taşınmış/yeniden yaratılmış dizinin bayat
    tanıtıcısı da böyle düşer.
  - Dizin yoksa kaynak kurulmaz; yeniden kurmayı dışarıdan tetikleyen bir
    giriş açık kalır (phase-6'nın "Ayarlar…"ı kullanır).
  - Olay işleyicisi **fonksiyon işaretçili** varyantla kurulur: `bt-shell`'e
    `block2` kenarı eklenmez. İşleyicinin kuyruğu parametre: üretimde ana
    kuyruk, sınamada özel seri kuyruk.
  - `bt-shell`'in `libc` kullanımı bekçinin dışına genişlemez.
- **`crates/bt-core/src/settings.rs`** — saf fark: (önceki, yeni) → neyin
  değiştiği (tema seçimi, `light_theme`/`dark_theme`, `scrollback`, …).
- **`crates/bt-core/src/session.rs`** — terminal seçenekleri canlı:
  alacritty `Config`'ini kuran **tek** fonksiyon hem `spawn`'da hem canlı
  değişimde kullanılır ve `Config`'i **bizim seçeneklerimizin tamamından**
  kurar. `Term::set_options` `Config`'in hepsini değiştiriyor (alacritty
  `term/mod.rs:499-516`): `..Config::default()` örüntüsüyle çağrılsa ilgisiz
  bir değişim geçmişi geri dönülmez kırpardı (`grid/mod.rs:154-158`).
  `Session` güncel seçenekleri tutar; değişimden sonra kare isteği (geçmiş
  küçülünce kaydırma ofseti de değişir). Doc: `set_options` başlık olayını
  `Term` kilidi altında yolluyor, `Adapter`'ın o kolu kilit almamaya devam
  etmeli.
- **`crates/bt-shell/src/app.rs`** — uygulayıcı (ana thread):
  - Ayrıştırılamayan dosya → yalnız ayar yuvası dolar, **hiçbir şey
    uygulanmaz**.
  - Değilse fark: tema seçimi ya da etkin tema dosyası değişti → ad çözümü →
    `set_theme` (görünüm uygulayıcısıyla aynı yol); `scrollback` →
    seçenekler. Yuvalar kendi kaynaklarına göre dolar/boşalır.
  - **Hata kuralı görünüm değişiminden farklı (R3.4):** kullanılamayan tema
    (yarım kaydedilmiş tema dosyası) takas edilmez, ekrandaki tema kalır ve
    tema yuvası dolar. phase-3'ün `AppDelegate::choose_theme`'i gömülü yedeğe
    düşüyor (`ThemeLoaded::or_embedded`) — bu yolda o yedek düzenleme
    sırasında pencereyi gömülü temaya çakar; `Failed`'i ayrı ele al.
  - Açılışta izleme kurulur; **hermetik dal** süreli koşuda kurmaz.
- **`docs/AYARLAR.md`** — "yeniden açılışta" cümlesi gider: kayıt anında
  uygulanır; yarım kayıtta ekran olduğu gibi kalır; dizini ilk kez kabuktan
  elle oluşturan kullanıcı için "Ayarlar…" ya da yeniden açılış (phase-6
  gelene kadar yalnız yeniden açılış).

## Kabul

Geçici dizinde, özel kuyrukla, zaman aşımlı bekleyişle (döngüyle yoklama
yok) izleme sınamaları:

- Yerinde ekleme (`append`) olay üretir.
- Yeni dosya yazıp üstüne yeniden adlandırma olay üretir; ikinci kayıt da
  (yeniden kurulumdan sonra) olay üretir.
- Symlink'li `settings.toml`: hedef dosyaya yazmak olay üretir.
- Dizin silinip yeniden yaratılınca kaynaklar yeni dizine kurulur.
- Olmayan dizin: kaynak yok, hata yok.

Ayrıca:

- Fark fonksiyonu: değişmeyen dosya boş fark; yalnız `scrollback` değişimi.
- `Config` kurucusu: bir seçenek değişince öteki korunur (bugün
  `scrollback`; phase-8 `osc52`'yi ekleyince aynı sınama genişler).
- `make test-yaris` yeşil (`set_options` `Term` kilidini ana thread'den alıyor).
- `make duman` jetonları değişmez.
- Göz: tema adını editörde değiştirip kaydetmek pencereyi anında değiştirir;
  aktif kullanıcı temasının bir rengini değiştirmek de; tırnağı eksik bir
  kayıt ekranı bozmaz, alt başlık hatayı söyler, düzeltince kaybolur.

## Yayın Etkisi

- **ayar şeması** — davranış: değişiklik kayıt anında uygulanır.
- Göz kontrolü: vim açıkken tema değişimi.

## Checklist

- [ ] `watch.rs`: dizin + dosya kaynakları, her olayda yeniden kurulum
- [ ] Saf fark fonksiyonu
- [ ] `Config`'i tamamından kuran tek fonksiyon, canlı seçenek değişimi,
      kare isteği
- [ ] Uygulayıcı: ayrıştırılamayan dosyada uygulama yok; hermetik dal
- [ ] Test: dört izleme senaryosu + olmayan dizin, fark, `Config` koruması
- [ ] `docs/AYARLAR.md`
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum)
- [ ] Yayın etkisi yazıldı
