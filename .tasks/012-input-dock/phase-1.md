# Phase 1 — Dock kanalı: ikinci OSC kolu

## Özet

Tarayıcı OSC 133'ün yanında ikinci bir numarayı daha tanısın, base64 gövdeyi
çözsün ve sınır aşımını **görünür** bir sonuçla bildirsin.

_Requirements: R1.2, R1.3_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `Scanner` bugün numarayı `133`'e
  kilitliyor ve başkasını tampona hiç uğratmadan eliyor; ikinci kol açılır.
  - **Numara seçimi bir karardır:** `docs/ARASTIRMA.md`'nin desteklenen dizi
    listesiyle (0, 7, 8, 9, 12, 52, 104, 133, 777) ve yaygın terminal
    kullanımıyla çakışmayan bir numara seçilir; seçim gerekçesiyle doc'a
    yazılır. Başka terminalde tanımsız davranış üretmemeli.
  - **Kendi sınırı.** `PAYLOAD_LIMIT` (256) 133 için doğru ama bir komut satırı
    onu rahat aşar. Dock kolu kendi sınırını taşır; sayı **türetilir** (tipik
    satır uzunluğu × base64 şişmesi) ve doc'ta gerekçelenir.
  - **Aşım görünür.** Bugünkü `Skip` kolu çağırana hiçbir sinyal vermiyor;
    dock kolunda aşım bir **sonuç** olarak döner ki tüketici "gösteremiyorum"
    diyebilsin. Sessiz düşüş bu deponun yasakladığı belirti sınıfı.
  - Base64 çözme elle yazılır — yeni bağımsız bir crate **mimari karardır**
    (`proje.md`), bu phase onu açmaz.
- **`crates/bt-core/src/shell.rs`** — `DockState`: çözülmüş görüntü durumu
  (metin, caret sütunu, renk aralıkları, öneri kuyruğu). Yaprak kilit altında,
  `ShellLog`'un yanında. **Yeniden kullanılan tampon**, kare başına allocation
  değil — ölçüt `CLAUDE.md`'nin kare başına maliyet kuralı.
- **`crates/bt-core/src/session.rs`** — `Session::dock_state()`, `shell_state()`
  ile aynı örüntü: yaprak kilidi alıp bırakır, `Term` kilidine dokunmaz.

Bu phase'te **hiçbir şey çizilmiyor**; kanal açılıyor ve depolanıyor.

## Kabul

- Tarayıcı iki numarayı da tanıyor; 133'ün davranışı **birebir** değişmedi
  (mevcut sınamalar dokunulmadan geçer).
- Base64 gövde çözülüyor; bozuk gövde **panik değil** yoksayma üretiyor
  (`CLAUDE.md` → PTY ve ayrıştırma yolunda panik yok).
- Sınır aşımı çağırana görünür bir sonuç veriyor; sınama onu çiviliyor.
- Çıplak `ESC`'in diziyi bitirmesi kuralı dock kolunda da geçerli.
- `make test-yaris` yeşil: okuma yolu ile kare yolu arasında yeni bir
  paylaşılan durum var ve kilit sırası (`term` → `shell`) bozulmuyor.

## Yayın Etkisi

- **Riskli phase:** okuma yolu ve paylaşılan durum değişiyor →
  `make test-yaris` **ve** phase sonunda `/code-review` (`proje.md` → Kalite
  kapısı).
- **`CLAUDE.md`:** `bt-core` satırı tarayıcının artık yalnız 133 olmadığını
  söylemeli.
- Yeni bağımlılık: **yok** (base64 elle). Ayar şeması, tema, app bundle,
  terminfo: yok. Shader: yok.
- Ölçüm bekleyen iddia: **yok** (bu phase çizmiyor, akış maliyeti phase-2'de
  doğuyor).

## Checklist

- [ ] İkinci OSC numarası seçildi ve gerekçesi doc'ta
- [ ] Base64 çözme; bozuk gövde yoksayılıyor, panik yok
- [ ] Dock kolunun kendi sınırı türetildi ve doc'ta gerekçeli
- [ ] Sınır aşımı **görünür** bir sonuç (sessiz düşüş yok)
- [ ] `DockState` yeniden kullanılan tampon; `Session::dock_state()` yaprak
      kilitten okuyor
- [ ] Test: 133'ün davranışı değişmedi (mevcut tarayıcı sınamaları)
- [ ] Test: sınır aşımı ve bozuk base64
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Yayın etkisi yazıldı
