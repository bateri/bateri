# Phase 4 — Önizleme ve temizlik

## Özet

⌘-tık dosyayı önizleme klasörüne indirip salt okunur açar; önbellek açılışta,
günde bir kez ve Clear Now ile temizlenir.

_Requirements: R5, R5.1, R5.2, R5.3, R5.4, R6_

## Değişiklikler

- **`crates/bt-shell-macos/src/hyperlink.rs`** — uzak hit'te `open_link`
  `remote_files`'ın politikasına gider: dosya → önizleme, klasör → hiçbir
  şey; Open Preview menü öğesi etkin.
- **`crates/bt-shell-macos/src/preview.rs`** (yeni) — önizleme akışı: boyut
  `preview_max_size`'ı aşıyorsa sayfa (R5.2: "Open Preview", "Save to
  Downloads instead"); önbellekte aynı boyut+mtime varsa doğrudan aç (R5.4);
  yoksa önizleme şeridine iş; bitince `0444` (ayar `preview_read_only`),
  bateri'nin yazdığı boyut/mtime kaydı ve son açılış damgası (önbelleğin
  küçük indeks dosyası, `{preview_dir}/.index`), sonra açma: düz metin kolu
  `NSWorkspace.URLForApplicationToOpenContentType(public.plain-text)` +
  `openURLs:withApplicationAtURL:`, diğeri `openURL`.
- **`crates/bt-shell-macos/src/app.rs`** — açılışta temizlik (arka planda,
  planlayıcı + uygulayıcı), günde bir kez yalnız saklama (`dispatch` gecikmeli
  iş; boşta kare üretmez — kare yoluna dokunmaz), farklılaşmış kopyayı
  `download_dir`'e taşıma + bildirim. Clear Now'ın çağıracağı tek yöntem.
- **`crates/bt-shell-common/src/remote_files.rs`** — gerekiyorsa indeksin
  okuma/yazma biçimi (saf).

## Kabul

- Sınamalar: indeks round-trip; temizlik uygulayıcısı geçici bir dizinde
  (saklama, boyut sınırı en eski önce, farklılaşmış kopyanın taşınması,
  bozuk indeks → hiçbir şey silinmez).
- `make check` yeşil.
- Gözle kontrol: küçük metin dosyası ⌘-tıkta açılır ve "Read Only" görünür;
  `.sh` TextEdit'te düz metin; 100 MB üstünde soru ve "Save to Downloads
  instead"; ikinci ⌘-tık yeniden indirmez; TextEdit'te Unlock + değiştirilen
  kopya bir sonraki açılışta Downloads'a taşınıp bildirilir.

## Checklist

- [ ] ⌘-tık → önizleme akışı, sınır sayfası, önbellekten açma
- [ ] Salt okunur + düz metin kolu
- [ ] İndeks; açılış ve günlük temizlik; farklılaşmış kopyanın taşınması
- [ ] Test: Kabul listesi
- [ ] Doğrulama geçti (`make check`)
