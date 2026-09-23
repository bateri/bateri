# Yumuşak kaydırma

## Hedef

Geçmişte kaydırmak pürüzsüz olsun: trackpad'le kaydırırken ekran parmağı
piksel piksel izler ve fırlatmanın momentumu da öyle yavaşlar; klasik
tekerleğin çentiği sıçramaz, kısa bir süzülmeyle gider. Jest bitince ekran
en yakın satıra oturur. `[motion] smooth_scroll = "off"`, Hareketi Azalt ve
`cursor_motion = "snap"` bugünkü satır adımını geri getirir. Kararlar ve
gerekçeleri `discussion.md` → `## Karar`.

## Gereksinimler

- **R1** — Kesirli kaydırma konumu (`bt-core`).
  - **R1.1** — `Session`'ın tek yeni kaydırma durumu `[0, 1)` aralığında bir
    kesir; tam satırın tek yetkilisi `scroll_locked` (bant eşlemesi
    değişmeden). Delta çağrısı kesri biriktirir, 1'i aşan kısmı
    `scroll_locked`'a indirir.
  - **R1.2** — Uçlarda kesir sıfır: dipte negatif, geçmişin tepesinde pozitif
    kesir kalmaz; hiçbir şeyi değiştirmeyen delta kare istemez.
  - **R1.3** — Dış yazıcılar ezilmez: geçmişteyken gelen çıktı ve resize
    kesre dokunmaz ve tam satırı geri almaz; girdide dibe dönüş ve
    Shift+PgUp kesri sıfırlar ve bir **nesil** artırır.
  - **R1.4** — Kesir ve kaydırma nesli `frame()` sınırından geçer; kesir
    sıfırdan büyükken ızgaranın hemen üstündeki satır (bandlı dipte bandın
    üstündeki satır) doldurma bandının kanalından verilir — tek kapısı
    satırın defterde olması; sayısı `Cursor::fill`'e ve `fill_shown`'a
    karışmaz.
  - **R1.5** — Tekerlek olayı kesirli ve tam satır hâliyle birlikte iner; rota
    önce seçilir, kesir yalnız kaydırma kolunda, ok ve rapor tam satırla.
    Çentik süzülmesi isteği `Session`'da birikir ve kare yolu alır.
- **R2** — Çizim ve süzülme (`bt-gpu`).
  - **R2.1** — Kesir orijine eklenir (ızgara aşağı), aygıt pikseline
    yuvarlanır ve `Origin`'e **encode edilen** değer olarak yayınlanır — fare
    eşlemesi ve seçim onu okur. Dock ötelemeden muaf kalır.
  - **R2.2** — Tepe satırı doldurma bandının viewport'unda, bandın üstünde
    çizilir; `filled` biti ve blok işaretleri bugünkü anlamında.
  - **R2.3** — Çentik süzülmesi `Motion`'da ikinci bir `Slide` örneği (kalan
    satır); stil `cursor_motion`'ınki, yeni sabit yok. Kare başına pay
    `frame()`'in argümanı olarak teslim edilir ve uyandırmaz; uçuştayken kare
    içerik karesidir (link'in yerel kararı, `Waker::wake` değil).
  - **R2.4** — `settled()` süzülmeyi kapsar; `snap`'e geçiş, Hareketi Azalt,
    örtülme ve nesil değişimi süzülmeyi bitirir; `Motion::sync`'in ofset
    snap'i ona dokunmaz. Boşta sıfır kare korunur.
- **R3** — Giriş ve ayar (`bt-shell`, `bt-core::settings`).
  - **R3.1** — Olayın niyeti saf bir fonksiyonda sınıflanır: hassas delta
    doğrudan, hassas olmayan çentik süzülme isteği, jest ya da momentum
    sonu yerleşme niyeti (payı `Session` kilit altında hesaplar:
    `round(kesir) − kesir`), momentum başı yerleşmeyi bitirir.
  - **R3.2** — `[motion] smooth_scroll = "on" | "off"`, varsayılan `"on"`,
    kayıt anında; şablonda, `docs/AYARLAR.md`'de, round-trip sınamasında.
  - **R3.3** — Ayar, Hareketi Azalt ve `cursor_motion = "snap"` `bt-shell`'de
    tek `bool`'a iner; `false` kolu bugünkü `wheel_lines` + artık yolu, bayt
    bayt.
  - **R3.4** — `CLAUDE.md` ve `docs/AYARLAR.md` → `[motion]`'ın tekerleği
    snap'leyen cümleleri yeni davranışa çevrilir.

## Yaklaşım

1. `bt-core`: kesir, delta çağrısı ve nesil; tam satır `scroll_locked`'tan.
   Tekerlek API'si kesirli + tam satır alır; `bt-shell` bu phase'de yalnız tam
   satırı doldurur, yani davranış bugünküyle aynı. `frame()` kesri, nesli ve
   tepe satırını verir (kesir hep sıfır olduğu için tepe satırı doğmaz).
2. `bt-gpu`: orijin bileşimi, tepe satırının çizimi, süzülme animatörü ve
   kare yolu. Tetikleyen `bt-shell` henüz yok; sınamalar hermetik.
3. `bt-shell`: niyet sınıflaması, momentum, ayar ve belge — özelliği açan
   phase.

## Kapsam Dışı

- Alternatif ekran (tekerlek oka dönüşüyor) ve fare kipi (tekerlek rapor):
  tam satır, bugünkü gibi.
- Yatay kaydırma.
- Kendi momentum yavaşlatıcımız: momentum AppKit'in `momentumPhase`
  olaylarından geliyor.
- Kesirli konumda tepedeki yarım satırın seçilebilmesi: bant satırı gibi
  reddediliyor ve dinlenme hâlinde kesir yok (Karar 3).
- Doldurma bandının seçilemeyişi (yol haritasındaki borç) — değişmiyor.
- Süre sayacının banda girmesi — değişmiyor.
- **Bilinen sınır:** basılı sürüklemede seçimin ucu (`follow_pointer`) olay
  anındaki ofseti okuyor; çentik süzülürken uç uçuşun bir kare gerisinde
  kalabilir.
- **Bilinen sınır:** süzülmenin ortasında alternatif ekrana geçilirse
  `scroll_locked` `None` döner, kesir sıfır sayılır ve süzülme biter; tepe
  satırı doğmaz.

## Akış

```
NSEvent (scrollWheel:)
  └─ bt-shell: niyet (saf) + smooth bool ──► Session::scroll_wheel(kesirli, tam satır, niyet)
                                               │ Term kilidi: rota
                                               ├─ Arrows/Report ─► tam satır (bugünkü)
                                               └─ Scroll
                                                   ├─ smooth=false ─► scroll_locked(tam satır)
                                                   ├─ doğrudan ────► delta(kesir) ─► scroll_locked(⌊…⌋)
                                                   └─ çentik/yerleşme ─► süzülme isteği biriktir, wake
link, kare başı:
  istek al → Motion (kalan satır Slide) advance → pay = Δ
  → frame(pay): pay kilit altında uygulanır (uyandırmaz) → Cursor { kesir, nesil, tepe satırı (bant kanalı) }
  → orijin = öteleme + kesir → encode (ızgara, bant+tepe satırı, dock) → Origin yayını
  süzülme uçuştaysa → sonraki kare de içerik karesi; yerleşince uyku
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| phase-3 | |
| kapı | |
