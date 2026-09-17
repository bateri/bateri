# Phase 4 — Bastırma: giriş satırı ızgaradan çıkıyor

## Özet

Safha `Input` iken giriş satırının hücreleri ızgarada çizilmesin; phase-3'ün
bıraktığı çift görüntü kapansın.

_Requirements: R3.1, R3.2, R3.3_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `frame()` döngüsünde
  `prompt_row..=cursor_row` aralığının hücreleri **glyph listesine girmez**.
  - **Aralık hesaplanabilir:** sıfır genişlikli `PS1` (phase-5) inmeden önce bu
    phase'te aralık prompt satırından imleç satırına; `prompt_row` OSC 133
    `B`'nin satırı. phase-5 sıfır genişliği getirince aralık **bütün
    sütunlar** olur.
  - **Safha kopyası `Term` kilidinden ÖNCE alınır** — `Theme` ile aynı örüntü.
    Safha `shell` yaprak kilidinde, sink `Term` kilidi altında ve ikisi hiçbir
    yerde iç içe girmiyor; kilit sırası bozulamaz.
  - **Çıpa taraması bastırmadan etkilenmez:** `cell.hyperlink()` okuması glyph
    üretiminden **bağımsız** koşar, yani blok şeridi yerinde kalır. Naif bir
    bastırma (satırı tümden atlamak) çıpayı da öldürürdü; bekçisi yazılır.
  - **Doluluk sayısı (`content_rows`) bastırılan satırları saymaz** — yoksa
    011'in tabana yapışması boş bir satır için yer ayırırdı.
- **`crates/bt-core/src/shell.rs`** — **özel kip tetiği.** ZLE'nin
  `bck-i-search`, `menu-select`, `zle -M` ve `CORRECT`'in `[nyae]`'i beş
  değişkenin **dışında** çiziyor; o anlarda bastırma **bırakılır** ve ızgara
  devralır.
  - Tetiğin sinyali bu phase'in asıl tasarım işi: ZLE tarafında kip ayırt
    edilebiliyor ve kanal zaten açık (phase-1), yani beşinci bir alan olarak
    taşınabilir. Alternatifi terminal tarafında sezgi olurdu — **seçilmez**,
    tahmin bu deponun yasakladığı sınıf.

## Kabul

- Yazarken metin **yalnız dock'ta** görünüyor; ızgarada giriş satırı boş.
- **Komut şeridi yerinde kalıyor** — bastırma çıpayı öldürmedi.
- Tab'a basınca tamamlama listesi ızgarada beliriyor ve o sırada bastırma
  bırakılmış, yani kullanıcı ZLE'nin kendi arayüzünü eksiksiz görüyor.
- `bck-i-search` ve `CORRECT`'in `[nyae]`'i çalışıyor.
- İçerik tabana yaslanması bozulmuyor: bastırılan satır doluluğa sayılmıyor.
- Enter'dan sonra dock boşalıyor ve komut ızgarada normal bir satır olarak
  duruyor (bastırma yalnız `Input` safhasında).

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırı bastırmayı ve safha kopyasının kilit
  öncesi alındığını söyler.
- **Bilinen sınır:** `zle -I` ile basılan bir iş bildirimi çıpa satırını bir
  satır kaydırabiliyor (011 Karar 10a'nın kayıtlı sınırı). Bastırma aralığı
  çıpadan türediği için belirti burada **görünür** hâle geliyor: bir satır
  fazla ya da eksik bastırılabilir. Adıyla yazılır.
- **`psvar[9]` düşerse** bedeli artık şerit değil: prompt devredilmişken
  (phase-5) kimlik kaybı bastırma aralığını da belirsizleştirir. Borç
  `docs/YOL-HARITASI.md`'de, bu phase onu **büyütüyor** ve notu güncellenir.
- shader, ayar şeması, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.
- Ölçüm bekleyen iddia: yok.

## Checklist

- [ ] `prompt_row..=cursor_row` glyph listesine girmiyor
- [ ] Safha kopyası `Term` kilidinden önce alınıyor (`Theme` örüntüsü)
- [ ] Çıpa taraması bastırmadan bağımsız; **bekçisi var**
- [ ] `content_rows` bastırılan satırları saymıyor
- [ ] Özel kip tetiği kanaldan geliyor (terminal tarafında sezgi **yok**)
- [ ] Test: bastırma açıkken şerit yerinde
- [ ] Test: özel kipte bastırma bırakılıyor
- [ ] Gerçek zsh oturumunda gözle: Tab, Ctrl-R, `CORRECT`
- [ ] Doğrulama geçti (`make hepsi` + `make kur`)
- [ ] Yayın etkisi yazıldı
