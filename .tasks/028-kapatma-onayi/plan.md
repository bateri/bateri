# Kapatma onayı

## Hedef

Kabuğun dışında bir program ön planda koşarken (Claude Code, vim, ssh, uzun
bir derleme) bir sekmeyi, pencereyi ya da uygulamayı kapatmak önce sorar ve
programın adını söyler; kabuk boştayken ve kabuk kendi çıktığında hiçbir şey
sorulmaz. `[terminal] confirm_close` bunu `never` / `running` / `always`
arasında seçtirir. Kararlar ve gerekçeleri `discussion.md` → `## Karar`.

## Gereksinimler

- **R1** — Koşan işin tespiti.
  - **R1.1** — `bt-core` PTY çocuğunun pid'ini verir (`Session::child_pid`);
    `bt-core` platformsuz kalır, `Cargo.toml`'u değişmez.
  - **R1.2** — `bt-shell` kabuğun kimliğini doğurduğu komuttan kaydeder:
    `login` yolunda kabuk çocuğun çocuğu, doğrudan yolda çocuğun kendisi.
  - **R1.3** — Ön plan grubu kabuğun `e_tpgid`'inden, lider ve grup üyeleri
    kısa bilgiden okunur; ön plan grubu kabuğun grubu değilse iş koşuyor.
  - **R1.4** — Adlar ön plan grubunun yapraklarından, tekrarsız; yoksa
    liderin adı, o da yoksa adsız "koşuyor".
  - **R1.5** — Başarısızlık kolu: çocuksuz `login` ve ölü okuyucu boşta,
    okunamayan tablo koşuyor.
  - **R1.6** — Karar saf bir fonksiyon ve sahte tabloyla sınanır; okuyucu
    doğrudan yolda gerçek bir PTY'yle sınanır.
- **R2** — Sorular.
  - **R2.1** — `should_ask` saf: süreli koşu **her zaman** hayır ve ilk
    soru; sonra `never` hayır, `always` evet, `running` koşan iş varsa evet.
    Süreç tablosu yalnız cevap ona bağlıysa okunur.
  - **R2.2** — ⌘W ve Close Tab (`performClose:`) `windowShouldClose:`'da
    sorar; sayfa açıkken ikinci bir soru açılmaz.
  - **R2.3** — ⇧⌘W grubun tamamı için **tek** sayfa sorar ve onayda her
    sekmeyi `close` ile kapatır.
  - **R2.4** — Kırmızı düğme ve sekme çubuğunun "Close Other Tabs"ı ölçülür;
    kural "bir jest, en çok bir soru", kapsam AppKit'inki.
  - **R2.5** — ⌘Q ve Dock ▸ Quit `applicationShouldTerminate:`'te bütün
    pencereler için tek uyarı sorar (uygulama önce öne alınır); sistem
    kapanışı ve oturum kapatma da aynı yoldan.
  - **R2.6** — Kabuğun çıkışı (`close`) hiçbir zaman sormaz.
  - **R2.7** — Metin: başlık kapatılanı söyler, açıklama süreçleri adıyla
    sayar; "Close"/"Quit" varsayılan (Return), "Cancel" Esc. Soruyu kuran
    tek fonksiyon pencere listesi alır.
  - **R2.8** — Sayfanın bloğu yalnız pencere kimliğini yakalar; pencere o
    arada kapandıysa iş düşer. Açık sayfanın yuvası `WindowIvars`'ta.
- **R3** — Ayar ve kayıt.
  - **R3.1** — `[terminal] confirm_close = "never" | "running" | "always"`,
    varsayılan `"running"`; tanınmayan değer kendi anahtarını değiştirmez;
    `TerminalOptions`'a ve `changes().terminal`'a girmez; kapanış anında
    güncel ayardan okunur. Şablonda, `docs/AYARLAR.md`'de, round-trip
    sınamasında.
  - **R3.2** — Bağımlılık bayrakları (`NSAlert`, `NSButton`, `NSControl`,
    `block2`) ve `bt-shell`'in `block2` kenarı yorumlarıyla; bayatlayan
    cümleler (`bt-shell/Cargo.toml`, `app.rs`'teki iki doc) ve `CLAUDE.md`
    (katman tablosu, ayar listesi, Sekmeler paragrafı) aynı commit'te.

## Yaklaşım

1. `bt-core`: `child_pid`, spawn'da `TappedPty`'den önce alınır.
2. `bt-shell`: süreç tablosu modülü — saf karar + ince `libc` okuyucusu;
   pencere kabuğun kimliğini (login mi doğrudan mı) doğumda kaydeder.
3. `bt-core::settings`: `confirm_close`; `bt-shell`: `should_ask`, soru
   kurucusu, `windowShouldClose:`, `closeWindow:`,
   `applicationShouldTerminate:`, bayraklar, belgeler.

## Kapsam Dışı

Arka plan işleri, kabuk yerleşiği döngüsü, `exec vim` (Karar 7, bilinen
sınır); "bir daha sorma" kutusu; `close_on_exit`; bölme; OSC 133'ün ikinci
sinyal olarak kullanımı.

## Akış

```
⌘W / Close Tab ──performClose:──▶ windowShouldClose: ─┐
kırmızı düğme ───(ölçülen kapsam)──────────────────────┤
⇧⌘W ─────────────closeWindow: (grup) ──────────────────┤
⌘Q / logout ─────applicationShouldTerminate: (hepsi) ──┤
                                                       ▼
                    should_ask(run, confirm_close, ‹running?›)
                      │ süreli koşu → hayır (tablo okunmaz)
                      │ running → jobs::foreground(pencere) ← child_pid + libproc
                      ▼
              hayır → kapan          evet → sayfa / runModal
                                       Close → close() / NSTerminateNow
                                       Cancel → hiçbir şey
exit ──child_exit──▶ close() ──(delegate'e sormaz)──▶ windowWillClose:
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| kapı | |
