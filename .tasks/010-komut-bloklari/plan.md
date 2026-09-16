# Komut blokları

## Hedef

Kabuğun bastığı OSC 133 işaretleri ilk kez ürüne dönüşür: her komut, sol
kenarda çıkış koduna göre renklenen bir şeritle kendi bloğu olarak görünür.
Şerit kaydırma, pencere boyutlandırma ve geçmiş dolduktan sonra da doğru
satırda durur.

## Gereksinimler

- **R1** — Kabuk her prompt'a artan bir blok kimliği basar ve o kimlik
  ızgarada, prompt'un hücrelerinde taşınır.
  - **R1.1** — PS1 eki **sabit** kalır; kimlik `psvar` ile prompt anında
    genişler (mevcut idempotent nöbet örüntüsü korunur).
  - **R1.2** — Sarmalayıcı hiçbir kolda ölümcül değildir ve kullanıcının
    rc dosyasına yazmaz (009 sözleşmesi).
  - **R1.3** — Kimlik `D` işaretinin yükünde de gelir (`D;{kod};aid={N}`).
- **R2** — `bt-core` blok kimliğini çıkış koduna bağlayan bir defter tutar.
  - **R2.1** — Defter `aid → çıkış kodu`; satır, süre ve safha **yok**.
  - **R2.2** — Sabit halka: tavan `scrollback` mertebesinde, tahliye üstüne
    yazmayla. Tahliye sinyali beklenmez (alacritty yayınlamıyor).
  - **R2.3** — Yaprak kilit; `Term` kilidinin altına girmez.
- **R3** — `frame()` sınırı blokları **çözülmüş** verir: satır aralığı ve
  renk. Çıkış kodu sınırı geçmez.
  - **R3.1** — İki fazlı: `Term` altında yalnız `(aid, ilk_satır)` çiftleri
    yeniden kullanılan bir tampona toplanır, kilit bırakıldıktan sonra renk
    çözülür.
  - **R3.2** — Kimlikler **oturum başına** artandır (`exec zsh` sayacı
    sıfırlar). İlk görünür çıpanın **üstü** bir önceki bloğa aittir.
    **Bilinmeyen hiçbir hâlde çizilmez:** üstteki kimlik defterde yoksa
    (halka dolaştı, sayaç sıfırlandı) ya da hiç çıpa görünmüyorken kabuk
    `Input`'taysa şerit çizilmez. Yanlış çizmemek M'nin savunma tezidir ve
    köşede de tutar. Hiç çıpa yokken kabuk `Running`'se pencere son `A`'nın
    bloğuna aittir.
  - **R3.3** — Alternatif ekranda blok verilmez.
  - **R3.4** — Çıpa okuması hücrenin atlama kapısından **sonra** gelir.
- **R4** — `bt-gpu` şeridi `Frame`'de **kendi listesinden** çizer.
  - **R4.1** — `bg` / `bg_count` sözleşmesi ve `move_cursor`'ın truncate'i
    dokunulmadan kalır; `hucre=` jetonunun anlamı oynamaz.
  - **R4.2** — Yeni pipeline ya da shader yok: mevcut `cell_bg`'nin genel
    piksel dörtgeni.
  - **R4.3** — Animasyon yok; şerit anında belirir. Boşta sıfır kare
    sözleşmesi dokunulmadan kalır.
- **R5** — Sol kenardan sabit bir pay ayrılır ve **tek sabitten** üç
  tüketiciye gider: `cols` hesabı, çizim orijini, fare eşlemesi.
  - **R5.1** — Pay her zaman ayrılır; oturum ortasında değişmez.
  - **R5.2** — Alternatif ekranda pay ayrılmış kalır, yalnız şerit çizilmez.
- **R6** — Temaya yalnız **çizilen** durum rolü eklenir; gömülü iki tema ve
  `docs/AYARLAR.md` aynı commit'te güncellenir.
  - **R6.1** — Eksik ve bilinmeyen anahtar iki yönde de sessiz kalmaya
    devam eder; kullanıcı temaları okunmayı sürdürür.

## Yaklaşım

1. **Betik çıpayı basar.** `bateri.zsh` bir blok sayacı tutar; `precmd`
   sayacı artırıp `psvar`'a yazar, `A` ve `D` işaretleri `aid={N}` alanıyla
   çıkar. PS1'e **sabit** bir önek/sonek çifti girer: önek
   `%{\e]8;;bateri://block/%1v\a%}`, sonek mevcut `B` ekinin yanında
   `%{\e]8;;\a%}`. Sayaç `__bateri_restore`'un sildiği izlerden sağ çıkar.
2. **Tarayıcı kimliği taşır.** `Mark`'lara kimlik alanı eklenir;
   `parse_mark` `aid=` alanını okur (bugün tolere ediyor, düşürüyor).
   Modülün saflığı korunur: saat okunmaz, kilit görülmez.
3. **Defter doğar.** `Session`'da yaprak kilitte sabit halka: `aid → çıkış
   kodu`. Okuyucu thread yazar, `frame()` okur.
4. **`frame()` blokları verir.** Hücre döngüsünde, atlama kapısından sonra,
   hücrenin hyperlink'inden kimlik çekilir; `Term` kilidi altında
   `(aid, ilk_satır)` toplanır. Kilit bırakıldıktan sonra kimlikler
   defterden renklendirilir ve blok aralıkları sınırdan geçer.
   Bloğun **sonu** bir sonraki kimliğin bir üstü; son bloğun sonu pencerenin
   altıdır. Şerit prompt satırından başlar — blok komutun kendisini de
   kapsar, yalnız çıktısını değil.
5. **Renderer şeridi çizer.** `Frame` ayrı bir liste ve ayrı bir draw call
   kazanır; `cell_bg` pipeline'ı, piksel uzayında dikdörtgen.
6. **Geometri payı açılır.** Gutter genişliği tek sabit; `cols` hesabı,
   çizim orijini ve fare eşlemesi ondan beslenir.
Tema rolleri adım 4 ile **birlikte** iner, çizimle değil: renk sınırdan
çözülmüş geçtiği için rolün ilk tüketicisi `frame()`, çizen taraf renk
üretmiyor. `CLAUDE.md`'nin eskiyen cümleleri değiştikleri phase'in
commit'inde düzelir.

## Kapsam Dışı

- **Şeridin belirme animasyonu** — ertelendi (Karar 5); yol haritasının
  "(+ blok animasyonları)" kalemi borç olarak kalır.
- **Komut süresi ve `command_duration_threshold`** — süre bu sette
  kaydedilmiyor; gerektiğinde `D`'nin yükünden gelir.
- **`command_gutter` ayarı ve ayar ayrıştırma yardımcısı** (Karar 6).
- **Prompt'u terminalin çizmesi** (`B`'nin asıl tüketicisi) → 011.
- **Katlama, `block_depth`, komutlar arası atlama.**
- **bash ve fish betikleri** — çıpa satırı onlara da yazılacak, ama
  betikler bu sette doğmuyor.
- **Sekme/bölme** — blok defteri "bir pencere = bir oturum" varsayımıyla
  iniyor; 014 retrofit eder (yol haritası bu bedeli yazıyor).
- **Ölçüm** — bu set kare süresi, gecikme ya da bellek **iddiası
  taşımıyor**; uydurulmuş bir "ölçüm bekliyor" satırı yazılmaz.

## Göç

- **Tema dosyası biçimi büyüyor** (yeni durum rolü). Kullanıcı temaları
  okunmaya devam eder: eksik anahtar yuvaya dokunmadan geçiyor ve gömülü
  `bateri`'den miras alınıyor (`theme.rs`). Kendi açık temasını yazmış
  kullanıcı rolü koyu temadan miras alır — `dim` ile aynı, kabul edilmiş ve
  belgelenen kusur; `docs/AYARLAR.md`'nin o uyarısı genişler.
- **Ayar anahtarı değişmiyor**, göç yok.
- **Betik güncelleniyor:** açık oturumlar eski betikle koşmaya devam eder,
  yeni pencere yenisini alır. Eski betikle açılmış oturumda çıpa yok, yani
  şerit yok — hata değil, geri düşüşün kendisi.
- **`cols` bir azalabilir.** Sabit ölçülü sınamalar (`split_into_grid`)
  mekanik olarak düşer ve aynı commit'te düzelir.

## Akış

```
zsh precmd: aid++ → psvar[1]=aid → OSC 133 A;aid=N → prompt (PS1 önekinde OSC 8)
zsh preexec: OSC 133 C            zsh precmd: OSC 133 D;kod;aid=N
        │                                 │
        ▼                                 ▼
  TappedPty::read ── Scanner ── Mark{aid} ──► defter: aid → çıkış kodu
        │                                         (yaprak kilit, sabit halka)
        ▼                                              │
  EventLoop::advance ──► Term: OSC 8 hücrelerde ◄──────┤
                                   │                   │
                     Session::frame │ faz 1: Term kilidi altında
                                   │   (aid, ilk_satır) topla
                                   │ faz 2: kilit bırakıldı, renk defterden
                                   ▼
                   Cell'ler + Cursor + Block{satır aralığı, renk}
                                   ▼
                  bt-gpu: ayrı liste ──► cell_bg pipeline (dikdörtgen)
```

## Durum

Her phase'in kapısı `make hepsi`. Ek doğrulama (`proje.md` → Doğrulama):
phase-1 `make kur` (`assets/shell/*`) ve `make test-yaris` (okuyucu thread ↔
kare üreten taraf), phase-2 `make test-yaris`, phase-3 ve phase-4
`make duman` (pencereyi açan davranış).

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| kapı | |
