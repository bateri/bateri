# Tazelik zamansal olsun

## Hedef

Kullanıcı yazarken caret dock'tan **çıkmasın**. Bugün zsh'in bir karakteri
kendisi dönüştürdüğü her hâlde (`🥰` → `<0001f970>`) tazelik kapısı aynayı
bayat sanıyor, bastırma kalkıyor ve caret ızgaraya sıçrıyor. Kapı "ayna ile
ızgara aynı şeyi mi söylüyor" yerine önce **"gönderdiğim tuşun aynası geldi
mi"** diye sorsun; cevap gelmediyse bugünkü içerik kapısı aynen dursun.

## Gereksinimler

- **R1** — Kapı iki yollu: `fresh = answered || içerik_kapısı`.
  - **R1.1** — `içerik_kapısı` bugünkü ifadenin **aynısı** (`last_ink`
    eşitliği ve `at_anchor`); çıpa yarısı kalıyor.
  - **R1.2** — `answered` doğruysa `last_ink_in_row` taraması **koşmuyor**
    (kısa devre); kare yoluna iş eklenmiyor, çıkıyor.
  - **R1.3** — Saat, tolerans ve yeni kare talebi **yok**.
- **R2** — `answered` iki damgadan.
  - **R2.1** — `Session.key_gen` (`Arc<AtomicU64>`) kullanıcı girdisinin tek
    hunisi `send_input`'ta, `Msg::Input` gönderilmeden **önce** artıyor.
    `paste` iki kolunda da oradan geçiyor (`write_owned` → `send_input`);
    `Adapter::reply` ve tekerlek raporu geçmiyor ve geçmemeli — onlar tuş değil.
  - **R2.2** — Ayna çözüldüğü anda `key_gen` okunup `DockState::answers`'a
    yazılıyor; yazar tek (okuyucu thread, `apply_scan`), içerikle **aynı**
    kilit turunda.
  - **R2.3** — `SuppressedInput` damgayı `last_ink` ile aynı yaprak kilit
    turunda taşıyor; bayat okuma bayat damga getiriyor → bugünkü kapı.
- **R3** — Dock'un gösteremediği satır ızgarada: sekme dışında bir kontrol
  karakteri taşıyan görüntü `DockStatus::Control` (`Multiline`'ın kardeşi).
  - **R3.1** — Karar `decode_line`'da, satır sonu kontrolünün yanında; kapı
    ona dokunmuyor, `Live` olmayan ayna zaten bastırmıyor.
  - **R3.2** — Sekme istisna: bilgi taşımıyor, Ctrl-V Tab satırı dock'ta kalıyor.
  - **R3.3** — `^A` konumdan bağımsız ızgarada ve okunur (bugün yalnız son
    karakterse öyle; ortadaysa sessizce kayboluyordu).
- **R4** — İki bilinen sınır adıyla yazılı (`discussion.md` → Karar 2): tuş +
  hemen yapıştırma penceresi ve kabuğun dışından yazım.
- **R5** — Sözleşme aynı commit'te: `CLAUDE.md`'nin bayatlık paragrafı,
  `shell.rs`'in `^A` yorumu, `docs/YOL-HARITASI.md` (borç kapanır, `^X`
  kalemi açılır, numaralar kayar).

## Yaklaşım

1. **`shell.rs`** — `DockState` `answers: u64` kazanıyor (`clone_from` ve
   `reset` taşıyor). `DockStatus::Control` doğuyor ve `decode_line` onu
   `Multiline`'ın yanında kuruyor. `apply_scan` damgayı argüman olarak
   alıyor; `SuppressedInput` onu taşıyor. `^A` yorumu yeni gerçeğe dönüyor.
2. **`dock.rs`** — `Control` kolunun eşleşmelerde `Multiline` gibi
   davranması (satır başında caret, metin yok); yeni fonksiyon yok.
3. **`session.rs`** — `key_gen` `Session`'da ve `TappedPty`'de
   (`screen_clears` emsali, `Arc` paylaşımı). `send_input` artırıyor, okuyucu
   `apply_scan`'e o anki değeri veriyor. Kapı yeni biçim, `answered`
   kısa devreyle önde.
4. **Bekçiler** — iki mevcut bekçi (`a_stale_mirror_leaves_the_input_line_in_the_grid`,
   `a_blank_mirror_below_the_anchor_is_stale`) bayatlığı `printf`'le kuruyor ve
   kullanıcı girdisi yok, yani yeni kapıda "cevap geldi" sayılırlar: bayatlık
   bir **girdiyle** (`paste`/`write`) kurulacak; ikincisinin adı da değişiyor,
   çünkü gerçekten gelmiş boş bir ayna artık taze. Yeniler: `<hex>` şekli
   taze; `^A` ortada ve sonda ızgarada; tuş→yapıştırma penceresi sınır
   olarak; bir `race_*`.
5. **Belgeler** — `CLAUDE.md`, yol haritası, indeks.

## Kapsam Dışı

- **`^X` yer tutucusu** — dock'un kontrol karakterini zsh gibi çizmesi. TAB,
  DEL ve `^[` kendi kararlarını istiyor; ayrı kalem. Geldiği gün
  `DockStatus::Control` kolu silinir.
- **`<hex>`'in ızgaradaki hâli** — zsh'in yazdığı şey, kullanıcı da "onun
  kapsamı" dedi.
- **Tazelik kapısını `HANDOVER_HOLD`'un altına almak** — İşletme'nin
  gözlemi doğru (her yanlış verdikt anında bir sıçrama) ama çaresi yanlış
  verdikti azaltmak, onu sönümlemek değil: bayat aynada caret'i dock'ta
  tutmak 012'nin iki caret kusurunu geri getirir.
- **`write_gen`** — kabuğun dışından yazımı ayırırdı; panel pahalı buldu,
  sınır olarak yazılı.

## Akış

```
ana thread                        okuyucu thread (TappedPty::read)
send_input ─ key_gen += 1         8133;u ayrıştırıldı
   └─ Msg::Input ──▶ PTY ──▶ ZLE ──▶ apply_scan(event, key_gen.load())
                                        └─ DockState { status (Live|Control|…),
                                                       içerik, last_ink, answers }
kare yolu (Session::frame)
  suppressed_input()  ── yaprak kilit, tek tur ──▶ { last_ink, answers }   (Control ⇒ None)
  answered = answers == key_gen.load()
  fresh = answered || (last_ink_in_row == last_ink && at_anchor)
  caret_in_dock ... && (suppressed.is_none() || suppress_to.is_some())   ← değişmiyor
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ⏳ |
| kapı | ⏳ |
