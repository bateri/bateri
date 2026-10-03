# Oturum geri yükleme

## Hedef

bateri kapanıp yeniden açıldığında — Sparkle güncellemesi, ⌘Q, oturum
kapatma, yeniden başlatma — kullanıcının düzeni geri gelir: pencereler,
sekmeler (grup, sıra, seçili sekme), bölme ağacı (eksen + oran), odaktaki ve
büyütülmüş pane, her pane'in yerel dizini, punto farkı, uzak hedefi ve
geçmişinin metni renk ve biçimiyle. Kabuklar yenidir, koşan işler ölür.
Biçim sürümlü ve canlı devrin temeli (`context.md` → Sonraki set ile ilişki).
Kararlar `discussion.md` → Karar.

## Gereksinimler

- **R1** — Geçmiş anlık görüntüsü ve oynatması (`bt-core`)
  - **R1.1** — Kapanışa kilitli bir `Session` yöntemi birincil ızgaranın
    geçmişini + ekranını VT baytlarına çevirir: metin, SGR (ön/arka plan,
    kalın/eğik/sönük/ters/gizli, alt çizgi biçimi ve rengi, üstü çizili),
    kümenin sıfır genişlikli karakterleri; sarılan satır satır sonu taşımaz;
    OSC 8 ve blok çıpaları yazılmaz. Alternatif ekrandayken birincil ızgara
    okunur (`swap_alt`, yalnız kapanışta).
  - **R1.2** — Kabuk `Input`'tayken son bloğun çıpa satırından itibaren
    kesilir (bastırılan giriş satırı ve prompt geçmişe girmez); sondaki boş
    satırlar dökülmez; akış satır sonuyla biter.
  - **R1.3** — `SessionOptions::replay` verilirse baytlar `EventLoop`'tan önce
    ayrıştırıcı + `ClusterHandler` yolundan `Term`'e uygulanır, `Scanner`'a
    uğramaz; kabuk defteri, ayna ve uzak durum etkilenmez.
  - **R1.4** — Round-trip bekçisi: anlık görüntü → taze `Term`'e oynatma →
    hücreler karakter, renk, biçim ve küme olarak eşit; dar genişliğe
    oynatmada sarılan satır yeniden sarılır; geniş karakter iki hücre kalır.
  - **R1.5** — İlk girdi çalıştırılmadan verilebilir (satır sonu eklenmez);
    bugünkü çağıranların davranışı değişmez.
- **R2** — Kayıt modeli ve dosya (`bt-shell-common`)
  - **R2.1** — Platformsuz model: pencere (çerçeve, seçili sekme, key), sekme
    (ağaç: eksen + oran + pane sırası, odak, zoom), pane (`TabId`, yerel
    dizin, punto adımı, uzak satır, geçmiş var mı).
  - **R2.2** — Sürümlü satır biçimi: ilk satır sürüm; ağaç ön-sıralı jeton
    dizisi; metin alanları kaçışlı; tanınmayan sürüm, bozuk satır ya da
    tutarsız ağaç → `None`, panik yok. Round-trip sınamalı.
  - **R2.3** — Dosya: dizin paket kimliğiyle adlı ve kilitle sahiplenilmiş,
    `0700`/`0600`; önce geçmişler, en son düzenin geçici ad + `rename`'i;
    okuma düzeni oynatmadan önce tüketir; yetim geçmişler süpürülür;
    eksik/bozuk geçmiş yalnız o pane'i boş bırakır.
  - **R2.4** — `Zoom` adımını okuyup kuran erişimci.
- **R3** — Kayıt ve geri yükleme (`bt-shell-macos`)
  - **R3.1** — Kayıt `AppDelegate::shutdown`'ın başında, `begin_close`'tan
    önce; pencere yoksa kayıt silinir; hermetik koşu ve paketsiz süreç
    okumaz/yazmaz.
  - **R3.2** — Açılışta kayıt varsa düzen kurulur, yoksa ya da hiçbir pencere
    kurulamazsa bugünkü tek pencere. Pencere çerçevesi görünür ekrana kırpılı
    ve kabuk doğmadan önce; sekmeler sırayla aynı gruba; seçili sekme ve key
    pencere geri gelir.
  - **R3.3** — Tek kurulum yolu: bir sekmenin bütün pane'leri kayıtlı ağaçla
    önce yerleşir, kabuklar yerleşimden sonra başlar; ağaç en küçük pane
    sınırına sığmazsa eşitlenir; odak ve zoom geri gelir.
  - **R3.4** — Her pane `pane_launch`'tan geçer ve kayıtlı `TabId`'yi, dizini,
    punto adımını ve geçmişi alır; uzak pane'in hedef satırı giriş satırında
    hazır, çalıştırılmamış.
  - **R3.5** — Pencereler `setRestorable(false)`.
- **R4** — Ayar ve belgeler
  - **R4.1** — `[terminal] restore_windows = "all" | "layout" | "off"`,
    varsayılan `"all"`; kullanılamayan dosyada `"layout"`; `"layout"` geçmiş
    yazmaz, `"off"` hiçbir şey yazmaz ve kalanı siler.
  - **R4.2** — Ayar penceresinde satır, `docs/AYARLAR.md` (Time Machine notu
    dahil), CHANGELOG ve `CLAUDE.md`'nin bugünkü hâl paragrafı.

## Yaklaşım

1. `bt-core`: saf kodlayıcı (satır/hücre → VT baytları) + kapanışa kilitli
   `Session` yöntemi; `Session::spawn`'da `Term::new` ile `EventLoop::new`
   arasında oynatma; ilk girdinin "çalıştırma" biti; ayar anahtarı modeli.
2. `bt-shell-common`: `restore` modülü — model, satır biçimi, ağaç dönüşümü
   (yaprak = pane'in kayıttaki sırası), dosya, kilit, süpürme; `Zoom`
   erişimcisi.
3. `bt-shell-macos`: `Launch`'a kimlik ve geçmiş; `TerminalWindow::restore`
   (yerleştir, sonra başlat); kayıt `shutdown`'da; açılışta geri yükleme.
4. Ayar penceresi satırı ve belgeler.

## Kapsam Dışı

- Koşan işin, PTY'nin ve tam VT durumunun (modlar, kayıtlı imleç, charset,
  alt ekran içeriği) korunması — canlı devir seti; onun biçim/sürüm kuralı da.
- Uzak dizin, blok şeritleri ve süreleri, dock aynası, arama, seçim,
  kaydırma konumu, tema (`discussion.md` → Karar 4).
- Çökme kurtarma (periyodik kayıt).
- Linux kabuğu (henüz yok); model ve dosya katmanı platformsuz yazılır.
- Kapanış süresi ve dosya boyutu iddiası — `/measure`'ın işi.

## Akış

```
kapanış: AppDelegate::shutdown                açılış: did_finish_launching
  restore_windows == off → kaydı sil            paketsiz | hermetik | off → tek pencere
  kilit yok | paketsiz | hermetik → atla        kilit al; restore::take()
  pencere → sekme → pane topla                    (düzeni okur ve tüketir)
    "all": Session::final_history() → .vt         None → open_window(None, Window)
  restore::save(): geçmişler, sonra düzen         Some → her pencere, her sekme:
    (rename = commit), yetimleri süpür              Launch × n (TabId, dizin, zoom,
  begin_close × n … (bugünkü yol)                     replay, uzak satır \r'siz)
                                                    TerminalWindow::restore:
                                                      ağaçla yerleştir → start × n
                                                    sekme grubu, odak, zoom,
                                                    seçili sekme, key pencere
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| phase-3 | |
| phase-4 | |
| kapı | |
