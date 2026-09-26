# ssh modunun ikinci turu

## Hedef

Uzak oturum 036'nın göstergesinden bir çalışma ortamına dönüşsün: kullanıcı
prod'da olduğunu renkten bilsin (desen listesi ya da menüden tek tık), ssh
sekmesinde ⌘T aynı makineye ikinci bir kabuk açsın, kopan ssh boş giriş
satırında "⏎ reconnect" teklif etsin ve Finder'dan bırakılan dosya uzak
kabuğa yerel bir yol yazmasın.

## Gereksinimler

- **R1** — Uzak hedefin modeli (Karar 1).
  - **R1.1** — `jobs::remote`'un uzak cevabı host, tür (ssh/mosh) ve yeniden
    koşturulacak argv'yi taşıyor; argv'nin ayıklama kuralı (`-L -R -D -M -f`)
    ve mosh'un `-#` satırı saf fonksiyonda, sahte tabloyla sınanıyor.
  - **R1.2** — `Session::set_remote` hedefi bütün olarak (host, tür, argv,
    kaçırılmış satır) alıyor ve `bt-core` onu uzak durumun yanında tutuyor;
    `C`/`D`/`A` kuralı ve nesil kapısı değişmiyor.
- **R2** — Host'un işareti (Karar 2).
  - **R2.1** — `[remote] hosts`: sıralı `{ host, mark }` dizisi; değerler
    `production`/`staging`/`development`/`none`/`#rrggbb`; bozuk girdi
    anahtarın tamamını reddediyor, tanı alt başlıkta; varsayılan boş.
  - **R2.2** — Glob `*`/`?`, harf duyarsız; desende `@` yoksa girdinin son
    `@`'ten sonrası; ilk eşleşen kazanır, `none` eşleşmeyi bitirir.
  - **R2.3** — Çözüm `set_remote`'ta ve canlı ayar değişiminde; kare yolu
    desen görmüyor.
  - **R2.4** — `docs/AYARLAR.md` ve şablon; round-trip sınaması bilinmeyen
    anahtarı ve yorumu koruyor.
- **R3** — Renk (Karar 3).
  - **R3.1** — `warning` tema rolü: iki gömülü değer (ANSI sarısı), tema
    dosyasında opsiyonel, zeminde 3:1 bekçisi.
  - **R3.2** — İşaretin rengi `⇄ host`'a ve dock'un üst saç çizgisine gidiyor;
    işaretsizde bugünkü `info`.
- **R4** — Sekmede renk (Karar 4): işaretli host'ta `NSWindowTab`'ın
  `accessoryView`'unda işaret renginde bir nokta; işaretsizde ve yerelde
  yok; uzak durumun, ayarın ve temanın kenarlarında tazeleniyor.
- **R5** — Menüden işaretleme (Karar 5): Shell ▸ "Mark “{host}” as ▸"
  (Production / Staging / Development / None, geçerli çözümde onay); yerelde
  gri; yazım `SettingsEdit`'ten, tam girdi yerinde ya da başa, None'ın glob
  kuralı; saf yazım kuralı sınanıyor.
- **R6** — ⌘T aynı host'a (Karar 6).
  - **R6.1** — Uzak sekmede ⌘T ve sekme çubuğunun `+`'sı yerel dizinde yerel
    kabuk doğuruyor ve ilk girdisi hedefin satırı + `\r`.
  - **R6.2** — İlk girdi `SessionOptions`'tan; sarmalayıcılı oturumda bizim
    ilk kimlikli `A`'mızda, sarmalayıcısızda doğumda; `send_input`'un
    yolundan. Satır okunur kaçırılıyor (`@ : , +`, sözcük başı dışında `=`).
  - **R6.3** — Shell ▸ New Local Tab (⌥⌘T) her zaman yerel; ⌘N yerel.
- **R7** — Bağlantı kopunca (Karar 8).
  - **R7.1** — Uzak oturum etkinken, tür ssh, bizim `D`'miz 255 → teklif
    yuvası (host, işaret, satır); `A` silmiyor, `send_input` ve `C` siliyor.
  - **R7.2** — Dock'un boş giriş satırında yer tutucu:
    `⇄ {host}  Connection lost · ⏎ reconnect` (host işaret renginde, kalan
    `dim`).
  - **R7.3** — Teklif varken boş satırda düz ⏎ satırı + `\r` gönderiyor;
    teklif yokken Enter yolu bayt bayt bugünkü.
  - **R7.4** — mosh'ta ve dock'suz pencerede teklif yok.
- **R8** — Finder damlası uzak oturumda yerel yol yapıştırmıyor (Karar 7;
  kullanıcının seçimine göre yükleme ya da ret). Yerel sekmede bugünkü gibi.
- **R9** — Sözleşme: `CLAUDE.md` (dokuz rolün dokuzu, menü listesi, ⌘T,
  teklif, damla) ve yol haritasının 037 satırı.

## Yaklaşım

1. **phase-1 model ve renk** — `bt-core`: hedefin bütün modeli, `[remote]
   hosts` ayrıştırma/eşleşme/yazım, `warning` rolü, işaretin kenarda çözümü,
   dock'un iki rengi; `bt-shell`: yoklamanın argv'si ve satırı, ayarın canlı
   uygulanması. Kullanıcı dosyaya desen yazınca renk görünüyor.
2. **phase-2 menü ve sekme** — AppKit yarısı: "Mark … as ▸" ve sekmenin
   noktası.
3. **phase-3 ⌘T** — `SessionOptions`'ın ilk girdisi, `open_window`'un hedefi,
   New Local Tab.
4. **phase-4 teklif** — `ShellLog`'un teklif yuvası, dock'un yer tutucusu, ⏎.
5. **phase-5 Finder ve sözleşme** — Karar 7'nin seçimi; `CLAUDE.md` ve yol
   haritası.

Gerekçeler `discussion.md` → Karar 1–8.

## Kapsam Dışı

- Tam uzak entegrasyon (uzakta blok, dock aynası, betik/terminfo taşıma) —
  `docs/YOL-HARITASI.md`'de satır.
- Glob'da sınıf ve küme (`[a-z]`, `{a,b}`); `~/.ssh/config`'in `HostName`
  çözümü; addan tahmin (kullanıcı reddetti).
- Ayar penceresinde host listesi düzenleyicisi.
- Tek sekmeli pencerenin başlık çubuğunda renk.
- `-o LocalForward=…` gibi `-o` biçimli yönlendirmelerin ayıklanması.
- İki ayrı kopma metni; mosh için teklif; dock'suz pencerede teklif.
- Yüklemede ilerleme göstergesi, parola ile giriş (seçenek B seçilirse).

## Akış

```
yoklama (036) ── Probe::Remote{host, kind, argv} ──▶ bt-shell: satır = shell_quote(argv)
                                                        │
                                  Session::set_remote(nesil, hedef)
                                                        │ desen çözümü (kenarda)
                     ┌──────────────────┬───────────────┴───────────────┐
                     ▼                  ▼                               ▼
            dock: ⇄ host + üst çizgi   sekme: accessoryView noktası    Shell ▸ Mark … as ▸
            (işaretin rengi)           (işaretliyse)                   → settings.toml → watch
                                                                         → set_host_marks
⌘T (uzak) ── open_window(from, hedefin satırı) ──▶ yeni Session: ilk bizim A'da satır + \r
⌥⌘T ────── open_window(from, yok) ──────────────▶ yerel kabuk

ssh biter ── bizim D; exit 255 ──▶ teklif yuvası ──▶ dock: "⇄ prod  Connection lost · ⏎ reconnect"
                                        │ ⏎ (boş satır)          │ başka girdi / C
                                        ▼                        ▼
                               send_input(satır + \r)        teklif kalkar

Finder damlası (uzak) ──▶ Karar 7: yükle (scp → uzak yolu yapıştır) | reddet
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| phase-5 | ✅ |
| kapı | ✅ |
