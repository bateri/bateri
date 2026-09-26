# ssh'ta uzak oturum hissi

## Hedef

`ssh` (ya da `mosh`) ile bağlanan kullanıcı uzak makinede olduğunu her
yüzeyde görsün: dock tek satırlık bir durum çubuğuna iner ve `⇄ host  /uzak/yol`
der, üst çizgisi `info` renginde; başlık ve sekme `⇄` taşır. Uzak tarafa hiçbir
şey kurulmaz. Bugünkü yanlış — ssh sürerken yerel yol ve dalın gösterilmesi —
kalkar.

## Gereksinimler

- **R1** — Uzak oturumun modeli `bt-core`'da.
  - **R1.1** — `ShellLog` `Running`'e her geçişte bir komut nesli artırır;
    `Session::running_command()` onu verir, `Session::set_remote(nesil, host)`
    yalnız nesil tutuyor ve safha `Running` iken yazar; uzak durum `C`, `D`
    ve `A`'da kendiliğinden silinir.
  - **R1.2** — `Wake::command_started` `Running` kenarında gelir (kenarda,
    yüksüz); üç uygulayıcı onu uygular.
  - **R1.3** — OSC 7 yetkisiyle birlikte olay doğurur; uzak etkinken her
    OSC 7, etkin değilken yabancı yetkili OSC 7 uzak yuvaya gider; yerel
    davranış değişmez.
- **R2** — Başlık: uzak etkinken `⇄ {OSC başlığı}`, yoksa `⇄ {host}`; iki
  kenarda da `title_changed`/tazeleme.
- **R3** — `info` tema rolü: `Theme` alanı, iki gömülü değer, tema
  dosyasında opsiyonel anahtar, zeminde 3:1 bekçisi, `docs/AYARLAR.md`.
- **R4** — Bağlam satırının uzak biçimi.
  - **R4.1** — `⇄ host` `info` renginde, sonra iki boşluk ve uzak yol bugünkü
    iki kademede; dal yok. Uzak yol yoksa yalnız `⇄ host`. Host kısalmaz;
    sığmazsa yalnız `⇄`, yol soldan kısalır.
  - **R4.2** — `⇄` varsayılan fontta küçük sınıfta kutu değil (atlas
    sınaması); kutuysa `↔`, ikisi de kutuysa iş durur.
  - **R4.3** — Dock'un üst saç çizgisi uzakta `info` renginde (`Dock`'a ayrı
    bir alan).
- **R5** — Uzakta sıfır giriş satırı.
  - **R5.1** — `Cursor::input_rows = 0` (dock'lu pencere, alternatif ekran
    değil); `.max(1)` bekçileri sıfırı taşır.
  - **R5.2** — Bandın fazlası tek, kesirli formülden; ızgara aşağı çizilir,
    tepedeki şeridi doldurma bandı kapatır; iki yönde süzülme.
  - **R5.3** — Giriş satırı yokken dock'a tık no-op.
  - **R5.4** — Uzak oturum `caret_in_dock`'un dördüncü ön koşulu: uzakta
    caret dock'ta değil, Dock→Grid tutmasının içinde de.
- **R6** — Algılama `bt-shell`'de.
  - **R6.1** — Argümanlar `KERN_PROCARGS2`'den, yalnız aday adlı üyeler için.
  - **R6.2** — Grubun en üstteki tanınan süreci; ssh (yalnız etkileşimli) ve
    mosh (`mosh` betiği, `mosh-client`); host yazıldığı gibi.
  - **R6.3** — `C` kenarında yoklama; kararsızsa sonraki çıktı kenarında
    tekrar (en çok bir bekleyen iş); ilk kesin cevap `D`'ye kadar kilitli.
- **R7** — Sözleşme: `CLAUDE.md` (yol haritasının 036 satırı ve kapsam dışı
  iki satırı set açılırken yazıldı).

## Yaklaşım

1. **Model önce, çizim sonra, algılama en son.** phase-1 `bt-core`'da uzak
   durumu, OSC 7 yönlendirmesini, başlığı, `info` rolünü, bağlam satırını
   ve üst çizginin rengini kuruyor; `Wake::command_started` `bt-shell`'de
   no-op gövdeyle iniyor. Üretimde henüz kimse `set_remote` çağırmıyor, yani
   davranış bugünküyle aynı; hepsi sınamayla doğrulanıyor.
2. phase-2 sıfır giriş satırını `bt-core` → `bt-gpu` boyunca açıyor
   (`input_rows`, bant fazlası, fare) — yine sınamayla, üretimde tetiksiz.
3. phase-3 algılamayı bağlıyor: `jobs.rs`'e argüman okuma ve saf ayrıştırıcı,
   `ShellWake`'e yoklama; sözleşme ve yol haritası. Gözle kontrol burada.

Gerekçeler `discussion.md` → Karar 1–8.

## Kapsam Dışı

- Tam uzak entegrasyon: uzakta blok, dock aynası, betik ve terminfo taşıma
  (kitty `kitten ssh`, Ghostty ssh-integration) — `docs/YOL-HARITASI.md`'de
  satır.
- Uzun yerel komutlarda giriş satırını gizlemek — kullanıcı reddetti;
  `docs/YOL-HARITASI.md`'de reddedilmiş olarak satır.
- `~/.ssh/config` çözümü, iç içe ssh'ta içteki host (gösterilen ilk atlama),
  pencere başlığını ayrıştırmak, sekme başlığında renk.
- `exec ssh`, sonradan ssh başlatan sarmalayıcı betik (Karar 2 → bilinen
  sınırlar).
- Ayar anahtarı (özellik her zaman açık).

## Akış

```
zsh preexec ── 133;C ──▶ reader: ShellLog Running'e geçti (nesil n)
                              │ Wake::command_started (kenar)
                              ▼
                   bt-shell ana kuyruk: running_command() == Some(n)
                              │ jobs: ön plan grubu → en üstteki ssh/mosh
                              │ (kararsız → silahlı, sonraki wake'te tekrar)
                              ▼
                   Session::set_remote(n, "prod")  ──▶ refresh_title: "⇄ …"
                              │
          ┌───────────────────┴────────────────────┐
          ▼                                        ▼
 frame(): input_rows = 0                 dock::render_context:
 bt-gpu: bant fazlası < 0,               "⇄ prod  /var/www/app" (info + dim)
 ızgara aşağı, doldurma tepeyi           üst saç çizgisi info
 kapatır, Slide süzülür
          ▲
 uzak OSC 7 ──▶ uzak yuva (etkinken her OSC 7)
 133;D / A ──▶ uzak durum + uzak yuva silinir → title_changed, bant geri süzülür
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | |
| kapı | |
