# Sekmeler

## Hedef

bateri'de macOS'un kendi sekmeleri olsun: ⌘T yeni sekme açar, her sekme kendi
kabuğunu koşturur, sekmeler sürüklenip sıralanır ve pencereden koparılır,
standart kısayollar (⌘T ⌘W ⇧⌘W ⌘N ⇧⌘]/[ ⌃⇥ ⌘1…9) çalışır. Pencere temiz
görünür: tek sekmede çubuk yok, başlık çubuğu temanın zemininde ve içerikle
arasında dikiş yok. Arka plandaki sekme sıfır kare çizer. Kararlar ve
gerekçeleri `discussion.md` → `## Karar`.

## Gereksinimler

- **R1** — Pencere başına durum `AppDelegate`'ten ayrılır; davranış değişmez.
  - **R1.1** — Pencere başına: pencere, view, yüzey, `Renderer`, link,
    oturum, `ShellWake`, dock payı (anlık + doğum), geçici punto. Uygulama
    genelinde: ayarlar, izleme kaynakları, alt başlık yuvaları, ölçüm
    defteri, süreli koşu tarifi, pencere listesi.
  - **R1.2** — Kayıt anı yolları (ayar, tema, görünüm, Hareketi Azalt,
    caret, alt başlık) **her pencereye** uygulanır; yayılan eylemler
    `AppDelegate`'te, pencereye ait eylemler (punto) pencere delegate'inde.
  - **R1.3** — Alternatif ekran habercisi responder zincirini kullanmaz:
    pencere kimliğini yakalar, listeden bulur.
  - **R1.4** — Her pencerenin kendi `Renderer`'ı (Karar 2a); `bt-gpu`'nun
    API'si değişmez.
- **R2** — `bt-core`'un sekmeye hazırlığı; tek pencerede görünen tek fark
  başlık.
  - **R2.1** — Kapanış başlatma ve bekleme diye bölünür;
    `Session::shutdown()` = başlat + `SHUTDOWN_GRACE` bekle, davranışı ve
    `Teardown` değerleri aynı.
  - **R2.2** — `Session` son OSC 7 dizinini verir (yoksa `None`).
  - **R2.3** — Başlık: OSC 0/2 → dizinin son bileşeni (ev `~`) → `bateri`
    (Karar 7). OSC 0/2 `Term` kilidi altındaki olaydan bir yaprak yuvaya
    iner; kural saf ve sınanıyor.
  - **R2.4** — Başlık değişince (OSC 0/2 **ve** OSC 7) `Wake` haber verir;
    haber yük taşımaz, ana kuyrukta en çok bir iş bekler, pencere başlığı
    **değişimde** yazılır — kare yolu başlık hesaplamaz.
- **R3** — Çok pencere, sekmeler, kapanış.
  - **R3.1** — Native tabbing açık, bütün pencereler aynı
    `tabbingIdentifier`; ⌘N yeni pencere, ⌘T ve çubuğun `+`'sı
    (`newWindowForTab:`) etkin pencerenin grubuna yeni sekme.
  - **R3.2** — Yeni kabuk etkin sekmenin OSC 7 dizininde, yoksa ev dizininde
    (Karar 4); geçici punto devralınır (Karar 3).
  - **R3.3** — Kısayol tablosu (`discussion.md` → Karar 6) menü öğeleriyle;
    `keyDown:`'ın izin listesi değişmez; ⌃⇥/⌃⇧⇥'in yolu doğrulanmış ve
    AppKit'in eklediği öğeler çiftlenmemiş.
  - **R3.4** — ⌘1…⌘8 n. sekme (yoksa no-op), ⌘9 son sekme; eşleme saf ve
    sınanıyor.
  - **R3.5** — Kabuk çıkınca o sekme kapanır; son pencere kapanınca uygulama
    açık kalır, Dock ikonu yeni pencere açar. **Süreli koşuda** `child_exit`
    doğrudan `terminate:`, pencere rapordan önce listeden çıkmaz ve son
    pencerede uygulama biter (Karar 5, Muhakeme).
  - **R3.6** — Pencere kapanırken `Waker` ana thread'de `ShellWake`'ten
    sökülür, kapanış başlatılır ve beklenmez; pencere nesnesi hemen düşer.
    ⌘Q listedeki bütün oturumları **tek** `SHUTDOWN_GRACE` içinde paralel
    kapatır. `kapanis=` süreli koşuda bugünküyle aynı (Karar 9).
  - **R3.7** — Arka plandaki sekme sıfır kare çizer (örtülme yolu doğrulanmış),
    öne gelince tek kare ve animasyon tekrarı yok (Karar 8); odak yalnız
    seçili sekmenin.
- **R4** — Temalı krom (Karar 1 → C): başlık çubuğu saydam ve ayırıcısız,
  pencere zemini temanın `background`'ı, görünüm temanın açıklığından; tema
  değişince bütün pencereler. Görünüm zinciri yeniden ateşlenmez
  (`apply_appearance` değişmeyen sistem bitinde erken döner). Tek sekmede
  çubuk yok.
- **R5** — Her phase yanlışladığı sözleşme cümlesini kendi commit'inde
  düzeltir (`CLAUDE.md`, `bt-shell/src/lib.rs`, `menu.rs`, `wake.rs`);
  set sonunda `docs/YOL-HARITASI.md`.

## Yaklaşım

1. **Pencereyi nesneye çıkar** (R1): tek pencere, bit bit aynı davranış,
   duman jetonları değişmez.
2. **`bt-core`'u hazırla ve başlığı bağla** (R2): kapanışın bölünmesi, dizin
   okuyucusu, başlık yuvası ve kuralı, `Wake`'e başlık haberi; tek pencere
   artık dizin/uygulama başlığını gösteriyor.
3. **Pencereleri ve sekmeleri aç** (R3): tabbing, Shell ve Window menüleri,
   kısayollar, kapanışın pencereye inmesi, `Waker`'ın sökülmesi, paralel ⌘Q,
   görünürlük ve odak doğrulaması.
4. **Kromu boya ve seti kapat** (R4, R5): başlık çubuğu, görünüm, tema
   fan-out'u, yol haritası, set kapısı ve gözle kontrol.

## Kapsam Dışı

`discussion.md` → Karar 10: bölme, pasif sekmenin drawable'larını bırakmak,
pencere geri yükleme, komut paletinin sekme listesi, sekme yeniden
adlandırma, kapatma onayı, çubuğun kendi rengini boyamak; koşan komut
noktası, zil ve etkinlik göstergesi (Karar 7); koşan komutun adını kendimiz
göstermek (betik değişikliği). Yeni ayar anahtarı ve yeni bağımlılık yok.

## Akış

```
⌘T / + / ⌘N ─► AppDelegate.open_window(etkin?)
                 │  dizin = etkin.session.working_directory() ?? ev
                 │  zoom  = etkin.zoom ?? 0
                 ▼
              TerminalWindow ── NSWindow (tabbingIdentifier ortak)
                 ├─ Renderer (kendi atlası)     ├─ BateriView
                 ├─ Session ◄── ShellWake{id, Mutex<Option<Waker>>}
                 │                 ├ child_exit    ─► ana kuyruk ─► id'li pencereyi kapat
                 │                 │                 (süreli koşu: terminate:)
                 │                 └ title_changed ─► ana kuyruk (≤1 iş) ─► setTitle(session.title())
                 └─ DisplayLink ── alt ekran habercisi{id} ─► ana kuyruk ─► id'li pencere
                         ▲
   occlusion ────────────┘ set_visible   (seçili olmayan sekme = görünmez = sıfır kare)
   key/resign ───────────┘ set_focused

ayar/tema kaydı ─► AppDelegate ─► her TerminalWindow: session / link / renderer / krom

pencere kapat : link.stop → waker sök → begin_shutdown (beklemez) → nesne düşer
⌘Q            : her pencere begin_shutdown → tek SHUTDOWN_GRACE → sonuç atılır
süreli koşu   : tek pencere → shutdown() → kapanis=
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | |
| kapı | |
