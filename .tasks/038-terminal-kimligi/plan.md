# Terminal kimliği ve sekme URL'i

## Hedef

Kabuk hangi terminalde koştuğunu bilsin (`TERM_PROGRAM=bateri`, sürümüyle)
ve her sekmenin dışarıdan açılabilen bir adı olsun: sekme başına sabit bir
`TERM_SESSION_ID` ve `BATERI_TAB_URL=bateri://tab/<id>`; `open
$BATERI_TAB_URL` o sekmeyi öne getirir.

## Gereksinimler

- **R1 — Kimlik ortamı (`bt-core`).**
  - **R1.1** — Her oturumun çocuğu `TERM_PROGRAM=bateri` ve
    `TERM_PROGRAM_VERSION=<workspace sürümü>` alır; sekme kimliği verildiyse
    `TERM_SESSION_ID=<UUID>` ve `BATERI_TAB_URL=bateri://tab/<UUID>` da.
    Dördü `TERM`/`COLORTERM`'ün katmanında: ek ortam onları ezemez.
  - **R1.2** — Sekme kimliği tipli (`TabId`): yalnız kanonik UUID metnini
    kabul eder; URL'yi yazan ve `bateri://tab/<id>`'yi çözen tek yer. Çözüm
    şema/host/UUID'de büyük-küçük harf duyarsız, başka her biçimde (`block/`
    dahil) `None`; panik yok.
  - **R1.3** — Sürüm sabiti `bt-core`'dan dışarı açık ve `bt-shell`'in
    sürümüyle eşitliği sınanıyor (Karar 3).
- **R2 — Sekme kimliği (`bt-shell`).**
  - **R2.1** — Her `TerminalWindow` doğarken `NSUUID` ile bir kimlik alır,
    ömrü boyunca sabit; oturuma `SessionOptions` ile gider. Süreli koşu
    dahil.
  - **R2.2** — Kimlikle pencere bulunur (URL yolu için).
- **R3 — URL şeması.**
  - **R3.1** — `Info.plist.in`'de `CFBundleURLTypes` → `bateri`; `make kur`
    içerik denetimi şemayı arar.
  - **R3.2** — `application:openURLs:` her URL'yi sırayla işler: tanınan ve
    yaşayan sekme → (küçültülmüşse geri açılır) seçili sekme + key + uygulama
    öne; tanınan ama ölü → yalnız uygulama öne; tanınmayan → hiçbir şey.
    Yeni pencere hiçbir kolda açılmaz; soğuk başlatmada ilk pencere bir kez.
  - **R3.3** — URL hiçbir kolda kabuğa bayt göndermez (Karar 6).
- **R4 — Belge.** `CLAUDE.md`: kimlik ortamının kuralı, URL'nin "yalnız
  odak" değişmezi, `bateri://` şemasının iki yolu (`block/` iç çıpa, `tab/`
  dış ad); `docs/YOL-HARITASI.md`'nin tıklanabilir bağlantılar satırına
  `bateri://` bağlantılarının yutulacağı notu.

## Yaklaşım

1. **phase-1 — kimlik ortamı.** `bt-core`'da `TabId`, sabitler ve `spawn`'un
   dört değişkeni; `bt-shell`'de `NSUUID` bayrağı, pencerenin kimliği ve
   `SessionOptions` kurucuları. Sonunda her sekmenin kabuğunda dört değişken
   var; URL henüz hiçbir şey açmıyor.
2. **phase-2 — şema ve odak.** `Info.plist.in`, `make kur` denetimi,
   `application:openURLs:` ve odak sırası, belge.

Gerekçeler `discussion.md` → Karar.

## Kapsam Dışı

- `LC_TERMINAL`; miras kalan yabancı kimliklerin (`ITERM_SESSION_ID`…)
  silinmesi (Karar 8).
- URL'den komut çalıştırma, dizin açma ya da yeni pencere/sekme doğurma —
  URL yalnız var olan sekmeye odaklar (Karar 5–6).
- OSC 8 bağlantılarını açmak (yol haritasında ayrı set; bu set yalnız
  çakışmamayı kayda geçiriyor).
- Uzak kabuğa (ssh) kimlik taşımak.
- Pencere geri yükleme (state restoration) ile kimliğin yeniden başlatmadan
  sağ çıkması.

## Akış

```
TerminalWindow::new ── NSUUID ──► tab_id (ivar)
        │
        └─ start ─► SessionOptions { tab_id, env } ─► Session::spawn
                                    env ∪ {TERM, COLORTERM, TERM_PROGRAM,
                                           TERM_PROGRAM_VERSION,
                                           TERM_SESSION_ID, BATERI_TAB_URL}
                                           (sabitler en son → ezilemez)

open bateri://tab/<id> ─► LaunchServices ─► application:openURLs:
        │ TabId::from_url
        ├─ None            → hiçbir şey
        ├─ Some, pencere yok → NSApp.activate()
        └─ Some, pencere var → deminiaturize? → makeKeyAndOrderFront
                               → NSApp.activate()
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| kapı | |
