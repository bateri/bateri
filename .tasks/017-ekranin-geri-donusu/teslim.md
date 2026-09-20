# Ekranın geri dönüşü — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [context.md](context.md) ·
> [discussion.md](discussion.md) · [phase-0](phase-0.md) · [phase-1](phase-1.md)
> · [phase-2](phase-2.md) · [phase-1b](phase-1b.md) · [phase-3](phase-3.md) ·
> [phase-4](phase-4.md) · [phase-5](phase-5.md)

Tab tamamlama listesi kapandığında ekranın tepesinde kalan boşluk artık boş
kalmıyor: geçmişin en yeni satırlarıyla doluyor ve içerik oraya **kayarak**
iniyor — yani listenin ittiği satırlar geldikleri yönün tersinden geri
geliyor. Kullanıcı ekranı **kasten** temizlediyse (Ctrl-L, `clear`) dönüş
yok; ayrımın tek sinyali PTY tarayıcısının yeni CSI kolunun gördüğü
`CSI 2 J`. `content_rows`/`origin` aritmetiği değişmedi — değişen yalnız
orijinin üstünde **ne boyandığı** ve o hedefin `fill > 0` iken süzülmesi.
Dışarıya görünen yüzey: `CLAUDE.md`'nin dört cümlesi, `docs/AYARLAR.md`'nin
"yalnız yukarı kayar" maddesi, `docs/YOL-HARITASI.md`'nin borç maddesi ve
011'in karar kaydı. Ayar şeması, tema, terminfo, shell betiği, app bundle ve
bağımlılık listesi **hiç değişmedi**.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make test-yaris
```

`make shader` **gerekmiyor**: doldurma mevcut `cell_bg`/`cell`
pipeline'larını kullanıyor, yeni `.metal` yok ve `stride 32` assert'leri
değişmedi (phase-3). `make kur` ve `make terminfo` bu setin kapsamı dışında.

### Beklenen çıktı

- `make hepsi` → exit 0 (set kapısında koşuldu, `8c3845f`).
- `make test-yaris` → exit 0 (aynı koşu).
- `make duman` → **koştu ve yeşil** (2026-09-20, üç ardışık koşu `exit 0`):
  `kare=30 hucre=8 glif=6 kural=15 yuva=13/1984 yuk=smoke istek=4 icerik=3
  hareket=27 kayma=0 sessiz≈1740ms kapanis=clean profil=debug ornek=off
  pipeline=ok`. İddia edilen `hucre=8 glif=6 kural=15` **bit bit doğrulandı**;
  `icerik=3` (sınır 8) ve `sessiz≈1740ms` (taban 868) da bandın içinde.
  Duman yine de bu özelliğe **yapısal olarak kör** — süreli koşu `/bin/sh`
  koşuyor, dock yok, doldurma hiç tetiklenmiyor (phase-3), ve `kayma=0` tam
  da onu söylüyor. Yani jeton satırı **regresyon yokluğunun** kanıtı, özelliğin
  çalıştığının değil; onu gözle kontrol (B.2) söyleyecek.
- Ölçüm değişmedi; `docs/OLCUMLER.md`'ye bu setten giren sayı yok. Bekleyen
  iddialar B.3'te.

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make test-yaris` yeşil
- [x] `/code-review` set aralığında koştu (9 bulgu; 1 giderildi, 1 reddedildi,
      1 karar kaydı, 6 waive — B.4)
- [x] `/audit` koştu (7 mercek, 4 bulgu, hepsi giderildi)
- [x] `make duman` — **koştu, yeşil** (üç koşu `exit 0`; jetonlar yukarıda)
- [ ] Gözle kontrol (B.2)

## B. Yayın (doğrulamadan SONRA)

### B.1 Commit'ler `[oto]`

Set yedi phase + bir kapı commit'i:

| Phase | Commit | Ne getirdi |
|---|---|---|
| phase-0 | `d44b64e` | Ölçüm: negatif `originY` meşru → 2b-i kolu; Ctrl-C'nin +1'i |
| phase-1 | `b2c10ed` | Tarayıcının CSI kolu, `2J` bayrağı |
| phase-2 | `592246d` | `fill` hesabı, geçmişten okuma, `Cursor::fill` |
| phase-1b | `3fdd356` | Bayrağın iki kör noktası (vim'in `2J`'si, `content_rows == rows`) |
| phase-3 | `8f99315` | Üçüncü `setViewport`, sayaç muafiyeti |
| phase-4 | `445fe0d` | `sync_origin` guard'ının üçüncü terimi + belge tadilleri |
| phase-5 | `ff66b09` | `point_to_cell`'in reddi |
| kapı | `8c3845f` | `/code-review` + `/audit` bulguları, yanlış örneklerin düzeltilmesi |

`/ship` doğrulama + `main`'e push'u kapsar. **Bu sette push edilmedi.**

### B.2 Gözle kontrol `[elle]`

Üçü de gerçek pencerede, entegrasyonlu zsh oturumunda:

1. **Asıl senaryo.** Ekranı doldur → `ls alfa_<TAB>` → Ctrl-C. İçerik aşağı
   **süzülmeli**, üstten geçmiş satırları girmeli. Bakılacak: kaymanın hızı ve
   yerleşmesi (aynı animatör, aynı stil — 011'in yukarı akışıyla simetrik
   hissetmeli), en üstteki satırın kesirli orijin yüzünden yarım girmesi.
2. **Duran seçim + bant.** Geçmişe kaydır, geçmişten ekrana uzanan bir seçim
   yap, dibe dön, Tab → Ctrl-C. Bant seçili satırları **vurgusuz** gösterir,
   Cmd-C onları yine verir. Bu, bugünkü
   `selection_scrolled_into_history_is_not_drawn` (`session.rs:5828`)
   davranışının görünür yarısı — bant onu yaratmıyor, üstünü açıyor. **Karar
   kullanıcının:** bantta vurgula / bant görünürken seçimi düşür / bırak.
   Kabul edilme gerekçesi ve iki çarenin bedeli `phase-2.md` §5'te.
3. **Alternatif ekrandan çıkış.** `vim` aç, çık. Çıkış karesinde `fill > 0` ve
   öteleme **bir kare** süzülüyor, sonra dock'u geri getiren resize'ın
   `geometry` bayrağı snap'liyor. Hissedilir ve savunulur ama **tasarlanmış
   değil** (`Motion::sync_origin`'in doc'unda adıyla duruyor). Rahatsız
   ederse çare dock resize'ının zamanlamasında, yani 012'nin sahasında — bu
   sette değil.

### B.3 Ölçüm bekleyen iddialar `[komut]` — `/measure`

Hiçbiri kapı değil; `CLAUDE.md`'nin "ölçülmemiş sayı yazılmaz" kuralı gereği
sayı **yazılmadı**:

1. **CSI kolunun tarama maliyeti** yoğun akışta (vim, `less`) — hızlı yol artık
   CSI başına birkaç bayt fazladan adımlıyor (phase-1).
2. **Doldurmalı karede sınır hücresi sayısı** ve `Term` kilidi altındaki ek
   satır okumasının maliyeti — `frame()` bu setten önce geçmişe **hiç**
   inmiyordu (phase-2).
3. **Doldurmanın kare maliyeti** — duman kapısı buna kör (B.1/phase-3), tek
   koruma birim bekçiler.
4. **Alternatif ekrandan çıkıştaki bir karelik kayma** (B.2 §3).

011'in kayıtlı borcu (`kaymanın yerleşme süresi — kanca yok`) bu sette
**büyümedi**.

### B.4 Bilinen sınırlar (adıyla) `[elle]`

Hiçbiri açık hata değil; hepsi kapıda tartılıp kabul edildi.

1. **Duran seçim bantta vurgusuz** — B.2 §2. Kapının en öncelikli kalemi,
   kararı gözle kontrolde.
2. **`\e[H \e[J` kasten temizleme sayılmaz** ve bant ekranı geri getirir.
   Ölçüt `2J`'nin basılması; zsh'in `clear-screen`'i, `clear(1)` ve
   `tput clear` üçü de basıyor, yani kör nokta pratikte dar
   (`discussion.md` → Kör nokta).
3. **Bant seçilemez.** Doldurulan satırlar görünür ama tıklama **reddediliyor**
   (R5.1) — seçilebilir olmaları `Cell.row` sözleşmesini negatife açmayı
   isterdi, bu setin kapsamı dışı. Ayrıca reddedilen tıklama `set_selection`'a
   varmadığı için **duran bir seçimi temizlemiyor** (phase-5).
4. **Blok işareti (chevron) ve komut süresi sayacı bantta yok.** Bant geçmiş
   satırlarını çiziyor, blok üstverisini değil.
5. **Doymuş defterde bir Ctrl-L doldurmayı o oturum için kapatır**: bayrağın
   düşme ölçütü geçmişin damganın üstüne çıkması ve `scrollback` dolduğunda
   artacak sayı kalmıyor (`plan.md` → R1.2). Çare doymuş defterde de artan bir
   sayaç.
6. **Üç küçük kalem** (kapıda waive): `CSI 2;5J` gibi çok parametreli biçim
   bayrağı kurmuyor; DCS gövdesindeki `\e[2J` bayrağı kurar (OSC kollarının da
   paylaştığı sınıf); `Motion::sync`'in iki `bool`'u transpoze edilebilir
   (bekçisi var).
7. **Yol haritasının overlay maddesi kapanmadı, küçüldü**: bu çare liste
   **ekrandayken** üstteki çıktıyı geri getirmiyor (iTerm de getirmiyor),
   yalnız liste kalktıktan sonra ekranı Tab öncesine döndürüyor. Maddenin
   kapanıp kapanmayacağına kullanıcı gözle kontrolde karar verir.

### B.5 Belge etkisi `[oto]` — hepsi kendi commit'lerinde

- **`CLAUDE.md`** — tarayıcının kolları üçten dörde (phase-1), bayrağın ömrü ve
  doldurmanın formülü (phase-1b), `frame()` sınırında boşluğun dolması
  (phase-2), `setViewport`'un liste sayısı (phase-3), "kayma tek yönlüdür"
  cümlesinin koşulu (phase-4), doldurulan alanın neden **seçilemez** olduğu
  (phase-5).
- **`docs/AYARLAR.md:506-516`** — "yalnız yukarı kayar" maddesinin kullanıcı
  dili (phase-4).
- **`docs/YOL-HARITASI.md`** — borç maddesi düzeltildi (yanlış olan "kaybı
  hiçbir terminal geri getiremez" cümlesi ölçümle çürütüldü) ve daraltıldı;
  "spinner testeresi" bedeli artık yalnız doldurmanın kapalı olduğu kollarda
  geçerli (phase-4). Eksik 016 satırı kapıda eklendi.
- **`.tasks/011-tabana-yapisik-icerik/`** — kural **daraltıldığı** için karar
  kaydına not düşüldü; snap'in özgün gerekçesi korundu (phase-4).

### Yayın Checklist

- [x] `make duman` yeşil, jetonlar değişmemiş (2026-09-20)
- [ ] Gözle kontrol B.2 §1 (asıl senaryo — kaymanın hissi)
- [ ] Gözle kontrol B.2 §2 (duran seçim + bant → karar)
- [ ] Gözle kontrol B.2 §3 (alternatif ekrandan çıkış — bir karelik kayma)
- [ ] `docs/YOL-HARITASI.md`'nin overlay maddesi kapansın mı, kullanıcı kararı
- [ ] `/ship`

## Geri Alma

Set **katmanlı** geri alınabilir; her basamak tek başına yeşil bırakır:

1. **Yalnız kaymayı geri al** (dönüş anında olsun, kayarak olmasın):
   `445fe0d`'in `motion.rs` değişikliğini revert et. Doldurma kalır, hedef
   snap'ler. Belge tadilleri aynı commit'te olduğu için elle ayıklanır.
2. **Yalnız seçim reddini geri al:** `ff66b09` revert. Bant üstündeki tıklama
   bugünkü gibi 0. satıra kırpılır — `CLAUDE.md`'nin seçim sözleşmesi ihlal
   olur, yani bu basamak yalnız 3. ile birlikte anlamlı.
3. **Özelliğin tamamı:** `ff66b09 445fe0d 8f99315 592246d 3fdd356 b2c10ed`
   sırasıyla revert. `d44b64e` (ölçüm) kalabilir — sınama ve düzeltilmiş
   yorumdan ibaret, davranışa dokunmuyor. Sonrası bit bit bugünkü hâl:
   `fill_rows()` sıfır dönerken sınırdan geçen kare zaten bugünküyle aynıydı
   (R2.4, geri alma şeridi).
4. Ayar şeması, terminfo, tema, shell betiği ve app bundle değişmediği için
   **geri alınacak kullanıcı verisi yok**; eski ayar dosyaları aynen okunur.
